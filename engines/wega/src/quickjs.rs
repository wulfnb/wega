use crate::script::ScriptError;
use blitz_dom::{BaseDocument, NodeId};
use rquickjs::{Coerced, Context, Ctx, FromJs, Function, Runtime};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

/// Rust owns the only DOM. The VM sees checked generational IDs, never pointers.
pub(crate) struct QuickJsHost {
    context: Context,
    runtime: Runtime,
    deadline: Arc<Mutex<Instant>>,
    budget: std::time::Duration,
    interrupted: Arc<AtomicBool>,
    errors: Rc<RefCell<Vec<String>>>,
}
impl QuickJsHost {
    pub fn new(
        doc: Rc<RefCell<BaseDocument>>,
        limits: &crate::Limits,
        navigate: Rc<RefCell<Vec<url::Url>>>,
        initial_bytes: usize,
        network: crate::network::Network,
    ) -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        // Trusted bindings have a separate baseline, as in the V8 backend.
        runtime.set_memory_limit(limits.js_heap_bytes.saturating_add(4 * 1024 * 1024));
        runtime.set_gc_threshold(limits.js_heap_bytes / 4);
        runtime.set_max_stack_size(1024 * 1024);
        let deadline = Arc::new(Mutex::new(Instant::now() + limits.script_time));
        let check = deadline.clone();
        let interrupted = Arc::new(AtomicBool::new(false));
        let interrupt = interrupted.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            let expired = Instant::now()
                >= *check
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            if expired {
                interrupt.store(true, Ordering::Relaxed);
            }
            expired
        })));
        runtime.set_host_promise_rejection_tracker(Some(Box::new(
            |ctx, promise, reason, handled| {
                if let Ok(callback) = ctx.globals().get::<_, Function>("__wega_reject") {
                    let _ = callback.call::<_, ()>((promise, reason, handled));
                }
            },
        )));
        let context = Context::full(&runtime).map_err(|e| e.to_string())?;
        let dom = Rc::new(crate::dom::Dom::new(
            doc,
            limits,
            navigate,
            initial_bytes,
            network,
        ));
        let errors = dom.errors.clone();
        context.with(|ctx| {
            let host = Function::new(ctx.clone(), move |message: String| -> String {
                let result = serde_json::from_str::<Vec<Value>>(&message)
                    .map_err(|e| e.to_string())
                    .and_then(|args| dom.call(&args));
                match result {
                    Ok(value) => json!({"value":value}),
                    Err(error) => json!({"error":error}),
                }
                .to_string()
            })
            .map_err(|e| e.to_string())?;
            ctx.globals()
                .set("__wega_host", host)
                .map_err(|e| e.to_string())
        })?;
        let host = Self {
            context,
            runtime,
            deadline,
            budget: limits.script_time,
            interrupted,
            errors,
        };
        host.run("globalThis.__wega_call=(...args)=>{const r=JSON.parse(__wega_host(JSON.stringify(args)));if(r.error)throw Error(r.error);return r.value}","wega:bridge").map_err(|e|e.to_string())?;
        host.run(include_str!("dom/bindings.js"), "wega:dom")
            .map_err(|e| e.to_string())?;
        host.run(include_str!("dom/observers.js"), "wega:frame-apis")
            .map_err(|e| e.to_string())?;
        host.runtime.run_gc();
        let baseline = host.heap_bytes();
        host.runtime
            .set_memory_limit(baseline.saturating_add(limits.js_heap_bytes));
        Ok(host)
    }
    fn arm(&self) {
        *self
            .deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now() + self.budget;
    }
    fn error(&self, ctx: &Ctx<'_>, error: rquickjs::Error, source: &str) -> ScriptError {
        let interrupted = self.interrupted.load(Ordering::Relaxed);
        let allocation = matches!(error, rquickjs::Error::Allocation);
        let mut message = error.to_string();
        let mut fatal = interrupted || allocation;
        if error.is_exception() {
            let value = ctx.catch();
            if let Some(object) = value.as_object() {
                let name = object.get::<_, String>("name").unwrap_or_default();
                let detail = object.get::<_, String>("message").unwrap_or_default();
                // QuickJS reports native heap/stack exhaustion as InternalError.
                fatal |= name == "InternalError"
                    && (detail.contains("out of memory") || detail.contains("stack overflow"));
                fatal |=
                    name == "RangeError" && detail.contains("Maximum call stack size exceeded");
                message = format!("{name}: {detail}");
                if let Ok(stack) = object.get::<_, String>("stack") {
                    message.push_str(&format!(
                        "\n{}",
                        stack.lines().take(6).collect::<Vec<_>>().join("\n")
                    ));
                }
            } else {
                message = Coerced::<String>::from_js(ctx, value)
                    .map(|s| s.0)
                    .unwrap_or_else(|_| "Unprintable thrown value".into());
            }
        }
        if interrupted {
            message = format!("Script deadline exceeded: {message}");
        }
        ScriptError {
            fatal,
            message: format!("{source}: {message}"),
        }
    }
    pub fn take_errors(&self) -> Vec<String> {
        std::mem::take(&mut *self.errors.borrow_mut())
    }
    pub fn run(&self, source: &str, name: &str) -> Result<String, ScriptError> {
        self.arm();
        self.interrupted.store(false, Ordering::Relaxed);
        self.context.with(|ctx| {
            let mut options = rquickjs::context::EvalOptions::default();
            options.filename = Some(name.to_owned());
            let result = ctx
                .eval_with_options::<rquickjs::Value, _>(source, options)
                .map(|v| format!("{v:?}"));
            match result {
                Ok(value) => Ok(value),
                Err(error) => {
                    let mut error = self.error(&ctx, error, name);
                    if !error.fatal {
                        // The reporter is bounded and may not replace the original error.
                        let report = format!("__wega_report(new Error({}))", json!(error.message));
                        let _ = ctx.eval::<(), _>(report);
                        if let Ok(missing) = ctx.eval::<String, _>("__wega_missing()")
                            && !missing.is_empty()
                            && error.message.contains("TypeError")
                        {
                            error.message.push_str(&format!("; missing: {missing}"));
                        }
                    }
                    Err(error)
                }
            }
        })
    }
    pub fn dispatch(&self, event: &str, id: NodeId) -> Result<bool, ScriptError> {
        self.arm();
        self.context.with(|ctx| {
            ctx.eval::<bool, _>(format!(
                "__wega_dispatch({}, {})",
                json!(event),
                json!(id.as_u64().to_string())
            ))
            .map_err(|e| self.error(&ctx, e, "event dispatch"))
        })
    }
    pub fn evaluate_json(&self, source: &str) -> Result<String, ScriptError> {
        self.arm();
        self.context.with(|ctx| {
            ctx.eval::<String, _>(format!(
                "JSON.stringify((0,eval)({})) ?? 'null'",
                json!(source)
            ))
            .map_err(|e| self.error(&ctx, e, "evaluate_json"))
        })
    }
    pub fn tick(&self) -> Result<(), ScriptError> {
        self.run("__wega_timers()", "timer dispatch")?;
        self.arm();
        for _ in 0..4096 {
            if Instant::now()
                >= *self
                    .deadline
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
            {
                break;
            }
            if !self.runtime.execute_pending_job().map_err(|e| {
                e.0.with(|ctx| self.error(&ctx, rquickjs::Error::Exception, "promise job"))
            })? {
                break;
            }
        }
        self.run("__wega_rejections()", "unhandled promise rejection")?;
        if self.runtime.is_job_pending() {
            return Err(ScriptError {
                fatal: true,
                message: "Microtask budget exceeded".into(),
            });
        }
        Ok(())
    }
    pub fn next_timer(&self) -> Option<std::time::Duration> {
        if self.runtime.is_job_pending() {
            return Some(std::time::Duration::ZERO);
        }
        self.arm();
        self.context
            .with(|ctx| ctx.eval::<f64, _>("__wega_next_timer()").ok())
            .filter(|n| n.is_finite())
            .map(|n| std::time::Duration::from_secs_f64((n.max(0.0) / 1000.0).min(60.0)))
    }
    pub fn heap_bytes(&self) -> usize {
        self.runtime.memory_usage().memory_used_size.max(0) as usize
    }
}

impl crate::script::ScriptHost for QuickJsHost {
    fn run(&mut self, source: &str, name: &str) -> Result<String, ScriptError> {
        QuickJsHost::run(self, source, name)
    }
    fn dispatch(&mut self, event: &str, id: NodeId) -> Result<bool, ScriptError> {
        QuickJsHost::dispatch(self, event, id)
    }
    fn evaluate_json(&mut self, source: &str) -> Result<String, ScriptError> {
        QuickJsHost::evaluate_json(self, source)
    }
    fn tick(&mut self) -> Result<(), ScriptError> {
        QuickJsHost::tick(self)
    }
    fn next_timer(&self) -> Option<std::time::Duration> {
        QuickJsHost::next_timer(self)
    }
    fn heap_bytes(&self) -> usize {
        QuickJsHost::heap_bytes(self)
    }
    fn take_errors(&mut self) -> Vec<String> {
        QuickJsHost::take_errors(self)
    }
    fn freeze(&mut self, frozen: bool) {
        if frozen {
            self.runtime.run_gc();
        }
    }
}
