//! V8 isolates are confined to the owning DOM thread; only termination crosses threads.
use crate::{
    env::ScriptEnvironment,
    runtime::{RuntimeKind, RuntimeSpec, configure_secure_context, create_runtime},
    script::{ScriptError, ScriptHost},
};
use deno_core::JsRuntime;
use serde_json::json;
use std::{
    rc::Rc,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

/// Limits the script host enforces (a subset of the embedder's limits).
#[derive(Clone, Debug)]
pub struct RuntimeLimits {
    pub js_heap_bytes: usize,
    /// Hard limit for one script task; exceeding it tears the page's VM down.
    pub script_time: Duration,
    /// Tasks longer than this are reported as slow but keep running.
    pub slow_script: Duration,
    /// Bound for module subresources.
    pub resource_bytes: usize,
}

/// Embedder-supplied inputs for a document host.
pub struct HostConfig {
    pub limits: RuntimeLimits,
    /// Snapshot produced by `build_startup_snapshot`.
    pub snapshot: &'static [u8],
    /// Trusted bootstrap script run in the restored runtime before page code.
    pub bootstrap: &'static str,
}
type ModuleLoad = std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = (
                    JsRuntime,
                    Result<deno_core::ModuleId, deno_core::error::CoreError>,
                ),
            >,
    >,
>;
type ModuleEvaluation =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), deno_core::error::CoreError>>>>;
fn heap_bytes(runtime: &mut JsRuntime) -> usize {
    let stats = runtime.v8_isolate().get_heap_statistics();
    stats
        .used_heap_size()
        .saturating_add(stats.external_memory())
}
/// Makes an isolate the thread's current isolate for the guard's lifetime.
///
/// This is the engine's only `unsafe` code. Entering is needed because garbage
/// collection, memory-pressure notifications and the watchdog-guarded script steps
/// can run native/weak callbacks that allocate V8 handles, which requires a current
/// isolate; `deno_core::scope!` does not enter the isolate itself.
///
/// Safety invariants (all hold for every use in this module):
///
/// 1. **Balanced and thread-local.** `enter` happens in `new` and the matching `exit`
///    in `drop` on the same thread. The raw pointer makes the guard `!Send`, so it
///    cannot be moved to another thread.
/// 2. **Valid pointer.** The pointer comes from the isolate of a live `JsRuntime`
///    owned by this `V8Host` (or borrowed as `&mut JsRuntime`). Guards are stack
///    locals created and dropped inside one method; no path drops the runtime while
///    a guard exists. Moving the `JsRuntime` value (into the module-loading future in
///    `run_module`) does not move or free the V8-owned isolate.
/// 3. **LIFO nesting.** Guards may nest (`bounded` -> `collect_garbage`); because they
///    are stack-scoped they are dropped in reverse creation order, which V8's
///    entered-isolate stack requires.
/// 4. **No aliasing across the call.** `new` takes `&mut Isolate` only to enter; the
///    guard stores the pointer without borrowing, and the runtime is only used
///    through its own `&mut` paths while the guard is alive (never concurrently:
///    the host is single-threaded and the watchdog uses a separate thread-safe
///    `IsolateHandle`).
#[allow(unsafe_code)]
struct RawIsolateGuard(*mut deno_core::v8::Isolate);

#[allow(unsafe_code)]
impl RawIsolateGuard {
    fn new(isolate: &mut deno_core::v8::Isolate) -> Self {
        // SAFETY: invariants 1-4 above; the matching `exit` is in `Drop`.
        unsafe {
            isolate.enter();
        }
        Self(isolate as *mut _)
    }
}

#[allow(unsafe_code)]
impl Drop for RawIsolateGuard {
    fn drop(&mut self) {
        // SAFETY: the isolate outlives the guard (invariant 2) and this thread entered it (invariant 1).
        unsafe {
            (*self.0).exit();
        }
    }
}

fn collect_garbage(runtime: &mut JsRuntime, critical: bool) {
    let _guard = RawIsolateGuard::new(runtime.v8_isolate());
    // GC can invoke native/weak callbacks that allocate V8 handles.
    deno_core::scope!(scope, runtime);
    if critical {
        scope.memory_pressure_notification(deno_core::v8::MemoryPressureLevel::Critical);
    }
    scope.low_memory_notification();
}
struct WakeLoop(Arc<dyn Fn() + Send + Sync>);
impl Wake for WakeLoop {
    fn wake(self: Arc<Self>) {
        (self.0)();
    }
}

/// One long-lived watchdog thread per isolate. `arm` starts a deadline for one
/// script run; when it passes before `disarm`, execution is terminated.
struct Watchdog {
    shared: Arc<(Mutex<WatchState>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}
#[derive(Default)]
struct WatchState {
    deadline: Option<Instant>,
    generation: u64,
    fired: bool,
    shutdown: bool,
}
impl Watchdog {
    fn new(handle: deno_core::v8::IsolateHandle) -> Result<Self, String> {
        let shared = Arc::new((Mutex::new(WatchState::default()), Condvar::new()));
        let state = shared.clone();
        let thread = std::thread::Builder::new()
            .name("wega-watchdog".into())
            .spawn(move || {
                let (lock, wake) = &*state;
                let mut watch = lock
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                while !watch.shutdown {
                    match watch.deadline {
                        None => {
                            watch = wake
                                .wait(watch)
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                        }
                        Some(deadline) => {
                            let now = Instant::now();
                            if now >= deadline {
                                watch.fired = true;
                                watch.deadline = None;
                                handle.terminate_execution();
                            } else {
                                watch = wake
                                    .wait_timeout(watch, deadline - now)
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .0;
                            }
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }
    fn arm(&self, budget: Duration) {
        let (lock, wake) = &*self.shared;
        let mut watch = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        watch.generation += 1;
        watch.fired = false;
        watch.deadline = Some(Instant::now() + budget);
        wake.notify_one();
    }
    /// Stop the current deadline; returns whether it already fired.
    fn disarm(&self) -> bool {
        let (lock, wake) = &*self.shared;
        let mut watch = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        watch.deadline = None;
        wake.notify_one();
        watch.fired
    }
}
impl Drop for Watchdog {
    fn drop(&mut self) {
        {
            let (lock, wake) = &*self.shared;
            let mut watch = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            watch.shutdown = true;
            wake.notify_one();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct V8Host<E: ScriptEnvironment> {
    runtime: Option<JsRuntime>,
    loading_module: Option<ModuleLoad>,
    evaluating_modules: Vec<ModuleEvaluation>,
    queued: std::collections::VecDeque<(String, String, bool)>,
    watchdog: Watchdog,
    executor: Rc<tokio::runtime::Runtime>,
    env: Rc<E>,
    wake: Arc<dyn Fn() + Send + Sync>,
    budget: Duration,
    slow: Duration,
    oom: Arc<AtomicBool>,
    pending: bool,
    frozen: bool,
    heap: usize,
    heap_limit: usize,
    timer: Option<Instant>,
    polled_at: Instant,
}
impl<E: ScriptEnvironment> V8Host<E> {
    /// The document environment is created and owned by the view and injected here;
    /// the V8 host only runs script against it.
    pub fn new(env: Rc<E>, config: &HostConfig) -> Result<Self, String> {
        let limits = &config.limits;
        let executor = Rc::new(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .max_blocking_threads(8)
                .build()
                .map_err(|e| e.to_string())?,
        );
        let entered = executor.enter();
        // Do not enable lite mode per isolate: V8 flags are process-wide. Workers receive
        // their per-view option before initializing the platform.
        let document_url = env.document_url();
        let script_network = env.network();
        let host: Rc<dyn crate::native_ops::NativeHost> = env.clone();
        let mut runtime = create_runtime(RuntimeSpec {
            kind: RuntimeKind::Document,
            url: document_url.clone(),
            network: script_network,
            policy: env.network_policy(),
            host,
            snapshot: config.snapshot,
            js_heap_bytes: limits.js_heap_bytes,
            module_limit_bytes: limits.resource_bytes,
        })?;
        runtime.op_state().borrow_mut().put(String::new());
        runtime
            .execute_script("ext:wega_dom/bootstrap.js", config.bootstrap)
            .map_err(|error| error.to_string())?;
        runtime.op_state().borrow_mut().put(String::new());
        configure_secure_context(&mut runtime, &document_url)?;
        let bootstrap_heap = heap_bytes(&mut runtime);
        let oom = Arc::new(AtomicBool::new(false));
        let flag = oom.clone();
        let terminate = runtime.v8_isolate().thread_safe_handle();
        runtime.add_near_heap_limit_callback(move |current, _initial| {
            flag.store(true, Ordering::Relaxed);
            terminate.terminate_execution();
            current.saturating_mul(2)
        });
        let watchdog = Watchdog::new(runtime.v8_isolate().thread_safe_handle())?;
        drop(entered);
        Ok(Self {
            runtime: Some(runtime),
            loading_module: None,
            evaluating_modules: vec![],
            queued: Default::default(),
            watchdog,
            executor,
            wake: env.wake(),
            env,
            budget: limits.script_time,
            slow: limits.slow_script,
            oom,
            pending: false,
            frozen: false,
            heap: 0,
            heap_limit: bootstrap_heap.saturating_add(limits.js_heap_bytes),
            timer: None,
            polled_at: Instant::now(),
        })
    }
    /// Run one bounded step of script; reports a notice when it is slow.
    fn bounded<T>(&mut self, name: &str, run: impl FnOnce(&mut Self) -> T) -> (T, bool) {
        self.env.begin_task();
        let started = Instant::now();
        self.watchdog.arm(self.budget);
        let _guard = self
            .runtime
            .as_mut()
            .map(|rt| RawIsolateGuard::new(rt.v8_isolate()));
        let result = run(self);
        drop(_guard);
        let fired = self.watchdog.disarm();
        let elapsed = started.elapsed();
        if !fired && elapsed > self.slow {
            self.env.note_slow_script(format!(
                "{name}: slow script ({} ms); the page may be unresponsive",
                elapsed.as_millis()
            ));
        }
        (result, fired)
    }
    fn execute_value(
        &mut self,
        source: &str,
        name: &str,
    ) -> Result<deno_core::v8::Global<deno_core::v8::Value>, ScriptError> {
        if self.runtime.is_none() {
            return Err(ScriptError {
                fatal: false,
                message: "Module graph is loading".into(),
            });
        }
        let executor = self.executor.clone();
        let entered = executor.enter();
        let (result, timed_out) = self.bounded(name, |host| {
            host.runtime
                .as_mut()
                .unwrap()
                .execute_script(name.to_owned(), source.to_owned())
        });
        self.polled_at = Instant::now();
        self.heap = heap_bytes(self.runtime.as_mut().unwrap());
        if self.heap > self.heap_limit && !self.oom.load(Ordering::Relaxed) {
            collect_garbage(self.runtime.as_mut().unwrap(), false);
            self.heap = heap_bytes(self.runtime.as_mut().unwrap());
        }
        drop(entered);
        if self.oom.load(Ordering::Relaxed) || self.heap > self.heap_limit {
            return Err(ScriptError {
                fatal: true,
                message: format!(
                    "{name}: V8 heap limit exceeded ({} > {} bytes including bootstrap)",
                    self.heap, self.heap_limit
                ),
            });
        }
        if timed_out {
            return Err(ScriptError {
                fatal: true,
                message: format!("{name}: Script deadline exceeded"),
            });
        }
        result.map_err(|error| {
            let text = error.to_string();
            ScriptError {
                fatal: text.contains("Maximum call stack size exceeded"),
                message: format!("{name}: {text}"),
            }
        })
    }
    fn execute(&mut self, source: &str, name: &str) -> Result<String, ScriptError> {
        self.execute_value(source, name)
            .map(|value| format!("{value:?}"))
    }
}
impl<E: ScriptEnvironment> ScriptHost for V8Host<E> {
    type Requests = E::Requests;
    fn checkpoint(&mut self) -> Result<(), ScriptError> {
        // HTML's microtask checkpoint follows each script task. Timer dispatch
        // belongs to the timer task source, never to a parser-blocking script.
        if self.frozen || self.runtime.is_none() {
            return Ok(());
        }
        let executor = self.executor.clone();
        let _entered = executor.enter();
        let (_, timed_out) = self.bounded("microtask checkpoint", |host| {
            host.runtime
                .as_mut()
                .unwrap()
                .v8_isolate()
                .perform_microtask_checkpoint();
        });
        self.heap = heap_bytes(self.runtime.as_mut().unwrap());
        if timed_out || self.oom.load(Ordering::Relaxed) || self.heap > self.heap_limit {
            return Err(ScriptError {
                fatal: true,
                message: "Microtask checkpoint exceeded its execution or heap limit".into(),
            });
        }
        Ok(())
    }
    fn run(&mut self, source: &str, name: &str) -> Result<String, ScriptError> {
        if self.runtime.is_none() {
            if self.queued.len() >= 64 {
                return Err(ScriptError {
                    fatal: true,
                    message: "Script queue limit exceeded".into(),
                });
            }
            self.queued.push_back((source.into(), name.into(), false));
            return Ok("queued".into());
        }
        let result = self.execute(source, name);
        self.pending = true;
        if let Err(error) = &result
            && !error.fatal
        {
            let message = error.message.clone();
            let reported = self.execute(
                &format!(
                    "__wega_report(new Error({}));__wega_missing()",
                    json!(message)
                ),
                "wega:error",
            );
            if let Err(error) = reported
                && error.fatal
            {
                return Err(error);
            }
        }
        result
    }
    fn run_module(&mut self, source: &str, name: &str) -> Result<String, ScriptError> {
        if self.runtime.is_none() {
            if self.queued.len() >= 64 {
                return Err(ScriptError {
                    fatal: true,
                    message: "Script queue limit exceeded".into(),
                });
            }
            self.queued.push_back((source.into(), name.into(), true));
            return Ok("queued".into());
        }
        let specifier = url::Url::parse(name).map_err(|e| ScriptError {
            fatal: false,
            message: e.to_string(),
        })?;
        let mut runtime = self.runtime.take().unwrap();
        let code = source.to_owned();
        self.loading_module = Some(Box::pin(async move {
            let result = runtime
                .load_side_es_module_from_code(&specifier, code)
                .await;
            (runtime, result)
        }));
        self.pending = true;
        Ok("module queued".into())
    }
    fn modules_pending(&self) -> bool {
        self.loading_module.is_some()
            || !self.evaluating_modules.is_empty()
            || !self.queued.is_empty()
    }
    /// Run `source` and return its completion value as JSON, without `eval`
    /// (which a page's CSP may disable).
    fn evaluate_json(&mut self, source: &str) -> Result<String, ScriptError> {
        let value = self.execute_value(source, "evaluate_json")?;
        // JSON serialization calls user-defined toJSON/getters and must carry
        // the same watchdog/heap budget as the original evaluation.
        let executor = self.executor.clone();
        let _entered = executor.enter();
        let (output, timed_out) = self.bounded("JSON serialization", |host| {
            deno_core::scope!(scope, host.runtime.as_mut().unwrap());
            deno_core::v8::tc_scope!(let scope, scope);
            let local = deno_core::v8::Local::new(scope, value);
            let result = deno_core::v8::json::stringify(scope, local)
                .map(|text| {
                    if text.utf8_length(scope) > wega_protocol::MAX_RESULT_BYTES {
                        return Err(ScriptError {
                            fatal: false,
                            message: "Evaluation result exceeds the IPC budget".into(),
                        });
                    }
                    Ok(text.to_rust_string_lossy(scope))
                })
                .transpose()?;
            if let Some(exception) = scope.exception() {
                let message = exception
                    .to_string(scope)
                    .map(|text| text.to_rust_string_lossy(scope))
                    .unwrap_or_else(|| "JSON serialization failed".into());
                Err(ScriptError {
                    fatal: false,
                    message,
                })
            } else {
                Ok(result)
            }
        });
        self.heap = heap_bytes(self.runtime.as_mut().unwrap());
        if timed_out || self.oom.load(Ordering::Relaxed) || self.heap > self.heap_limit {
            return Err(ScriptError {
                fatal: true,
                message: "JSON serialization exceeded its execution or heap limit".into(),
            });
        }
        Ok(output?
            .filter(|text| text != "undefined")
            .unwrap_or_else(|| "null".into()))
    }
    fn tick(&mut self) -> Result<(), ScriptError> {
        if self.frozen {
            return Ok(());
        }
        let module_waker = Waker::from(Arc::new(WakeLoop(self.wake.clone())));
        let mut module_cx = Context::from_waker(&module_waker);
        if self.loading_module.is_some() {
            let (result, expired) = self.bounded("module loading", |host| {
                let load = host.loading_module.as_mut().unwrap();
                host.executor.block_on(async {
                    tokio::task::yield_now().await;
                    load.as_mut().poll(&mut module_cx)
                })
            });
            self.polled_at = Instant::now();
            match result {
                Poll::Pending => {
                    if expired || self.oom.load(Ordering::Relaxed) {
                        return Err(ScriptError {
                            fatal: true,
                            message: "Module loading exceeded its execution or heap limit".into(),
                        });
                    }
                    self.pending = true;
                    return Ok(());
                }
                Poll::Ready((runtime, result)) => {
                    self.runtime = Some(runtime);
                    self.loading_module = None;
                    let id = result.map_err(|e| ScriptError {
                        fatal: expired || self.oom.load(Ordering::Relaxed),
                        message: e.to_string(),
                    })?;
                    let executor = self.executor.clone();
                    let entered = executor.enter();
                    let (evaluation, expired) = self.bounded("module evaluation", |host| {
                        host.runtime.as_mut().unwrap().mod_evaluate(id)
                    });
                    drop(entered);
                    self.evaluating_modules.push(Box::pin(evaluation));
                    if expired || self.oom.load(Ordering::Relaxed) {
                        return Err(ScriptError {
                            fatal: true,
                            message: "Module execution exceeded its limit".into(),
                        });
                    }
                }
            }
        }
        while let Some((source, name, module)) = self.queued.pop_front() {
            if module {
                self.run_module(&source, &name)?;
            } else {
                self.run(&source, &name)?;
            }
            if self.runtime.is_none() {
                return Ok(());
            }
        }
        self.run("__wega_timers()", "timer dispatch")?;
        let waker = Waker::from(Arc::new(WakeLoop(self.wake.clone())));
        let mut cx = Context::from_waker(&waker);
        let (result, timed_out) = self.bounded("event loop", |host| {
            let runtime = host.runtime.as_mut().unwrap();
            host.executor.block_on(async {
                for _ in 0..4 {
                    tokio::task::yield_now().await;
                    let result = runtime.poll_event_loop(&mut cx, Default::default());
                    if result.is_ready() {
                        return result;
                    }
                }
                Poll::Pending
            })
        });
        self.polled_at = Instant::now();
        self.heap = heap_bytes(self.runtime.as_mut().unwrap());
        self.runtime
            .as_mut()
            .unwrap()
            .v8_isolate()
            .clear_kept_objects();
        if timed_out {
            return Err(ScriptError {
                fatal: true,
                message: "Event loop script deadline exceeded".into(),
            });
        }
        match result {
            Poll::Pending => {
                self.pending = true;
            }
            Poll::Ready(Ok(())) => {
                self.pending = false;
            }
            Poll::Ready(Err(e)) => {
                return Err(ScriptError {
                    fatal: self.oom.load(Ordering::Relaxed) || timed_out,
                    message: e.to_string(),
                });
            }
        }
        let mut index = 0;
        while index < self.evaluating_modules.len() {
            match self.evaluating_modules[index].as_mut().poll(&mut module_cx) {
                Poll::Pending => {
                    self.pending = true;
                    index += 1;
                }
                Poll::Ready(result) => {
                    drop(self.evaluating_modules.swap_remove(index));
                    result.map_err(|e| ScriptError {
                        fatal: self.oom.load(Ordering::Relaxed)
                            || e.to_string().contains("execution terminated")
                            || e.to_string().contains("Maximum call stack size exceeded"),
                        message: e.to_string(),
                    })?;
                }
            }
        }
        let next = self.evaluate_json("__wega_next_timer()")?;
        self.timer = serde_json::from_str::<Option<f64>>(&next)
            .ok()
            .flatten()
            .filter(|n| n.is_finite())
            .map(|ms| Instant::now() + Duration::from_secs_f64((ms.max(0.0) / 1000.0).min(60.0)));
        Ok(())
    }
    fn next_timer(&self) -> Option<Duration> {
        if self.frozen {
            return None;
        }
        let timer = self
            .timer
            .map(|when| when.saturating_duration_since(Instant::now()));
        if self.pending {
            let pending = (self.polled_at + Duration::from_millis(2))
                .saturating_duration_since(Instant::now());
            Some(timer.map_or(pending, |t| t.min(pending)))
        } else {
            timer
        }
    }
    fn frame(&mut self, _timestamp: f64) -> Result<bool, ScriptError> {
        if self.runtime.is_none() {
            return Ok(false);
        }
        self.run("__wega_frame(performance.now())", "wega:animation-frame")?;
        self.checkpoint()?;
        if self.runtime.is_none() {
            return Ok(false);
        }
        Ok(self.evaluate_json("__wega_has_frame()")? == "true")
    }
    fn heap_bytes(&self) -> usize {
        self.heap
    }
    fn take_errors(&mut self) -> Vec<String> {
        self.env.take_errors()
    }
    fn take_requests(&mut self) -> Self::Requests {
        self.env.take_requests()
    }
    fn allow_eval(&mut self, allowed: bool) {
        if let Some(runtime) = &mut self.runtime {
            let _guard = RawIsolateGuard::new(runtime.v8_isolate());
            deno_core::scope!(scope, runtime);
            scope
                .get_current_context()
                .set_allow_generation_from_strings(allowed);
        }
    }
    fn budget(&self) -> (usize, usize) {
        self.env.budget()
    }
    fn sync_nodes(&mut self, count: usize) {
        self.env.sync_nodes(count);
    }
    fn history(&self) -> (usize, usize) {
        self.env.history()
    }
    fn freeze(&mut self, frozen: bool) {
        self.frozen = frozen;
        if frozen && let Some(runtime) = &mut self.runtime {
            let _entered = self.executor.enter();
            let _guard = RawIsolateGuard::new(runtime.v8_isolate());
            collect_garbage(runtime, true);
            self.heap = heap_bytes(runtime);
        }
    }
}
