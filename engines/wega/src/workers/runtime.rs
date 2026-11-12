//! One worker realm: its own thread, V8 isolate and event loop, restored from
//! the same snapshot and Web API extensions as a document realm. Dedicated,
//! shared and service workers differ only in their global scope and owner.

use super::transfer::{PortId, SharedPortHub};
use crate::network::{Destination, FetchContext, Network};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use url::Url;
use wega_adapter_v8::{LoopStatus, WorkerRuntime, WorkerTerminator};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dedicated,
    Shared,
    Service,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Dedicated => "dedicated",
            Self::Shared => "shared",
            Self::Service => "service",
        }
    }
}

/// The latest committed bitmap of an OffscreenCanvas whose placeholder canvas
/// lives in a document.
pub struct CanvasFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}
pub type CanvasFrames = Arc<Mutex<HashMap<u64, CanvasFrame>>>;

/// Receives everything a worker reports to its owner: `(kind, JSON payload)`.
pub(crate) type Sink = Arc<dyn Fn(&str, &str) + Send + Sync>;

pub(crate) struct Spec {
    pub kind: Kind,
    pub name: String,
    pub script_url: Url,
    /// Source supplied by the owner (blob/inline scripts, stored service workers).
    pub source: Option<String>,
    pub module: bool,
    /// The document (or registration scope) that created the worker.
    pub creator: Url,
    pub network: Network,
    pub csp: Arc<RwLock<crate::csp::Csp>>,
    pub limits: crate::Limits,
    pub port_hub: SharedPortHub,
    pub service: Arc<super::ServiceWorkerHub>,
    /// A dedicated worker's implicit port.
    pub port: Option<PortId>,
    pub frames: CanvasFrames,
    /// Service worker registration scope.
    pub scope: String,
    pub sink: Sink,
}

pub(crate) enum Command {
    Dispatch(String, String),
    Stop,
}

pub(crate) struct Handle {
    commands: mpsc::Sender<Command>,
    isolate: Arc<Mutex<Option<WorkerTerminator>>>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Handle {
    pub fn spawn(spec: Spec) -> Result<Self, String> {
        let mut spec = spec;
        if std::env::var_os("WEGA_WORKER_DEBUG").is_some() {
            // Developer diagnostics: mirror everything the worker reports.
            let (inner, kind) = (spec.sink.clone(), spec.kind.name());
            spec.sink = Arc::new(move |name, payload| {
                if name != "swFetch" {
                    eprintln!("[wega {kind} worker] {name}: {payload}");
                }
                inner(name, payload);
            });
        }
        let (commands, receiver) = mpsc::channel();
        let isolate = Arc::new(Mutex::new(None));
        let stopped = Arc::new(AtomicBool::new(false));
        let (slot, flag) = (isolate.clone(), stopped.clone());
        let label = format!("wega-{}-worker", spec.kind.name());
        let thread = std::thread::Builder::new()
            .name(label)
            .spawn(move || {
                let sink = spec.sink.clone();
                if let Err(message) = run(spec, receiver, slot, flag.clone()) {
                    sink(
                        "error",
                        &json!({ "message": message, "fatal": true }).to_string(),
                    );
                }
                flag.store(true, Ordering::Release);
                sink("closed", "null");
            })
            .map_err(|error| format!("Failed to start worker thread: {error}"))?;
        Ok(Self {
            commands,
            isolate,
            stopped,
            thread: Some(thread),
        })
    }
    /// Queue an internal event for the worker's global scope.
    pub fn dispatch(&self, name: &str, payload: String) -> Result<(), String> {
        if self.stopped.load(Ordering::Acquire) {
            return Err("InvalidStateError: worker has stopped".into());
        }
        self.commands
            .send(Command::Dispatch(name.into(), payload))
            .map_err(|_| "InvalidStateError: worker has stopped".to_string())
    }
    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
    /// Stop the worker, including one stuck in script.
    pub fn stop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.commands.send(Command::Stop);
        if let Some(isolate) = self
            .isolate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            isolate.terminate();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.stop();
    }
}

const BOOTSTRAP: &str = r#"(() => {
  let dispatch;
  for (const name of ["web", "bridge", "canvas", "install", "cache", "worker"]) {
    const value = globalThis["__wega_bootstrap_" + name]();
    if (name === "worker") dispatch = value;
  }
  for (const key of Object.getOwnPropertyNames(globalThis))
    if (key.startsWith("__wega_bootstrap_")) delete globalThis[key];
  for (const key of ["__wega_call", "__wega_web", "__wega_install_web", "__wega_canvas", "__wega_guard_binding"])
    delete globalThis[key];
  return dispatch;
})()"#;

fn secret() -> String {
    use std::hash::{BuildHasher, Hasher};
    let part = || {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(0x5747_4547);
        hasher.finish()
    };
    format!("{:016x}{:016x}", part(), part())
}

fn run(
    spec: Spec,
    commands: mpsc::Receiver<Command>,
    isolate: Arc<Mutex<Option<WorkerTerminator>>>,
    stopped: Arc<AtomicBool>,
) -> Result<(), String> {
    let secret = secret();
    let host = Rc::new(WorkerHost {
        kind: spec.kind,
        name: spec.name,
        script_url: spec.script_url.clone(),
        creator: spec.creator,
        network: spec.network.clone(),
        csp: spec.csp,
        limits: spec.limits.clone(),
        port_hub: spec.port_hub,
        service: spec.service,
        port: spec.port,
        frames: spec.frames,
        scope: spec.scope,
        sink: spec.sink.clone(),
        stopped: stopped.clone(),
        closed: Cell::new(false),
        sealed: Cell::new(false),
        secret: secret.clone(),
        canvas: RefCell::new(crate::canvas::CanvasEngine::new(spec.limits.max_pixels * 4)),
        webgl: RefCell::new(crate::webgl::WebGLEngine::new(spec.limits.max_pixels * 8)),
        start: Instant::now(),
    });
    let native: Rc<dyn wega_adapter_v8::NativeHost> = host.clone();
    let mut runtime = WorkerRuntime::new(wega_adapter_v8::RuntimeSpec {
        kind: wega_adapter_v8::RuntimeKind::Worker,
        url: spec.script_url.clone(),
        network: Arc::new(spec.network.clone()),
        policy: spec.network.policy.clone(),
        host: native,
        snapshot: crate::script::STARTUP_SNAPSHOT,
        js_heap_bytes: spec.limits.js_heap_bytes,
        module_limit_bytes: spec.limits.resource_bytes,
    })
    .map_err(|error| format!("Failed to create worker isolate: {error}"))?;
    *isolate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(runtime.terminator());
    runtime
        .execute("ext:wega_dom/worker-bootstrap.js", BOOTSTRAP.to_owned())
        .map_err(|error| format!("Worker bootstrap failed: {error}"))?;
    host.sealed.set(true);
    runtime.configure_secure_context(&spec.script_url)?;

    let dispatch = |runtime: &mut WorkerRuntime, name: &str, payload: &str| {
        let source = format!(
            "__wega_worker_dispatch({},{},{})",
            json!(secret),
            json!(name),
            json!(payload)
        );
        if let Err(error) = runtime.execute("wega:worker-event", source) {
            host.report(&error);
        }
    };

    // Fetch and evaluate the worker script.
    let source = match spec.source {
        Some(source) => Ok(source),
        None => fetch_script(
            &spec.network,
            &spec.script_url,
            &host.creator,
            spec.limits.resource_bytes,
            &stopped,
            spec.kind == Kind::Service,
        )
        .map(|script| script.source),
    };
    let source = match source {
        Ok(source) => source,
        Err(message) => {
            (host.sink)(
                "error",
                &json!({ "message": message, "fatal": true, "load": true }).to_string(),
            );
            return Ok(());
        }
    };
    if spec.module {
        if let Err(error) = runtime.load_main_module(&spec.script_url, source) {
            (host.sink)(
                "error",
                &json!({ "message": error, "fatal": true, "load": true }).to_string(),
            );
            return Ok(());
        }
    } else if let Err(error) = runtime.execute(spec.script_url.as_str(), source) {
        if stopped.load(Ordering::Acquire) {
            return Ok(());
        }
        (host.sink)(
            "error",
            &json!({
                "message": error,
                "filename": spec.script_url.as_str(),
                "script": true,
            })
            .to_string(),
        );
        if spec.kind == Kind::Service {
            return Ok(());
        }
    }
    dispatch(&mut runtime, "loaded", "null");

    let tick = Duration::from_millis(4);
    'life: while !stopped.load(Ordering::Acquire) && !host.closed.get() {
        loop {
            match commands.try_recv() {
                Ok(Command::Dispatch(name, payload)) => dispatch(&mut runtime, &name, &payload),
                Ok(Command::Stop) | Err(mpsc::TryRecvError::Disconnected) => break 'life,
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        let turn = runtime.poll(tick);
        if runtime.exceeded_heap_limit() {
            return Err("Worker exceeded its JavaScript heap limit".into());
        }
        if let Some(Err(error)) = turn.module {
            host.report(&error);
        }
        match turn.status {
            LoopStatus::Failed(error) => {
                if stopped.load(Ordering::Acquire) {
                    break;
                }
                host.report(&error);
            }
            // Nothing is pending: sleep until the owner has work.
            LoopStatus::Idle => match commands.recv_timeout(tick) {
                Ok(Command::Dispatch(name, payload)) => dispatch(&mut runtime, &name, &payload),
                Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            },
            LoopStatus::TimedOut | LoopStatus::EvaluatingModule => {}
        }
    }
    Ok(())
}

pub(crate) struct Script {
    pub url: Url,
    pub source: String,
    pub headers: Vec<(String, String)>,
}

fn javascript_mime(value: &str) -> bool {
    let essence = value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        essence.as_str(),
        "text/javascript"
            | "application/javascript"
            | "application/x-javascript"
            | "text/ecmascript"
            | "application/ecmascript"
            | "text/jscript"
            | "text/livescript"
            | "text/javascript1.5"
    )
}

/// Blocking script load through the document's network stack. Worker scripts
/// must be JavaScript resources; `data:` scripts skip the MIME check.
pub(crate) fn fetch_script(
    network: &Network,
    url: &Url,
    initiator: &Url,
    limit: usize,
    stopped: &AtomicBool,
    bypass_service_worker: bool,
) -> Result<Script, String> {
    if url.scheme() == "blob" {
        return Err("NetworkError: blob worker scripts must be resolved by their document".into());
    }
    let (sender, receiver) = mpsc::channel();
    let (net, target) = (network.clone(), url.clone());
    let mut context = FetchContext::subresource(initiator, Destination::Script);
    context.bypass_service_worker = bypass_service_worker;
    network.spawn_detached(move |client| {
        let _ = sender.send(net.fetch(client, &target, &context, limit, None));
    })?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let response = loop {
        match receiver.recv_timeout(Duration::from_millis(25)) {
            Ok(result) => break result.map_err(|error| format!("NetworkError: {error}"))?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if stopped.load(Ordering::Acquire) || Instant::now() > deadline {
                    return Err("NetworkError: worker script load was abandoned".into());
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("NetworkError: network dispatcher stopped".into());
            }
        }
    };
    if !(200..300).contains(&response.status) {
        return Err(format!(
            "NetworkError: worker script returned HTTP status {}",
            response.status
        ));
    }
    if matches!(response.url.scheme(), "http" | "https")
        && !javascript_mime(response.header("content-type").unwrap_or(""))
    {
        return Err(format!(
            "SecurityError: worker script has an unsupported MIME type ({})",
            response.header("content-type").unwrap_or("none")
        ));
    }
    let headers = response
        .headers
        .iter()
        .filter_map(|(name, value)| {
            Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
        })
        .collect();
    Ok(Script {
        url: response.url,
        source: String::from_utf8_lossy(&response.bytes).into_owned(),
        headers,
    })
}

struct WorkerHost {
    kind: Kind,
    name: String,
    script_url: Url,
    creator: Url,
    network: Network,
    csp: Arc<RwLock<crate::csp::Csp>>,
    limits: crate::Limits,
    port_hub: SharedPortHub,
    service: Arc<super::ServiceWorkerHub>,
    port: Option<PortId>,
    frames: CanvasFrames,
    scope: String,
    sink: Sink,
    stopped: Arc<AtomicBool>,
    closed: Cell<bool>,
    /// Bootstrap-only operations are refused once worker script can run.
    sealed: Cell<bool>,
    secret: String,
    canvas: RefCell<crate::canvas::CanvasEngine>,
    webgl: RefCell<crate::webgl::WebGLEngine>,
    start: Instant,
}
impl WorkerHost {
    /// The URL used for origin, referrer and policy decisions. `data:` workers
    /// act for the document that created them.
    fn base(&self) -> &Url {
        if matches!(self.script_url.scheme(), "http" | "https" | "file") {
            &self.script_url
        } else {
            &self.creator
        }
    }
    fn report(&self, message: &str) {
        (self.sink)("error", &json!({ "message": message }).to_string());
    }
    fn hub(&self) -> std::sync::MutexGuard<'_, super::transfer::MessagePortHub> {
        self.port_hub
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn import(&self, address: &str) -> Result<Value, String> {
        let url = self
            .base()
            .join(address)
            .map_err(|_| format!("SyntaxError: invalid script URL {address}"))?;
        if !self
            .csp
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .allows_url("script-src", &url, &self.creator, None)
        {
            return Err(format!(
                "NetworkError: importScripts blocked by Content Security Policy: {url}"
            ));
        }
        let script = fetch_script(
            &self.network,
            &url,
            self.base(),
            self.limits.resource_bytes,
            &self.stopped,
            self.kind == Kind::Service,
        )?;
        Ok(json!({ "url": script.url.as_str(), "source": script.source }))
    }
}
impl wega_adapter_v8::NativeHost for WorkerHost {
    fn is_generated_hook(&self, name: &str) -> bool {
        crate::dom_host::is_generated_hook(name)
    }
    fn fetch(
        &self,
        mut request: wega_network::Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<wega_network::Response, String>>>>
    {
        let blocked = self.connect_check(&request.url);
        if !blocked.is_empty() {
            return Box::pin(std::future::ready(Err(blocked)));
        }
        request.origin = self.document_origin();
        request.initiator = Some(self.base().clone());
        request.top_level = false;
        request.destination = wega_network::ResourceDestination::Fetch;
        request.redirect = "manual".into();
        request.limit = self.limits.resource_bytes.min(wega_network::MAX_BODY);
        // A service worker's own requests go to the network; other workers are
        // clients of the service worker that controls their document.
        let client = (self.kind != Kind::Service).then(|| self.creator.clone());
        self.network.request(request, client)
    }
    fn call(&self, args: &[Value]) -> Result<Value, String> {
        let text = |index: usize| args.get(index).and_then(Value::as_str).unwrap_or("");
        if let Some(result) = crate::canvas::host::host_call(&mut self.canvas.borrow_mut(), args) {
            return result;
        }
        if let Some(result) = crate::webgl::host::host_call(&mut self.webgl.borrow_mut(), args) {
            return result;
        }
        let port = |index: usize| {
            text(index)
                .parse::<u64>()
                .map(PortId)
                .map_err(|_| "Invalid port ID".to_string())
        };
        match text(0) {
            "url" => Ok(json!(self.script_url.as_str())),
            "limits" => Ok(json!({ "resourceBytes": self.limits.resource_bytes })),
            "now" => Ok(json!(self.start.elapsed().as_secs_f64() * 1000.0)),
            "log" => {
                (self.sink)(
                    "log",
                    &json!({ "message": text(1), "level": text(2) }).to_string(),
                );
                Ok(Value::Null)
            }
            "error" => {
                self.report(text(1));
                Ok(Value::Null)
            }
            "workerInfo" => {
                if self.sealed.get() {
                    return Err("InvalidAccessError: worker is already initialized".into());
                }
                Ok(json!({
                    "kind": self.kind.name(),
                    "name": self.name,
                    "url": self.script_url.as_str(),
                    "base": self.base().as_str(),
                    "origin": self.document_origin(),
                    "scope": self.scope,
                    "secret": self.secret,
                    "port": self.port.map(|port| port.0),
                    "online": !self
                        .network
                        .policy
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .offline,
                }))
            }
            "workerEmit" => {
                (self.sink)(text(1), text(2));
                Ok(Value::Null)
            }
            "workerClose" => {
                self.closed.set(true);
                Ok(Value::Null)
            }
            "importScript" => self.import(text(1)),
            "channelCreate" => {
                let (first, second) = self.hub().create_channel();
                Ok(json!({ "port1": first.0, "port2": second.0 }))
            }
            "portPost" => {
                let ports: Vec<u64> = serde_json::from_str(text(3)).unwrap_or_default();
                self.hub().post_message(
                    port(1)?,
                    text(2).to_string(),
                    ports.into_iter().map(PortId).collect(),
                )?;
                Ok(Value::Null)
            }
            "portClose" => {
                self.hub().close(port(1)?);
                Ok(Value::Null)
            }
            "portPoll" => Ok(json!(self.hub().poll_messages(port(1)?))),
            "cacheAllowed" => Ok(json!(wega_network::cache_api::cache_allowed(self.base()))),
            "cacheCall" => self.network.cache_call(self.base(), text(1)),
            "swClients" if self.kind == Kind::Service => {
                Ok(self.service.clients(&self.document_origin()))
            }
            "swControls" if self.kind == Kind::Service => {
                Ok(json!(Url::parse(text(1)).is_ok_and(|url| {
                    self.service.controls(&self.network, &url)
                })))
            }
            "frameCommit" => {
                let local = text(1).parse::<u64>().map_err(|_| "Invalid canvas ID")?;
                let placeholder = text(2).parse::<u64>().map_err(|_| "Invalid canvas ID")?;
                let mut engine = self.canvas.borrow_mut();
                let context = engine
                    .get_mut(crate::canvas::CanvasId(local))
                    .ok_or("Canvas not found")?;
                let (width, height) = (context.width, context.height);
                let pixels = context.get_image_data(0, 0, width as i32, height as i32)?;
                self.frames
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(
                        placeholder,
                        CanvasFrame {
                            width,
                            height,
                            pixels,
                        },
                    );
                (self.network.wake)();
                Ok(Value::Null)
            }
            other => Err(format!(
                "NotSupportedError: {other} is not available in workers"
            )),
        }
    }
    fn cookie_header(&self, address: &str, include: bool) -> String {
        let Ok(url) = Url::parse(address) else {
            return String::new();
        };
        if !include {
            return String::new();
        }
        let context = self.network.cookies.context(&url, Some(self.base()), false);
        self.network.cookies.get(&url, false, context)
    }
    fn cookie_response(&self, address: &str, include: bool, value: &str) {
        let Ok(url) = Url::parse(address) else {
            return;
        };
        if include {
            let context = self.network.cookies.context(&url, Some(self.base()), false);
            self.network.cookies.set(&url, value, false, context);
        }
    }
    fn document_origin(&self) -> String {
        self.base().origin().ascii_serialization()
    }
    fn ws_headers(&self, address: &str) -> Vec<(String, String)> {
        let mut headers = vec![("origin".to_owned(), self.document_origin())];
        if let Ok(url) = Url::parse(address) {
            let context = self.network.cookies.context(&url, Some(self.base()), false);
            let cookie = self.network.cookies.get(&url, false, context);
            if !cookie.is_empty() {
                headers.push(("cookie".to_owned(), cookie));
            }
        }
        headers
    }
    fn referrer(&self, address: &str) -> String {
        Url::parse(address)
            .ok()
            .and_then(|url| crate::network::referrer(self.base(), &url, None))
            .unwrap_or_default()
    }
    fn connect_check(&self, address: &str) -> String {
        let Ok(url) = Url::parse(address) else {
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
            // A controlling service worker may still answer from its caches.
            if self.kind == Kind::Service || !self.service.controls(&self.network, &self.creator) {
                return "Network request blocked by browsing policy".into();
            }
        }
        if crate::network::mixed_content(Some(self.base()), &url, false) {
            return format!("Mixed content blocked: {url}");
        }
        if !self
            .csp
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .allows_url("connect-src", &url, &self.creator, None)
        {
            return format!("Refused to connect to {url}: Content Security Policy connect-src");
        }
        String::new()
    }
}
