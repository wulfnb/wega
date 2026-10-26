//! V8 isolates are confined to the owning DOM thread; only termination crosses threads.
use crate::{
    Limits,
    dom::Dom,
    script::{ScriptError, ScriptHost},
};
use blitz_dom::{BaseDocument, NodeId};
use deno_core::{JsRuntime, RuntimeOptions};
use serde_json::json;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

impl crate::native_ops::NativeHost for Dom {
    fn fetch(
        &self,
        mut request: wega_network::Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<wega_network::Response, String>>>>
    {
        request.origin = self.document.borrow().url().origin().ascii_serialization();
        request.limit = self.limits.resource_bytes.min(wega_network::MAX_BODY);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let queued = self.network.spawn(move |client| {
            let _ = sender.send(client.response(&request));
        });
        Box::pin(async move {
            queued?;
            let mut response = receiver.await.map_err(|_| "Network dispatcher stopped")??;
            response
                .headers
                .retain(|(name, _)| !name.eq_ignore_ascii_case("set-cookie"));
            Ok(response)
        })
    }

    fn call(&self, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
        Dom::call(self, args)
    }
    fn cookie_header(&self, address: &str, mode: &str) -> String {
        let Ok(url) = url::Url::parse(address) else {
            return String::new();
        };
        if mode == "omit"
            || (mode != "include" && url.origin() != self.document.borrow().url().origin())
        {
            return String::new();
        }
        self.network.cookies.get(&url, false)
    }
    fn cookie_response(&self, address: &str, mode: &str, value: &str) {
        let Ok(url) = url::Url::parse(address) else {
            return;
        };
        if mode != "omit"
            && (mode == "include" || url.origin() == self.document.borrow().url().origin())
        {
            self.network.cookies.set(&url, value, false);
        }
    }
}
use crate::native_ops::wega_dom;
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
fn collect_garbage(runtime: &mut JsRuntime, critical: bool) {
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
pub(crate) struct V8Host {
    runtime: Option<JsRuntime>,
    loading_module: Option<ModuleLoad>,
    evaluating_modules: Vec<ModuleEvaluation>,
    queued: std::collections::VecDeque<(String, String, bool)>,
    terminate: deno_core::v8::IsolateHandle,
    executor: tokio::runtime::Runtime,
    dom: Rc<Dom>,
    budget: Duration,
    oom: Arc<AtomicBool>,
    pending: bool,
    frozen: bool,
    heap: usize,
    heap_limit: usize,
    timer: Option<Instant>,
    polled_at: Instant,
}
impl V8Host {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        limits: &Limits,
        navigate: Rc<RefCell<Vec<url::Url>>>,
        initial_bytes: usize,
        network: crate::network::Network,
    ) -> Result<Self, String> {
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let entered = executor.enter();
        let dom = Rc::new(Dom::new(document, limits, navigate, initial_bytes, network));
        // Do not enable lite mode per isolate: V8 flags are process-wide. Workers receive
        // their per-view option before initializing the platform.
        let document_url = dom.document.borrow().url().clone();
        let mut extensions = crate::web::extensions(&dom);
        extensions.push(wega_dom::init());
        for extension in &extensions {
            for source in extension
                .js_files
                .iter()
                .chain(extension.esm_files.iter())
                .chain(extension.lazy_loaded_js_files.iter())
                .chain(extension.lazy_loaded_esm_files.iter())
            {
                if !source.is_runtime_loadable() {
                    return Err(format!(
                        "Extension {} must be embedded in the worker binary",
                        source.specifier
                    ));
                }
            }
        }
        let mut runtime = JsRuntime::try_new(RuntimeOptions {
            startup_snapshot: Some(include_bytes!(concat!(
                env!("OUT_DIR"),
                "/wega-startup.bin"
            ))),
            module_loader: Some(Rc::new(crate::module_loader::Loader::new(
                dom.network.clone(),
                &document_url,
                1024 * 1024,
            ))),
            extension_transpiler: Some(Rc::new(crate::web::adapt_extension)),
            create_params: Some(
                deno_core::v8::CreateParams::default()
                    .heap_limits(0, limits.js_heap_bytes.saturating_add(32 * 1024 * 1024)),
            ),
            extensions,
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;
        // Deno's trusted bootstrap needs its own heap before page code starts.
        let host: Rc<dyn crate::native_ops::NativeHost> = dom.clone();
        runtime.op_state().borrow_mut().put(host);
        runtime
            .op_state()
            .borrow_mut()
            .put(crate::web::permissions(dom.network.policy.clone()));
        runtime.op_state().borrow_mut().put(String::new());
        runtime
            .execute_script(
                "ext:wega_dom/bootstrap.js",
                include_str!("dom/bootstrap.js"),
            )
            .map_err(|error| error.to_string())?;
        runtime.op_state().borrow_mut().put(String::new());
        let bootstrap_heap = heap_bytes(&mut runtime);
        let oom = Arc::new(AtomicBool::new(false));
        let flag = oom.clone();
        let terminate = runtime.v8_isolate().thread_safe_handle();
        runtime.add_near_heap_limit_callback(move |current, _initial| {
            flag.store(true, Ordering::Relaxed);
            terminate.terminate_execution();
            current.saturating_mul(2)
        });
        let terminate = runtime.v8_isolate().thread_safe_handle();
        drop(entered);
        Ok(Self {
            runtime: Some(runtime),
            loading_module: None,
            evaluating_modules: vec![],
            queued: Default::default(),
            terminate,
            executor,
            dom,
            budget: limits.script_time,
            oom,
            pending: false,
            frozen: false,
            heap: 0,
            heap_limit: bootstrap_heap.saturating_add(limits.js_heap_bytes),
            timer: None,
            polled_at: Instant::now(),
        })
    }
    fn execute(&mut self, source: &str, name: &str) -> Result<String, ScriptError> {
        if self.runtime.is_none() {
            return Err(ScriptError {
                fatal: false,
                message: "Module graph is loading".into(),
            });
        }
        let entered = self.executor.enter();
        let handle = self
            .runtime
            .as_mut()
            .unwrap()
            .v8_isolate()
            .thread_safe_handle();
        let timed_out = Arc::new(AtomicBool::new(false));
        let flag = timed_out.clone();
        let budget = self.budget;
        let (done, wait) = mpsc::channel();
        let watchdog = std::thread::spawn(move || {
            if matches!(
                wait.recv_timeout(budget),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                flag.store(true, Ordering::Relaxed);
                handle.terminate_execution();
            }
        });
        let result = self
            .runtime
            .as_mut()
            .unwrap()
            .execute_script(name.to_owned(), source.to_owned());
        let _ = done.send(());
        let _ = watchdog.join();
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
        if timed_out.load(Ordering::Relaxed) {
            return Err(ScriptError {
                fatal: true,
                message: format!("{name}: Script deadline exceeded"),
            });
        }
        match result {
            Ok(value) => Ok(format!("{value:?}")),
            Err(error) => {
                let text = error.to_string();
                let fatal = timed_out.load(Ordering::Relaxed)
                    || self.oom.load(Ordering::Relaxed)
                    || text.contains("Maximum call stack size exceeded");
                let text = if timed_out.load(Ordering::Relaxed) {
                    format!("Script deadline exceeded: {text}")
                } else if self.oom.load(Ordering::Relaxed) {
                    format!("V8 heap limit exceeded: {text}")
                } else {
                    text
                };
                Err(ScriptError {
                    fatal,
                    message: format!("{name}: {text}"),
                })
            }
        }
    }
}
impl ScriptHost for V8Host {
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
            let _ = self.execute(
                &format!(
                    "__wega_report(new Error({}));__wega_missing()",
                    json!(message)
                ),
                "wega:error",
            );
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
    fn dispatch(&mut self, event: &str, id: NodeId) -> Result<bool, ScriptError> {
        let text = self.evaluate_json(&format!(
            "__wega_dispatch({}, {})",
            json!(event),
            json!(id.as_u64().to_string())
        ))?;
        self.pending = true;
        Ok(text == "true")
    }
    fn evaluate_json(&mut self, source: &str) -> Result<String, ScriptError> {
        self.execute(
            &format!("__wega_evaluate({})", json!(source)),
            "evaluate_json",
        )?;
        Ok(self
            .runtime
            .as_mut()
            .unwrap()
            .op_state()
            .borrow()
            .borrow::<String>()
            .clone())
    }
    fn tick(&mut self) -> Result<(), ScriptError> {
        if self.frozen {
            return Ok(());
        }
        let module_waker = Waker::from(Arc::new(WakeLoop(self.dom.network.wake.clone())));
        let mut module_cx = Context::from_waker(&module_waker);
        if let Some(load) = &mut self.loading_module {
            let handle = self.terminate.clone();
            let budget = self.budget;
            let (done, wait) = mpsc::channel();
            let expired = Arc::new(AtomicBool::new(false));
            let flag = expired.clone();
            let watchdog = std::thread::spawn(move || {
                if matches!(
                    wait.recv_timeout(budget),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    flag.store(true, Ordering::Relaxed);
                    handle.terminate_execution();
                }
            });
            let result = self.executor.block_on(async {
                tokio::task::yield_now().await;
                load.as_mut().poll(&mut module_cx)
            });
            let _ = done.send(());
            let _ = watchdog.join();
            self.polled_at = Instant::now();
            match result {
                Poll::Pending => {
                    self.pending = true;
                    return Ok(());
                }
                Poll::Ready((runtime, result)) => {
                    self.runtime = Some(runtime);
                    self.loading_module = None;
                    let id = result.map_err(|e| ScriptError {
                        fatal: expired.load(Ordering::Relaxed) || self.oom.load(Ordering::Relaxed),
                        message: e.to_string(),
                    })?;
                    let entered = self.executor.enter();
                    let handle = self.terminate.clone();
                    let budget = self.budget;
                    let (done, wait) = mpsc::channel();
                    let flag = expired.clone();
                    let watchdog = std::thread::spawn(move || {
                        if matches!(
                            wait.recv_timeout(budget),
                            Err(mpsc::RecvTimeoutError::Timeout)
                        ) {
                            flag.store(true, Ordering::Relaxed);
                            handle.terminate_execution();
                        }
                    });
                    let evaluation = self.runtime.as_mut().unwrap().mod_evaluate(id);
                    let _ = done.send(());
                    let _ = watchdog.join();
                    drop(entered);
                    self.evaluating_modules.push(Box::pin(evaluation));
                    if expired.load(Ordering::Relaxed) || self.oom.load(Ordering::Relaxed) {
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
        let waker = Waker::from(Arc::new(WakeLoop(self.dom.network.wake.clone())));
        let mut cx = Context::from_waker(&waker);
        let runtime = self.runtime.as_mut().unwrap();
        let handle = runtime.v8_isolate().thread_safe_handle();
        let timed_out = Arc::new(AtomicBool::new(false));
        let flag = timed_out.clone();
        let budget = self.budget;
        let (done, wait) = mpsc::channel();
        let watchdog = std::thread::spawn(move || {
            if matches!(
                wait.recv_timeout(budget),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                flag.store(true, Ordering::Relaxed);
                handle.terminate_execution();
            }
        });
        let result = self.executor.block_on(async {
            for _ in 0..4 {
                tokio::task::yield_now().await;
                let result = runtime.poll_event_loop(&mut cx, Default::default());
                if result.is_ready() {
                    return result;
                }
            }
            Poll::Pending
        });
        let _ = done.send(());
        let _ = watchdog.join();
        self.polled_at = Instant::now();
        self.heap = heap_bytes(self.runtime.as_mut().unwrap());
        self.runtime
            .as_mut()
            .unwrap()
            .v8_isolate()
            .clear_kept_objects();
        match result {
            Poll::Pending => {
                self.pending = true;
            }
            Poll::Ready(Ok(())) => {
                self.pending = false;
            }
            Poll::Ready(Err(e)) => {
                return Err(ScriptError {
                    fatal: self.oom.load(Ordering::Relaxed) || timed_out.load(Ordering::Relaxed),
                    message: e.to_string(),
                });
            }
        }
        if timed_out.load(Ordering::Relaxed) {
            return Err(ScriptError {
                fatal: true,
                message: "Microtask deadline exceeded".into(),
            });
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
        self.tick()?;
        if self.runtime.is_none() {
            return Ok(false);
        }
        Ok(self.evaluate_json("__wega_has_frame()")? == "true")
    }
    fn heap_bytes(&self) -> usize {
        self.heap
    }
    fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut *self.dom.errors.borrow_mut())
    }
    fn freeze(&mut self, frozen: bool) {
        self.frozen = frozen;
        if frozen && let Some(runtime) = &mut self.runtime {
            let _entered = self.executor.enter();
            collect_garbage(runtime, true);
            self.heap = heap_bytes(runtime);
        }
    }
}

#[cfg(test)]
mod packaging_tests {
    #[test]
    fn eager_and_lazy_sources_are_embedded_without_a_build_tree() {
        let eager = deno_core::include_js_files!(wega_dom dir "src/dom", "bindings.js");
        let lazy = deno_core::include_lazy_loaded_js_files!(wega_dom dir "src/dom", "web.js");
        for source in eager.iter().chain(lazy.iter()) {
            assert!(
                source.is_runtime_loadable(),
                "{} requires a build-machine path",
                source.specifier
            );
        }
    }
}
