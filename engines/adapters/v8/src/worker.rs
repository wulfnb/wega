//! Worker realm runtime: owns the Deno runtime, its executor and module evaluation for one
//! worker thread. The embedder keeps worker orchestration (hubs, messages, the event
//! loop's command channel, Canvas/WebGL host operations) and drives this runtime through
//! a small, Deno-free API.
//!
//! Thread affinity: a `WorkerRuntime` is created on, used from and dropped on one thread
//! (it is `!Send`). Only the [`WorkerTerminator`] crosses threads.
//!
//! Reentrancy: host callbacks (`NativeHost`) run *inside* `execute`, `load_main_module` and
//! `poll`; they must not call back into the same `WorkerRuntime` (it is mutably borrowed)
//! and must not block on work that needs this thread's event loop.
use crate::runtime::{RuntimeSpec, configure_secure_context, create_runtime};
use deno_core::JsRuntime;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

type ModuleEvaluation = Pin<Box<dyn Future<Output = Result<(), deno_core::error::CoreError>>>>;

/// Thread-safe handle that interrupts script running on the worker's isolate.
#[derive(Clone)]
pub struct WorkerTerminator(deno_core::v8::IsolateHandle);

impl WorkerTerminator {
    /// Terminates JavaScript currently executing in the isolate (also stops a worker stuck in
    /// script). Safe from any thread, at any time; a no-op once the isolate is gone.
    pub fn terminate(&self) {
        self.0.terminate_execution();
    }
}

/// What the event loop did during one [`WorkerRuntime::poll`] turn.
#[derive(Debug)]
pub enum LoopStatus {
    /// The turn's time budget elapsed with work still pending.
    TimedOut,
    /// Nothing is pending and no module is evaluating: the embedder may sleep until it has work.
    Idle,
    /// The event loop is drained but the main module's evaluation is still pending.
    EvaluatingModule,
    /// The event loop failed (an uncaught error from a task or callback).
    Failed(String),
}

#[derive(Debug)]
pub struct Turn {
    /// Set once, on the turn in which the main module's evaluation finished.
    pub module: Option<Result<(), String>>,
    pub status: LoopStatus,
}

pub struct WorkerRuntime {
    // Dropped explicitly inside the executor (see `Drop`).
    runtime: Option<JsRuntime>,
    executor: tokio::runtime::Runtime,
    evaluation: Option<ModuleEvaluation>,
    out_of_memory: Arc<AtomicBool>,
}

impl WorkerRuntime {
    /// Creates the worker's executor and runtime (via [`create_runtime`] with
    /// `RuntimeKind::Worker`) and installs the heap-limit guard. Bootstrap scripts and
    /// secure-context exposure are separate steps so the embedder can seal its host between.
    pub fn new(spec: RuntimeSpec) -> Result<Self, String> {
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(4)
            .build()
            .map_err(|error| error.to_string())?;
        let mut runtime = {
            let _entered = executor.enter();
            create_runtime(spec)?
        };
        let out_of_memory = Arc::new(AtomicBool::new(false));
        let (flag, terminate) = (
            out_of_memory.clone(),
            runtime.v8_isolate().thread_safe_handle(),
        );
        runtime.add_near_heap_limit_callback(move |current, _initial| {
            flag.store(true, Ordering::Relaxed);
            terminate.terminate_execution();
            current.saturating_mul(2)
        });
        Ok(Self {
            runtime: Some(runtime),
            executor,
            evaluation: None,
            out_of_memory,
        })
    }

    fn runtime(&mut self) -> &mut JsRuntime {
        self.runtime.as_mut().expect("runtime lives until drop")
    }

    pub fn terminator(&mut self) -> WorkerTerminator {
        WorkerTerminator(self.runtime().v8_isolate().thread_safe_handle())
    }

    /// True once V8 reported the near-heap-limit condition (script was terminated).
    pub fn exceeded_heap_limit(&self) -> bool {
        self.out_of_memory.load(Ordering::Relaxed)
    }

    pub fn configure_secure_context(&mut self, url: &url::Url) -> Result<(), String> {
        let _entered = self.executor.enter();
        configure_secure_context(self.runtime(), url)
    }

    /// Runs a classic script. The error is the script's exception text.
    pub fn execute(&mut self, name: &str, source: String) -> Result<(), String> {
        let _entered = self.executor.enter();
        let name = name.to_owned();
        self.runtime()
            .execute_script(name, source)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    /// Loads the main ES module and starts its evaluation; completion is reported by
    /// [`WorkerRuntime::poll`]. Blocks the calling thread while imports load.
    pub fn load_main_module(&mut self, url: &url::Url, source: String) -> Result<(), String> {
        let _entered = self.executor.enter();
        let runtime = self.runtime.as_mut().expect("runtime lives until drop");
        let id = self
            .executor
            .block_on(runtime.load_main_es_module_from_code(url, source))
            .map_err(|error| error.to_string())?;
        self.evaluation = Some(Box::pin(runtime.mod_evaluate(id)));
        Ok(())
    }

    /// Drives the event loop (and the main module's evaluation) for at most `budget`.
    pub fn poll(&mut self, budget: Duration) -> Turn {
        let _entered = self.executor.enter();
        let runtime = self.runtime.as_mut().expect("runtime lives until drop");
        let evaluation = &mut self.evaluation;
        let turn = self.executor.block_on(async {
            let work = async {
                if let Some(pending) = evaluation.as_mut() {
                    tokio::select! {
                        biased;
                        result = pending => (Some(result), Ok(())),
                        result = runtime.run_event_loop(Default::default()) => (None, result),
                    }
                } else {
                    (None, runtime.run_event_loop(Default::default()).await)
                }
            };
            tokio::time::timeout(budget, work).await
        });
        match turn {
            Err(_elapsed) => Turn {
                module: None,
                status: LoopStatus::TimedOut,
            },
            Ok((module, looped)) => {
                let module = module.map(|result| {
                    self.evaluation = None;
                    result.map_err(|error| error.to_string())
                });
                let status = match looped {
                    Err(error) => LoopStatus::Failed(error.to_string()),
                    Ok(()) if self.evaluation.is_none() => LoopStatus::Idle,
                    Ok(()) => LoopStatus::EvaluatingModule,
                };
                Turn { module, status }
            }
        }
    }
}

impl Drop for WorkerRuntime {
    fn drop(&mut self) {
        // Destruction order: pending module evaluation (borrows nothing from the runtime but
        // may hold resources), then the runtime and isolate, all inside the executor's
        // context (Deno resources may need it), and only then the executor itself.
        let _entered = self.executor.enter();
        self.evaluation = None;
        drop(self.runtime.take());
    }
}
