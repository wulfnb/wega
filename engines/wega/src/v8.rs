//! V8 isolates are confined to the owning DOM thread; only termination crosses threads.
use crate::{
    Limits,
    dom::Dom,
    script::{ScriptError, ScriptHost},
};
use deno_core::{JsRuntime, RuntimeOptions};
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

impl crate::native_ops::NativeHost for Dom {
    fn fetch(
        &self,
        mut request: wega_network::Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<wega_network::Response, String>>>>
    {
        let blocked = self.connect_check(&request.url);
        if !blocked.is_empty() {
            return Box::pin(std::future::ready(Err(blocked)));
        }
        let document = self.document.borrow().url().clone();
        request.origin = document.origin().ascii_serialization();
        request.initiator = Some(document);
        request.top_level = false;
        request.redirect = "manual".into();
        request.limit = self.limits.resource_bytes.min(wega_network::MAX_BODY);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let queued = self.network.spawn(move |client| {
            let _ = sender.send(client.response(&request));
        });
        Box::pin(async move {
            queued?;
            receiver.await.map_err(|_| "Network dispatcher stopped")?
        })
    }

    fn call(&self, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
        Dom::call(self, args)
    }
    fn cookie_header(&self, address: &str, include: bool) -> String {
        let Ok(url) = url::Url::parse(address) else {
            return String::new();
        };
        if !include {
            return String::new();
        }
        let document = self.document.borrow().url().clone();
        let context = self.network.cookies.context(&url, Some(&document), false);
        self.network.cookies.get(&url, false, context)
    }
    fn cookie_response(&self, address: &str, include: bool, value: &str) {
        let Ok(url) = url::Url::parse(address) else {
            return;
        };
        if !include {
            return;
        }
        let document = self.document.borrow().url().clone();
        let context = self.network.cookies.context(&url, Some(&document), false);
        self.network.cookies.set(&url, value, false, context);
    }
    fn document_origin(&self) -> String {
        self.document.borrow().url().origin().ascii_serialization()
    }
    fn ws_headers(&self, address: &str) -> Vec<(String, String)> {
        let document = self.document.borrow().url().clone();
        let mut headers = vec![("origin".to_owned(), document.origin().ascii_serialization())];
        if let Ok(url) = url::Url::parse(address) {
            let context = self.network.cookies.context(&url, Some(&document), false);
            let cookie = self.network.cookies.get(&url, false, context);
            if !cookie.is_empty() {
                headers.push(("cookie".to_owned(), cookie));
            }
        }
        headers
    }
    fn referrer(&self, address: &str) -> String {
        let Ok(url) = url::Url::parse(address) else {
            return String::new();
        };
        let document = self.document.borrow().url().clone();
        crate::network::referrer(&document, &url, None).unwrap_or_default()
    }
    fn connect_check(&self, address: &str) -> String {
        let Ok(url) = url::Url::parse(address) else {
            return "Invalid URL".into();
        };
        let mut policy_url = url.clone();
        if url.scheme() == "ws" {
            let _ = policy_url.set_scheme("http");
        }
        if url.scheme() == "wss" {
            let _ = policy_url.set_scheme("https");
        }
        if matches!(url.scheme(), "http" | "https" | "ws" | "wss")
            && !self
                .network
                .policy
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .permits(&policy_url)
        {
            return "Network request blocked by browsing policy".into();
        }
        let document = self.document.borrow().url().clone();
        if crate::network::mixed_content(Some(&document), &url, false) {
            return format!("Mixed content blocked: {url}");
        }
        if !self
            .csp
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .allows_url("connect-src", &url, &document, None)
        {
            return format!("Refused to connect to {url}: Content Security Policy connect-src");
        }
        String::new()
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

pub(crate) struct V8Host {
    runtime: Option<JsRuntime>,
    loading_module: Option<ModuleLoad>,
    evaluating_modules: Vec<ModuleEvaluation>,
    queued: std::collections::VecDeque<(String, String, bool)>,
    watchdog: Watchdog,
    executor: Rc<tokio::runtime::Runtime>,
    dom: Rc<Dom>,
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
impl V8Host {
    pub fn new(
        context: crate::script::HostContext,
        limits: &Limits,
        initial_bytes: usize,
    ) -> Result<Self, String> {
        let executor = Rc::new(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?,
        );
        let entered = executor.enter();
        let dom = Rc::new(Dom::new(context, limits, initial_bytes));
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
                limits.resource_bytes,
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
        let watchdog = Watchdog::new(runtime.v8_isolate().thread_safe_handle())?;
        drop(entered);
        Ok(Self {
            runtime: Some(runtime),
            loading_module: None,
            evaluating_modules: vec![],
            queued: Default::default(),
            watchdog,
            executor,
            dom,
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
        self.dom.begin_task();
        let started = Instant::now();
        self.watchdog.arm(self.budget);
        let result = run(self);
        let fired = self.watchdog.disarm();
        let elapsed = started.elapsed();
        if !fired && elapsed > self.slow {
            let mut errors = self.dom.errors.borrow_mut();
            if errors.len() < 16 {
                errors.push(format!(
                    "{name}: slow script ({} ms); the page may be unresponsive",
                    elapsed.as_millis()
                ));
            }
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
    /// Run `source` and return its completion value as JSON, without `eval`
    /// (which a page's CSP may disable).
    fn evaluate_json(&mut self, source: &str) -> Result<String, ScriptError> {
        let value = self.execute_value(source, "evaluate_json")?;
        let runtime = self.runtime.as_mut().unwrap();
        deno_core::scope!(scope, runtime);
        let local = deno_core::v8::Local::new(scope, value);
        Ok(deno_core::v8::json::stringify(scope, local)
            .map(|text| text.to_rust_string_lossy(scope))
            .filter(|text| text != "undefined")
            .unwrap_or_else(|| "null".into()))
    }
    fn tick(&mut self) -> Result<(), ScriptError> {
        if self.frozen {
            return Ok(());
        }
        let module_waker = Waker::from(Arc::new(WakeLoop(self.dom.network.wake.clone())));
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
        let waker = Waker::from(Arc::new(WakeLoop(self.dom.network.wake.clone())));
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
    fn take_requests(&mut self) -> crate::dom::Requests {
        std::mem::take(&mut *self.dom.requests.borrow_mut())
    }
    fn allow_eval(&mut self, allowed: bool) {
        if let Some(runtime) = &mut self.runtime {
            deno_core::scope!(scope, runtime);
            scope
                .get_current_context()
                .set_allow_generation_from_strings(allowed);
        }
    }
    fn budget(&self) -> (usize, usize) {
        self.dom.budget()
    }
    fn sync_nodes(&mut self, count: usize) {
        self.dom.set_allocated(count);
    }
    fn history(&self) -> (usize, usize) {
        self.dom.history.get()
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
