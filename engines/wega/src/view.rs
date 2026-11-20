use crate::document::{DocumentSetup, WegaDocument};
use crate::frame::FrameState;
use crate::{
    Limits,
    csp::Csp,
    encoding::{self, DocumentKind},
    native_event::{NativeEvent, script_all},
    network::{Destination, FetchContext, Network, Response},
    script::{DynScriptHost, HostContext},
};
use encoding_rs::Encoding;
use html5ever::{LocalName, QualName, ns};
use keyboard_types::Modifiers;
use std::{
    cell::{Cell, RefCell},
    collections::{HashSet, VecDeque},
    rc::Rc,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use url::Url;
use wega_adapter_skia::{FrameRequest, FrameTarget};
use wega_dom::Mutator as _;
use wega_dom::NodeKey;
use wega_dom::input::{FocusState as _, element_chain};
use wega_dom::layout::{Point, ViewportConfig};
use wega_dom::{DocumentView, NodeView};
use wega_network::document::{
    navigation::{NavigationOptions, NavigationProvider},
    net::{AbortController, AbortSignal, Body, Bytes, NetHandler, NetProvider, Request},
};
use wega_protocol::identity::FrameId;

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
type FetchResult = Result<Response, String>;

/// Something the browser chrome must act on.
#[derive(Clone, Debug, PartialEq)]
pub enum PageEvent {
    /// The response is a file, not a page.
    Download(Url),
    /// A popup or `target=_blank` link, opened by a user gesture.
    OpenWindow(Url),
    /// History traversal outside this document's own entries.
    Traverse(i32),
    /// The CSS cursor over the hovered element changed.
    Cursor(String),
    /// A discovered or updated favicon URL.
    Favicon(Url),
    /// The link URL hovered by the pointer, or None if no link is hovered.
    Status(Option<String>),
    /// HTTP 401 basic/digest authentication request from the origin.
    Authentication {
        url: String,
        realm: Option<String>,
    },
    /// Accessibility tree update
    Accessibility(String),
    /// Native file chooser dialog request
    FileDialog {
        id: u64,
        multiple: bool,
        accept: Vec<String>,
    },
    ClipboardWrite(String),
    FindResult {
        active_match_ordinal: u32,
        number_of_matches: u32,
        query: String,
    },
    FrameAttached {
        parent: FrameId,
        frame: FrameId,
        url: String,
        name: Option<String>,
        sandbox: Option<String>,
    },
    FrameDetached {
        frame: FrameId,
    },
    FrameNavigated {
        frame: FrameId,
        url: String,
        origin: String,
    },
    FramePostMessage {
        source: FrameId,
        target: FrameId,
        origin: String,
        target_origin: Option<String>,
        message: String,
    },
    PermissionPrompt {
        id: u64,
        origin: String,
        feature: wega_protocol::permissions::PermissionFeature,
    },
    PermissionRevoked {
        origin: String,
        feature: wega_protocol::permissions::PermissionFeature,
    },
    NotificationShow {
        id: u64,
        title: String,
        options: wega_protocol::permissions::NotificationOptions,
    },
    NotificationClose {
        id: u64,
    },
    Fullscreen(bool),
    ProtocolRegister(wega_protocol::permissions::ProtocolHandlerRegistration),
    ExternalApp(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Move,
    Down,
    Up,
    Leave,
    Wheel,
}
/// One pointer input in viewport (client) CSS pixels.
#[derive(Clone, Copy, Debug)]
pub struct Pointer {
    pub kind: PointerKind,
    pub x: f32,
    pub y: f32,
    /// 0 main, 1 auxiliary, 2 secondary (DOM `button`).
    pub button: u8,
    /// Pressed buttons bitmask (DOM `buttons`).
    pub buttons: u8,
    /// Bit 1 shift, 2 control, 4 alt, 8 meta.
    pub modifiers: u8,
    pub delta_x: f64,
    pub delta_y: f64,
}
impl Pointer {
    pub fn new(kind: PointerKind, x: f32, y: f32) -> Self {
        Self {
            kind,
            x,
            y,
            button: 0,
            buttons: 0,
            modifiers: 0,
            delta_x: 0.0,
            delta_y: 0.0,
        }
    }
}

/// Blitz subresources (stylesheets, images, fonts) with CSP, mixed content,
/// cookies and a load counter for the `load` event.
struct Resources {
    network: Network,
    local: Option<std::path::PathBuf>,
    limit: usize,
    document: Url,
    referrer_policy: Option<String>,
    csp: Arc<RwLock<Csp>>,
    revision: Arc<AtomicU64>,
    in_flight: Arc<AtomicUsize>,
    completed: Arc<Mutex<Vec<(String, bool)>>>,
    /// Opaque-origin documents never send ambient credentials.
    omit_credentials: bool,
    resource_requests: AtomicUsize,
}
fn destination(url: &Url) -> Destination {
    let path = url.path().to_ascii_lowercase();
    let extension = path.rsplit_once('.').map(|(_, e)| e).unwrap_or_default();
    match extension {
        "css" => Destination::Style,
        "woff" | "woff2" | "ttf" | "otf" | "eot" => Destination::Font,
        _ if path.contains("/css/") || path.ends_with("/css") || path.contains("stylesheet") => {
            Destination::Style
        }
        _ => Destination::Image,
    }
}
impl NetProvider for Resources {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        // Resource handlers carry a trusted destination. URL extensions cannot
        // decide stylesheet/font authority (e.g. a CSS endpoint named /asset).
        let explicit = request
            .headers
            .get("sec-fetch-dest")
            .and_then(|h| h.to_str().ok());
        let resource_destination = match explicit {
            Some("style") => Destination::Style,
            Some("image") => Destination::Image,
            Some("font") => Destination::Font,
            _ => destination(&request.url),
        };
        let csp = self
            .csp
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let permitted = match explicit {
            Some("style") => csp.allows_url("style-src-elem", &request.url, &self.document, None),
            Some("image") => csp.allows_url("img-src", &request.url, &self.document, None),
            Some("font") => csp.allows_url("font-src", &request.url, &self.document, None),
            _ => {
                !matches!(request.url.scheme(), "http" | "https")
                    || csp.allows_subresource(&request.url, &self.document)
            }
        };
        drop(csp);
        let blocked = request.method != wega_network::document::net::http::Method::GET
            || !permitted
            || self.resource_requests.fetch_add(1, Ordering::Relaxed) >= 512;
        if blocked {
            eprintln!("Wega resource blocked: {}", request.url);
            self.completed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((request.url.to_string(), false));
            handler.bytes(request.url.to_string(), Bytes::new());
            return;
        }
        let local = self.local.clone();
        let network = self.network.clone();
        let wake = self.network.wake.clone();
        let revision = self.revision.clone();
        let in_flight = self.in_flight.clone();
        let completed = self.completed.clone();
        let limit = self.limit;
        let mut context = FetchContext::subresource(&self.document, resource_destination);
        context.omit_credentials = self.omit_credentials;
        context.resource_csp = Some(self.csp.clone());
        context.referrer_policy = self.referrer_policy.clone();
        in_flight.fetch_add(1, Ordering::Relaxed);
        let handler = Arc::new(Mutex::new(Some(handler)));
        let fallback = handler.clone();
        let failed_url = request.url.to_string();
        let failed = (
            self.network.wake.clone(),
            self.revision.clone(),
            self.in_flight.clone(),
            self.completed.clone(),
        );
        let task = move |client: &crate::network::Client| {
            let finish = |url: String, ok: bool| {
                completed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push((url, ok));
                in_flight.fetch_sub(1, Ordering::Relaxed);
                revision.fetch_add(1, Ordering::Relaxed);
                wake();
            };
            if request.signal.as_ref().is_some_and(AbortSignal::aborted) {
                finish(request.url.to_string(), false);
                return;
            }
            let response = network.fetch(client, &request.url, &context, limit, local.as_deref());
            if request.signal.as_ref().is_some_and(AbortSignal::aborted) {
                finish(request.url.to_string(), false);
                return;
            }
            let (url, bytes, ok) = match response {
                Ok(response)
                    if resource_destination == Destination::Style
                        && (response.status >= 400
                            || !response.header("content-type").is_some_and(|h| {
                                h.split(';').next().is_some_and(|mime| {
                                    mime.trim().eq_ignore_ascii_case("text/css")
                                })
                            })) =>
                {
                    eprintln!("Wega stylesheet rejected: status or content type");
                    (response.url, vec![], false)
                }
                Ok(response) => (response.url, response.bytes, true),
                Err(error) => {
                    eprintln!("Wega resource: {error}");
                    (request.url.clone(), vec![], false)
                }
            };
            if let Some(handler) = handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                handler.bytes(url.to_string(), Bytes::from(bytes));
            }
            finish(request.url.to_string(), ok);
        };
        if let Err(error) = self.network.spawn(task) {
            eprintln!("Wega resource: {error}");
            if let Some(handler) = fallback
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                handler.bytes(failed_url.clone(), Bytes::new());
            }
            let (wake, revision, in_flight, completed) = failed;
            completed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((failed_url, false));
            in_flight.fetch_sub(1, Ordering::Relaxed);
            revision.fetch_add(1, Ordering::Relaxed);
            wake();
        }
    }
}
struct Navigation(mpsc::Sender<NavigationOptions>, Arc<dyn Fn() + Send + Sync>);
impl NavigationProvider for Navigation {
    fn navigate_to(&self, options: NavigationOptions) {
        let _ = self.0.send(options);
        (self.1)();
    }
}

#[path = "document_host.rs"]
mod document_host;
#[path = "frames.rs"]
pub(crate) mod frames;
use crate::html_report::{HtmlReport, Parser};
#[cfg(test)]
use document_host::script_kind;
use wega_html::Step;

pub struct View {
    host: document_host::DocumentHost,
    scheduler: crate::event_loop::Scheduler,
    document: Rc<RefCell<WegaDocument>>,
    document_revision: u64,
    navigation_generation: u64,
    document_generation: Rc<Cell<u64>>,
    script: Option<Box<DynScriptHost>>,
    network: Network,
    hubs: crate::workers::Hubs,
    limits: Limits,
    abort: Option<AbortController>,
    pending: Option<mpsc::Receiver<FetchResult>>,
    referrer_policy: Option<String>,
    csp: Arc<RwLock<Csp>>,
    dialog: Rc<RefCell<Option<crate::dom::DocumentDialogHandler>>>,
    activation: Rc<Cell<Option<Instant>>>,
    navigate: Rc<RefCell<Vec<Url>>>,
    navigation_rx: mpsc::Receiver<NavigationOptions>,
    revision: Arc<AtomicU64>,
    last_revision: u64,
    /// Top-level views rasterise; nested frames only record scenes for their parent.
    raster: Option<wega_adapter_skia::Raster>,
    pub compatibility: Option<String>,
    script_failure: Option<String>,
    in_flight: Arc<AtomicUsize>,
    completed: Arc<Mutex<Vec<(String, bool)>>>,
    started: Instant,
    frame: FrameState,
    reported_frames: bool,
    frozen: bool,
    refresh: Option<(Instant, Url)>,
    composing: bool,
    showing_error: bool,
    /// Set while the error page itself is being parsed, so an error page that also exceeds the
    /// node budget stops instead of recursing into another error page.
    building_error: bool,
    cursor: String,
    pub status: Option<String>,
    pub rendering_backend: crate::native_engine::RenderingBackend,
    pub native_document: Option<Rc<RefCell<crate::native_engine::NativeDocument>>>,
    native_hover: Option<NodeKey>,
    native_down: Option<NodeKey>,
    last_down: Option<(Instant, f32, f32)>,
    click_count: u32,
    pub last_favicon: Option<Url>,
    pub user_script: String,
    pub transparent: bool,
    pub url: Url,
    pub loading: bool,
    pub notices: VecDeque<String>,
    pub events: VecDeque<PageEvent>,
    pub accessibility_enabled: bool,
    pub next_dialog_id: u64,
    pub pending_file_dialog: Option<(u64, NodeKey)>,
    pub selected_files: std::collections::HashMap<NodeKey, Vec<String>>,
    pub permissions: Rc<RefCell<crate::permissions::PermissionRegistry>>,
    pub fullscreen: bool,
    console: Rc<RefCell<crate::console::ConsoleLog>>,
    /// The current document's host environment, kept so native edits charge the same node and
    /// text counters as script (see `reserve_dom_budget`).
    dom: Option<Rc<crate::dom::Dom>>,
    /// `Some` for a nested browsing context: its identity, sandbox and origin policy.
    pub(crate) role: Option<frames::FrameRole>,
    /// This document's child browsing contexts. Replaced (and so torn down) with the document.
    pub(crate) frames: frames::FrameHost,
    /// The table the document's script host reads for `contentWindow`/`frames` and writes
    /// outgoing frame messages to. A new document gets a new table.
    pub(crate) frame_shared: Rc<RefCell<frames::FrameShared>>,
    /// Live-frame count and the next frame key for the whole tab (children share these).
    pub(crate) frame_counters: (Rc<Cell<usize>>, Rc<Cell<u64>>),
}
impl View {
    pub(crate) fn new(
        url: Url,
        network: Network,
        hubs: crate::workers::Hubs,
        limits: Limits,
    ) -> Result<Self, String> {
        let mut view = Self::build(url.clone(), network, hubs, limits, None)?;
        view.load(url)?;
        Ok(view)
    }
    /// A view with no document loaded yet. Nested frames pass their `role`.
    pub(crate) fn build(
        url: Url,
        network: Network,
        hubs: crate::workers::Hubs,
        limits: Limits,
        role: Option<frames::FrameRole>,
    ) -> Result<Self, String> {
        let document = WegaDocument::from_html(
            "<!doctype html><title>Wega</title>",
            DocumentSetup {
                fonts: network.fonts.clone(),
                ..Default::default()
            },
        );
        let document = Rc::new(RefCell::new(document));
        let native_document = Some(Rc::new(RefCell::new(
            crate::native_engine::NativeDocument::with_font_source(
                document.clone(),
                network.fonts.clone(),
            ),
        )));
        if let Some(native) = &native_document {
            let mut native = native.borrow_mut();
            native.viewport.width_px = 0;
            native.viewport.height_px = 0;
        }
        let (_tx, navigation_rx) = mpsc::channel();
        let raster = if role.is_none() {
            Some(wega_adapter_skia::Raster::new(network.wake.clone())?)
        } else {
            None
        };
        let csp = Arc::new(RwLock::new(Csp::default()));
        let view = Self {
            host: document_host::DocumentHost::default(),
            scheduler: crate::event_loop::Scheduler::default(),
            document,
            document_revision: 0,
            navigation_generation: 0,
            document_generation: Rc::new(Cell::new(0)),
            script: None,
            network,
            hubs,
            limits,
            abort: None,
            pending: None,
            referrer_policy: None,
            csp,
            dialog: Rc::new(RefCell::new(None)),
            activation: Rc::new(Cell::new(None)),
            navigate: Default::default(),
            navigation_rx,
            revision: Arc::new(AtomicU64::new(0)),
            last_revision: 0,
            raster,
            compatibility: None,
            script_failure: None,
            in_flight: Arc::new(AtomicUsize::new(0)),
            completed: Arc::new(Mutex::new(Vec::new())),
            started: Instant::now(),
            frame: FrameState::new(),
            reported_frames: false,
            frozen: false,
            refresh: None,
            composing: false,
            showing_error: false,
            building_error: false,
            cursor: String::new(),
            status: None,
            rendering_backend: crate::native_engine::RenderingBackend::from_env(),
            native_document,
            native_hover: None,
            native_down: None,
            last_down: None,
            click_count: 0,
            last_favicon: None,
            user_script: String::new(),
            transparent: false,
            url: url.clone(),
            loading: false,
            notices: VecDeque::new(),
            events: VecDeque::new(),
            accessibility_enabled: false,
            next_dialog_id: 1,
            pending_file_dialog: None,
            selected_files: std::collections::HashMap::new(),
            permissions: Rc::new(RefCell::new(crate::permissions::PermissionRegistry::new())),
            fullscreen: false,
            console: Rc::new(RefCell::new(crate::console::ConsoleLog::new())),
            dom: None,
            role,
            frames: Default::default(),
            frame_shared: Default::default(),
            frame_counters: Default::default(),
        };
        Ok(view)
    }
    /// Answer `alert`, `confirm` and `prompt`; without a handler they return
    /// their default (no) answer immediately.
    pub fn set_rendering_backend(&mut self, backend: crate::native_engine::RenderingBackend) {
        self.rendering_backend = backend;
        if backend == crate::native_engine::RenderingBackend::WegaNative
            && self.native_document.is_none()
        {
            self.native_document = Some(Rc::new(RefCell::new(
                crate::native_engine::NativeDocument::with_font_source(
                    self.document.clone(),
                    self.network.fonts.clone(),
                ),
            )));
        }
        if let Some(native) = &self.native_document {
            native.borrow_mut().csp = Some(self.csp.clone());
        }
        if let Some(dom) = &self.dom {
            *dom.native_document.borrow_mut() =
                if backend == crate::native_engine::RenderingBackend::WegaNative {
                    self.native_document.clone()
                } else {
                    None
                };
        }
        for child in &mut self.frames.children {
            child.view.set_rendering_backend(backend);
        }
        self.frame.invalidate();
    }
    pub fn rendering_backend(&self) -> crate::native_engine::RenderingBackend {
        self.rendering_backend
    }
    pub fn set_transparent(&mut self, transparent: bool) {
        self.transparent = transparent;
        if let Some(native) = &self.native_document {
            native.borrow_mut().canvas_background = if transparent {
                None
            } else {
                Some(peniko::Color::WHITE)
            };
        }
        self.frame.invalidate();
    }
    pub fn set_dialog_handler(&mut self, mut handler: crate::dom::DialogHandler) {
        self.set_document_dialog_handler(Box::new(move |_, _, _, kind, message, default| {
            handler(kind, message, default)
        }));
    }
    pub fn set_document_dialog_handler(&mut self, handler: crate::dom::DocumentDialogHandler) {
        *self.dialog.borrow_mut() = Some(handler);
    }
    fn local_base(&self) -> Option<std::path::PathBuf> {
        if self.url.scheme() == "file" && self.network.remote.is_none() {
            self.url
                .to_file_path()
                .ok()
                .and_then(|p| p.canonicalize().ok())
                .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
        } else {
            None
        }
    }
    fn fetch(
        &self,
        url: Url,
        limit: usize,
        context: FetchContext,
    ) -> Result<mpsc::Receiver<FetchResult>, String> {
        let (sender, receiver) = mpsc::channel();
        let wake = self.network.wake.clone();
        let network = self.network.clone();
        let local = self.local_base();
        self.network.spawn(move |client| {
            let result = network.fetch(client, &url, &context, limit, local.as_deref());
            let _ = sender.send(result);
            wake();
        })?;
        Ok(receiver)
    }
    fn same_document(&self, url: &Url) -> bool {
        let mut a = url.clone();
        a.set_fragment(None);
        let mut b = self.document.borrow().url().clone();
        b.set_fragment(None);
        a == b && self.pending.is_none() && self.script.is_some() && url.fragment().is_some()
    }
    /// Changes only when the DOM/global is replaced, not on ordinary mutations.
    pub fn document_revision(&self) -> u64 {
        self.document_revision
    }
    pub fn document_generation(&self) -> u64 {
        self.document_generation.get()
    }
    pub fn load_for_navigation(&mut self, url: Url, generation: u64) -> Result<(), String> {
        self.load_for_navigation_bypass_cache(url, generation, false)
    }
    pub fn load_for_navigation_bypass_cache(
        &mut self,
        url: Url,
        generation: u64,
        bypass_cache: bool,
    ) -> Result<(), String> {
        self.events.clear();
        self.navigate.borrow_mut().clear();
        self.navigation_generation = generation;
        self.load_bypass_cache(url, bypass_cache)
    }
    pub fn stop_for_navigation(&mut self, generation: u64) {
        self.navigation_generation = generation;
        self.stop();
    }
    pub fn load(&mut self, url: Url) -> Result<(), String> {
        self.load_bypass_cache(url, false)
    }
    pub fn load_bypass_cache(&mut self, url: Url, bypass_cache: bool) -> Result<(), String> {
        if self.same_document(&url) {
            self.document_generation.set(self.navigation_generation);
            self.navigate_fragment(&url);
            return Ok(());
        }
        let context = FetchContext {
            destination: Destination::Document,
            bypass_cache,
            ..Default::default()
        };
        self.load_request(url, context)
    }
    fn load_request(&mut self, url: Url, context: FetchContext) -> Result<(), String> {
        let doc_ctx = wega_protocol::DocumentContext {
            document: wega_protocol::DocumentId(self.document_generation.get()),
            revision: wega_protocol::DocumentRevision(self.document_revision),
        };
        self.permissions.borrow_mut().cancel_document(doc_ctx);
        self.stop_pending();
        self.scheduler.suspend();
        if let Some(script) = &mut self.script {
            script.freeze(true);
        }
        self.url = url.clone();
        self.loading = true;
        self.compatibility = None;
        self.host.ready_event_sent = false;
        self.composing = false;
        if url.as_str() == "about:blank" {
            self.set_html("<!doctype html><title>New tab</title><body></body>")?;
        } else {
            self.pending = Some(self.fetch(url, self.limits.html_bytes, context)?);
        }
        Ok(())
    }
    /// Stop loading: cancel fetches and the parser. The current document and
    /// its scripts stay.
    pub fn stop(&mut self) {
        self.stop_pending();
        self.scheduler.resume();
        if let Some(script) = &mut self.script {
            script.freeze(self.frozen);
        }
        self.document_generation.set(self.navigation_generation);
        self.url = self.document.borrow().url().clone();
    }
    fn stop_pending(&mut self) {
        if let Some(abort) = self.abort.take() {
            abort.abort();
        }
        self.pending = None;
        self.host.parser = None;
        self.host.blocking = None;
        self.host.deferred.clear();
        self.host.ordered.clear();
        self.host.asynchronous.clear();
        self.loading = false;
    }
    /// Replace the document with `html` (UTF-8, no response headers).
    pub fn set_html(&mut self, html: &str) -> Result<(), String> {
        self.stop_pending();
        self.set_document(html, encoding_rs::UTF_8, None, false)
    }
    fn set_document(
        &mut self,
        html: &str,
        encoding: &'static Encoding,
        response: Option<&Response>,
        xml: bool,
    ) -> Result<(), String> {
        if html.len() > self.limits.html_bytes {
            return Err("HTML document exceeds the size limit".into());
        }
        let document_revision = self
            .document_revision
            .checked_add(1)
            .ok_or("Document revision exhausted")?;
        if let Some(abort) = self.abort.take() {
            abort.abort();
        }
        let abort = AbortController::default();
        let (sender, receiver) = mpsc::channel();
        self.navigation_rx = receiver;
        self.showing_error = false;
        self.reported_frames = false;
        // `srcdoc` and `about:blank` documents inherit their creator's policies.
        let mut csp = match &self.role {
            Some(role) if role.inherits_origin => role.inherited_csp.clone(),
            _ => Csp::default(),
        };
        if let Some(response) = response {
            for value in response.headers.get_all("content-security-policy") {
                if let Ok(value) = value.to_str() {
                    csp.add(value);
                }
            }
        }
        self.referrer_policy = response
            .and_then(|r| r.header("referrer-policy"))
            .map(str::to_owned);
        self.csp = Arc::new(RwLock::new(csp));
        self.in_flight = Arc::new(AtomicUsize::new(0));
        self.completed = Arc::new(Mutex::new(Vec::new()));
        let setup = DocumentSetup {
            fonts: self.network.fonts.clone(),
            base_url: Some(self.url.to_string()),
            abort_signal: Some(abort.signal.clone()),
            net_provider: Some(Arc::new(Resources {
                network: self.network.clone(),
                local: self.local_base(),
                limit: self.limits.resource_bytes,
                document: self.url.clone(),
                referrer_policy: self.referrer_policy.clone(),
                csp: self.csp.clone(),
                revision: self.revision.clone(),
                in_flight: self.in_flight.clone(),
                completed: self.completed.clone(),
                omit_credentials: self.restrictions().opaque,
                resource_requests: AtomicUsize::new(0),
            })),
            navigation_provider: Some(Arc::new(Navigation(sender, self.network.wake.clone()))),
        };
        let document = Rc::new(RefCell::new(WegaDocument::empty(setup)));
        let abort = if xml {
            crate::html_report::parse_xml_document(
                document.clone(),
                html,
                self.limits.dom_nodes,
                abort,
            )?
        } else {
            abort
        };
        // A new document tears down every child browsing context of the old one.
        self.frames = Default::default();
        self.frame_shared = Default::default();
        {
            // Known before the first script runs: `window.parent` must be right immediately.
            let mut shared = self.frame_shared.borrow_mut();
            shared.has_parent = self.role.is_some();
            shared.depth = self.role.as_ref().map_or(0, |role| role.depth);
        }
        self.script = None;
        self.host.parser = None;
        self.host.blocking = None;
        self.host.deferred.clear();
        self.host.ordered.clear();
        self.host.asynchronous.clear();
        self.host.started_scripts = Default::default();
        self.host.applied_meta.clear();
        self.host.script_count = 0;
        self.host.encoding = encoding;
        self.native_hover = None;
        self.native_down = None;
        self.last_down = None;
        self.click_count = 0;
        self.document = document;
        self.native_document =
            if self.rendering_backend == crate::native_engine::RenderingBackend::WegaNative {
                Some(Rc::new(RefCell::new(
                    crate::native_engine::NativeDocument::with_font_source(
                        self.document.clone(),
                        self.network.fonts.clone(),
                    ),
                )))
            } else {
                None
            };
        if let Some(native) = &self.native_document {
            let mut native = native.borrow_mut();
            native.csp = Some(self.csp.clone());
            if self.transparent {
                native.canvas_background = None;
            }
            native.viewport.width_px = 0;
            native.viewport.height_px = 0;
        }
        self.script_failure = None;
        self.document_revision = document_revision;
        self.scheduler.replace(document_revision);
        self.document_generation.set(u64::MAX);
        self.document_generation = Rc::new(Cell::new(self.navigation_generation));
        // A new document starts a new console history; the old one is dropped with its realm.
        self.console = Rc::new(RefCell::new(crate::console::ConsoleLog::new()));
        self.events.clear();
        self.navigate.borrow_mut().clear();
        self.refresh = response
            .and_then(|r| r.header("refresh"))
            .and_then(|value| self.parse_refresh(value));
        self.abort = Some(abort);
        let dom = Rc::new(crate::dom::Dom::new(
            HostContext {
                document: self.document.clone(),
                navigate: self.navigate.clone(),
                network: self.network.clone(),
                hubs: self.hubs.clone(),
                csp: self.csp.clone(),
                dialog: self.dialog.clone(),
                document_generation: self.document_generation.clone(),
                document_revision: self.document_revision,
                activation: self.activation.clone(),
                started_scripts: self.host.started_scripts.clone(),
                permissions: self.permissions.clone(),
                console: self.console.clone(),
                restrictions: self.restrictions(),
                frames: self.frame_shared.clone(),
                frame_id: self.role.as_ref().map(|r| r.frame_id),
            },
            &self.limits,
            html.len(),
        ));
        *dom.native_document.borrow_mut() = self.native_document.clone();
        self.dom = Some(dom.clone());
        // A sandbox without `allow-scripts` gets no realm at all: no script, no handlers.
        if self.role.as_ref().is_none_or(|role| role.sandbox.scripts) {
            let mut host = crate::script::create(dom, &self.limits)?;
            host.allow_eval(self.csp().allows_eval());
            host.run(
                include_str!("../../../src/page_state.js"),
                "wega:page-state",
            )
            .map_err(|e| e.to_string())?;
            if self.restrictions().opaque {
                // Fail closed: if the lockdown cannot be installed, there is no realm.
                host.run(frames::OPAQUE_LOCKDOWN, "wega:opaque-origin")
                    .map_err(|e| format!("Opaque-origin lockdown failed: {e}"))?;
            }
            self.script = Some(host);
        }
        if !self.user_script.trim().is_empty() {
            let script = self.user_script.clone();
            let _ = self.run(&script, "wega:userscript");
        }
        if xml {
            // XML documents are parsed in one pass; their scripts run in order afterwards.
            let scripts = self
                .document
                .borrow()
                .query_selector_all("script")
                .map(|ids| ids.to_vec())
                .unwrap_or_default();
            for node in scripts {
                self.prepare_script(node, true, true);
            }
        } else {
            self.host.parser = Some(Parser::new(
                self.document.clone(),
                html,
                self.limits.dom_nodes,
                HtmlReport,
            ));
        }
        self.frame.invalidate();
        self.host.ready_event_sent = false;
        self.host.load_event_sent = false;
        self.host.ready_at = None;
        self.composing = false;
        self.loading = true;
        // Parse (and run inline scripts) as far as possible before returning.
        self.advance();
        self.check_favicon();
        Ok(())
    }
    pub(crate) fn restrictions(&self) -> frames::Restrictions {
        self.role
            .as_ref()
            .map_or_else(Default::default, |role| frames::Restrictions {
                opaque: role.opaque,
                forms_blocked: !role.sandbox.forms,
                popups_blocked: !role.sandbox.popups,
                modals_blocked: !role.sandbox.modals,
            })
    }
    fn csp(&self) -> std::sync::RwLockReadGuard<'_, Csp> {
        self.csp
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn parse_refresh(&self, value: &str) -> Option<(Instant, Url)> {
        let (delay, rest) = value
            .split_once([';', ','])
            .map_or((value, ""), |(d, r)| (d, r));
        let delay = delay.trim().parse::<f64>().ok()?;
        if !(0.0..=3600.0).contains(&delay) {
            return None;
        }
        let target = rest.trim();
        let target = target
            .strip_prefix("url=")
            .or_else(|| target.strip_prefix("URL="))
            .unwrap_or(target)
            .trim_matches(['"', '\''])
            .trim();
        let base = self.document.borrow().url().clone();
        let url = if target.is_empty() {
            base
        } else {
            base.join(target).ok()?
        };
        matches!(url.scheme(), "http" | "https" | "file")
            .then(|| (Instant::now() + Duration::from_secs_f64(delay), url))
    }
    pub fn poll(&mut self) {
        if self.frozen {
            return;
        }
        let result = self.pending.as_ref().and_then(|r| r.try_recv().ok());
        if let Some(result) = result {
            self.pending = None;
            match result {
                Ok(response) => self.commit(response),
                Err(error) => self.load_error(error),
            }
        }
        // A provisional navigation retains pixels, but cannot run the old realm.
        if !self.scheduler.script_allowed() {
            return;
        }
        self.document.borrow_mut().handle_messages();
        let revision = self.revision.load(Ordering::Relaxed);
        if revision != self.last_revision {
            self.last_revision = revision;
            self.frame.invalidate();
        }
        use crate::event_loop::Task;
        for task in [Task::Parser, Task::Lifecycle, Task::Timers, Task::Resources] {
            self.scheduler.enqueue(task);
        }
        while let Some(task) = self.scheduler.next() {
            match task {
                Task::Parser => self.advance(),
                Task::Lifecycle => self.lifecycle(),
                Task::Resources => self.resource_events(),
                Task::Timers => {
                    if let Some(script) = &mut self.script {
                        let result = script.tick();
                        if let Err(error) = result {
                            self.script_error(error);
                        }
                        self.frame.invalidate();
                        self.drain_requests();
                    }
                }
            }
        }
        if let Some(script) = &mut self.script {
            for error in script.take_errors() {
                if error.contains("; missing:") {
                    self.compatibility.get_or_insert_with(|| error.clone());
                }
                self.notices.push_back(error);
            }
        }
        while self.notices.len() > 16 {
            self.notices.pop_front();
        }
        self.sync_frames();
        if self.pending.is_none() {
            self.url = self.document.borrow().url().clone();
        }
        if let Some((due, url)) = self.refresh.clone()
            && Instant::now() >= due
            && self.host.load_event_sent
        {
            self.refresh = None;
            self.navigate.borrow_mut().push(url);
        }
        while let Ok(options) = self.navigation_rx.try_recv() {
            self.navigation(options);
        }
    }
    /// Show a navigation response according to its type.
    fn commit(&mut self, response: Response) {
        if let Err(reason) = self.frame_response_allowed(&response) {
            // A document that forbids embedding is never shown (and never scriptable) here.
            self.url = response.url.clone();
            self.notice(format!("Frame blocked: {reason}"));
            let _ = self.set_html(&frames::blocked_page(&reason));
            return;
        }
        self.url = response.url.clone();
        if response.status == 401 {
            let realm = response.header("www-authenticate").and_then(|h| {
                let lower = h.to_ascii_lowercase();
                if let Some(pos) = lower.find("realm=") {
                    let rem = &h[pos + 6..];
                    let trimmed = rem.trim_matches(|c| c == '"' || c == '\'');
                    let end = trimmed
                        .find('"')
                        .or_else(|| trimmed.find(','))
                        .unwrap_or(trimmed.len());
                    Some(trimmed[..end].to_string())
                } else {
                    None
                }
            });
            self.events.push_back(PageEvent::Authentication {
                url: response.url.to_string(),
                realm,
            });
        }
        if response.status >= 400 {
            self.notice(format!("HTTP {} for {}", response.status, response.url));
        }
        let kind = encoding::document_kind(
            response.header("content-type"),
            response.header("content-disposition"),
            &response.bytes,
        );
        let result = match kind {
            DocumentKind::Download => {
                self.scheduler.resume();
                if let Some(script) = &mut self.script {
                    script.freeze(self.frozen);
                }
                self.document_generation.set(self.navigation_generation);
                self.events
                    .push_back(PageEvent::Download(response.url.clone()));
                self.url = self.document.borrow().url().clone();
                self.loading = false;
                Ok(())
            }
            DocumentKind::Html => {
                let (html, encoding) =
                    encoding::decode_html(&response.bytes, response.header("content-type"));
                self.set_document(&html, encoding, Some(&response), false)
            }
            DocumentKind::Xml => {
                let (text, encoding) =
                    encoding::decode_html(&response.bytes, response.header("content-type"));
                self.set_document(&text, encoding, Some(&response), true)
            }
            DocumentKind::Text => {
                let (text, encoding) =
                    encoding::decode_html(&response.bytes, response.header("content-type"));
                let escaped = escape(&text);
                let title = escape(
                    response
                        .url
                        .path_segments()
                        .and_then(|mut s| s.next_back())
                        .unwrap_or_default(),
                );
                self.set_document(
                    &format!("<!doctype html><title>{title}</title><pre style='white-space:pre-wrap;word-wrap:break-word;margin:8px;font:13px ui-monospace,monospace'>{escaped}</pre>"),
                    encoding,
                    None,
                    false,
                )
            }
            DocumentKind::Image => {
                let title = escape(
                    response
                        .url
                        .path_segments()
                        .and_then(|mut s| s.next_back())
                        .unwrap_or_default(),
                );
                let src = escape(response.url.as_str());
                self.set_document(
                    &format!("<!doctype html><title>{title}</title><body style='margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:#0e0e0e'><img src=\"{src}\" style='max-width:100%;max-height:100vh'></body>"),
                    encoding_rs::UTF_8,
                    None,
                    false,
                )
            }
        };
        if let Err(error) = result {
            self.load_error(error);
        }
    }
    /// A navigation started by the page (link, form, script).
    fn navigation(&mut self, options: NavigationOptions) {
        if options.method == wega_network::document::net::http::Method::GET {
            if self.same_document(&options.url) {
                self.navigate_fragment(&options.url);
            } else if self.navigate.borrow().len() < 8 {
                self.navigate.borrow_mut().push(options.url);
            }
            return;
        }
        // POST forms load in this worker; the browser sees the new URL in State.
        let (body, content_type) =
            encode_body(&options.document_resource, options.content_type.as_deref());
        let context = FetchContext {
            method: reqwest::Method::POST,
            resource_csp: None,
            body: Some(body),
            content_type: Some(content_type),
            initiator: Some(self.document.borrow().url().clone()),
            referrer_policy: self.referrer_policy.clone(),
            destination: Destination::Document,
            bypass_cache: false,
            bypass_service_worker: false,
            omit_credentials: false,
            frame: self.role.as_ref().map(|r| r.frame_id),
            parent_frame: self.role.as_ref().and_then(|r| r.parent_frame_id),
        };
        if let Err(error) = self.load_request(options.url, context) {
            self.notice(error);
        }
    }
    /// Same-document fragment navigation: scroll, history entry, hashchange.
    fn navigate_fragment(&mut self, url: &Url) {
        self.run(
            &format!(
                "__wega_navigate_fragment({})",
                serde_json::json!(url.as_str())
            ),
            "wega:fragment",
        );
        self.url = self.document.borrow().url().clone();
        self.frame.invalidate();
    }
    /// Browser back/forward: handled within this document's history entries
    /// when possible, else reported back to the browser.
    pub fn traverse(&mut self, delta: i32) {
        let handled = self
            .evaluate_json(&format!("__wega_traverse({delta})"))
            .as_deref()
            == Ok("true");
        if !handled {
            self.events.push_back(PageEvent::Traverse(delta));
        }
        self.url = self.document.borrow().url().clone();
        self.frame.invalidate();
    }
    fn notice(&mut self, error: String) {
        if self.notices.len() >= 16 {
            self.notices.pop_front();
        }
        self.notices.push_back(error);
        self.frame.invalidate();
    }
    fn load_error(&mut self, error: String) {
        if self.role.is_some() {
            crate::diagnostics::Diagnostic::error(
                crate::diagnostics::Category::Network,
                format!("frame load failed: {error}"),
                Some(self.url.to_string()),
            )
            .emit();
        }
        let escaped = escape(&error);
        self.showing_error = false;
        self.building_error = true;
        let _=self.set_html(&format!("<!doctype html><title>Page unavailable</title><body style='font:18px system-ui;padding:36px;background:white;color:#142b24'><h1>This page could not be loaded</h1><p>{escaped}</p><p>Reload the page to retry.</p>"));
        self.building_error = false;
        self.showing_error = true;
        self.notice(error);
    }
    fn script_error(&mut self, error: wega_adapter_v8::ScriptError) {
        if error.message == "Module graph is loading" {
            return;
        }
        crate::diagnostics::Diagnostic::error(
            crate::diagnostics::Category::Javascript,
            error.to_string(),
            Some(self.url.to_string()),
        )
        .emit();
        // Non-fatal script errors already reach the console history through `__wega_report`;
        // a fatal stop never does, and is exactly what the next DevTools session must show.
        if error.fatal {
            self.console.borrow_mut().exception(
                wega_protocol::inspector::ExceptionKind::Uncaught,
                &error.message,
                self.url.as_str(),
            );
        }
        if !error.fatal && error.message.contains("; missing:") {
            self.compatibility
                .get_or_insert_with(|| error.message.clone());
        }
        if error.fatal {
            self.script_failure = Some(error.message.clone());
            self.navigate.borrow_mut().clear();
            self.activation.set(None);
            self.script = None;
            self.dom = None;
            self.host.blocking = None;
            self.host.deferred.clear();
            self.host.ordered.clear();
            self.host.asynchronous.clear();
        }
        self.notice(error.to_string());
    }
    fn run(&mut self, source: &str, name: &str) {
        self.run_program(source, name, false);
    }
    fn run_program(&mut self, source: &str, name: &str, module: bool) {
        if !self.scheduler.script_allowed() {
            return;
        }
        if let Some(script) = &mut self.script {
            let result = if module {
                script.run_module(source, name)
            } else {
                script.run(source, name)
            };
            let result = crate::script::finish_task(script.as_mut(), result);
            if let Err(error) = result {
                self.script_error(error);
            }
            self.frame.invalidate();
        }
    }
    pub fn evaluate(&mut self, source: &str) -> Result<String, String> {
        if !self.scheduler.script_allowed() {
            return Err("Document is suspended for navigation".into());
        }
        let script = self.script.as_mut().ok_or("No script context")?;
        let result = script.run(source, "evaluate");
        let result = crate::script::finish_task(script.as_mut(), result);
        self.frame.invalidate();
        if let Err(error) = &result {
            self.script_error(error.clone());
        }
        self.drain_requests();
        result.map_err(|e| e.to_string())
    }
    pub fn evaluate_json(&mut self, source: &str) -> Result<String, String> {
        if !self.scheduler.script_allowed() {
            return Err("Document is suspended for navigation".into());
        }
        let script = self.script.as_mut().ok_or("No script context")?;
        let result = script.evaluate_json(source);
        let result = crate::script::finish_task(script.as_mut(), result);
        self.frame.invalidate();
        if let Err(error) = &result {
            self.script_error(error.clone());
        }
        self.drain_requests();
        result.map_err(|e| e.to_string())
    }

    pub fn answer_permission(&mut self, id: u64, allow: bool) {
        let answer = self.permissions.borrow_mut().answer_prompt(id, allow);
        if let Some((_origin, _feature, allowed)) = answer {
            let state = if allowed { "granted" } else { "denied" };
            let _ = self.run(
                &format!("if (typeof __wega_permission_answered === 'function') __wega_permission_answered({}, {:?});", id, state),
                "wega:permissions",
            );
        }
    }

    pub fn revoke_permission(
        &mut self,
        origin: &str,
        feature: wega_protocol::permissions::PermissionFeature,
    ) {
        let res = self.permissions.borrow_mut().revoke(origin, feature);
        self.events.push_back(PageEvent::PermissionRevoked {
            origin: origin.to_string(),
            feature,
        });
        let tracks_json =
            serde_json::to_string(&res.stopped_tracks).unwrap_or_else(|_| "[]".into());
        let locks_json = serde_json::to_string(&res.released_locks).unwrap_or_else(|_| "[]".into());
        let _ = self.run(
            &format!(
                "if (typeof __wega_revoke_permission === 'function') __wega_revoke_permission({:?}, {:?}, {}, {});",
                origin,
                feature.as_str(),
                tracks_json,
                locks_json,
            ),
            "wega:permissions",
        );
    }

    pub fn exit_fullscreen(&mut self) {
        if self.fullscreen {
            self.fullscreen = false;
            self.events.push_back(PageEvent::Fullscreen(false));
            let _ = self.run(
                "if (typeof __wega_fullscreen_changed === 'function') __wega_fullscreen_changed(false);",
                "wega:fullscreen",
            );
        }
    }

    pub fn clear_history(&mut self) {
        self.run(
            "if (typeof __wega_clear_history === 'function') __wega_clear_history();",
            "wega:history",
        );
    }
    pub fn set_user_script(&mut self, script: String) {
        self.user_script = script;
    }
    pub fn set_accessibility_enabled(&mut self, enabled: bool) {
        self.accessibility_enabled = enabled;
        if enabled {
            if let Some(json) = self.accessibility_tree() {
                self.events.push_back(PageEvent::Accessibility(json));
            }
        }
    }
    pub fn accessibility_tree(&self) -> Option<String> {
        if let Some(native) = &self.native_document {
            let mut native = native.borrow_mut();
            native.ensure_layout();
            let doc = self.document.borrow();
            return serde_json::to_string(&crate::native_engine::accessibility::build(
                &native,
                &*doc,
                doc.focused(),
            ))
            .ok();
        }
        None
    }
    pub fn accessibility_action(&mut self, action: &str, target: u64, data: Option<&str>) {
        let target_node = NodeKey::from_u64(target);
        match action.to_ascii_lowercase().as_str() {
            "focus" => {
                let before = self.document.borrow().focused();
                let _ = self.document.borrow_mut().set_focus(target_node);
                self.focus_transition(before);
                self.frame.invalidate();
            }
            "default" | "click" => {
                self.activate();
                self.click_event(target_node, None);
            }
            "set_value" | "setvalue" => {
                if let Some(val) = data {
                    let mut doc = self.document.borrow_mut();
                    let _ = doc.set_input_value(target_node, val);
                    drop(doc);
                    let init = serde_json::json!({"data": val});
                    let source = script_all(&[
                        NativeEvent::new("input", target_node).init(init.clone()),
                        NativeEvent::new("change", target_node).init(init),
                    ]);
                    self.run(&source, "wega:accessibility-action");
                    self.frame.invalidate();
                }
            }
            _ => {}
        }
        if self.accessibility_enabled {
            if let Some(json) = self.accessibility_tree() {
                self.events.push_back(PageEvent::Accessibility(json));
            }
        }
    }
    pub fn file_selected(&mut self, dialog_id: u64, paths: Vec<String>) {
        if let Some((pending_id, target)) = self.pending_file_dialog.take() {
            if pending_id == dialog_id && !paths.is_empty() {
                self.selected_files.insert(target, paths.clone());
                let path_bufs: Vec<std::path::PathBuf> =
                    paths.iter().map(std::path::PathBuf::from).collect();
                let file_names: Vec<String> = paths
                    .iter()
                    .filter_map(|p| {
                        std::path::Path::new(p)
                            .file_name()
                            .map(|f| f.to_string_lossy().to_string())
                    })
                    .collect();
                let display_val = file_names.first().cloned().unwrap_or_default();
                {
                    let mut doc = self.document.borrow_mut();
                    doc.set_selected_files(target, path_bufs);
                    let val = format!("C:\\fakepath\\{display_val}");
                    let _ = wega_dom::mutate(&mut *doc).set_attribute(
                        target,
                        QualName::new(None, ns!(), LocalName::from("value")),
                        &val,
                    );
                }
                let files_json = serde_json::to_string(&file_names).unwrap_or_else(|_| "[]".into());
                let source = format!(
                    "__wega_native_file_input({}, {});",
                    serde_json::json!(target.as_u64().to_string()),
                    files_json
                );
                self.run(&source, "wega:file-input");
                self.frame.invalidate();
            }
        }
    }
    pub fn reset_form(&mut self, form: NodeKey) {
        let mut doc = self.document.borrow_mut();
        crate::dom::Dom::reset_form_controls(&mut doc, form);
        self.frame.invalidate();
    }
    pub fn copy(&mut self) -> Option<String> {
        let text = {
            let doc = self.document.borrow();
            doc.focused().and_then(|key| doc.selected_text(key))
        };
        let text = text.or_else(|| {
            let text = self
                .evaluate_json("window.getSelection()?.toString() || ''")
                .ok();
            text.filter(|s| !s.is_empty())
        });
        if let Some(ref text) = text {
            self.events
                .push_back(PageEvent::ClipboardWrite(text.clone()));
        }
        let _ = self.evaluate_json("document.execCommand('copy')");
        text
    }
    pub fn cut(&mut self) -> Option<String> {
        let text = {
            let mut doc = self.document.borrow_mut();
            doc.focused()
                .and_then(|key| {
                    doc.with_editor(key, |editor, fonts| {
                        let selected = editor.selected_text().map(str::to_owned);
                        if selected.is_some() {
                            editor
                                .driver(fonts, &mut parley::LayoutContext::new())
                                .delete_selection();
                        }
                        selected
                    })
                })
                .flatten()
        };
        let text = text.or_else(|| {
            let text = self
                .evaluate_json("window.getSelection()?.toString() || ''")
                .ok();
            text.filter(|s| !s.is_empty())
        });
        if let Some(ref text) = text {
            self.events
                .push_back(PageEvent::ClipboardWrite(text.clone()));
        }
        let _ = self.evaluate_json("document.execCommand('cut')");
        self.frame.invalidate();
        text
    }
    pub fn paste(&mut self, text: &str) {
        let handled = {
            let mut doc = self.document.borrow_mut();
            doc.focused()
                .and_then(|key| {
                    doc.with_editor(key, |editor, fonts| {
                        let mut layout = parley::LayoutContext::new();
                        let mut driver = editor.driver(fonts, &mut layout);
                        driver.insert_or_replace_selection(text);
                    })
                })
                .is_some()
        };
        if !handled {
            let text_json = serde_json::to_string(text).unwrap_or_default();
            let _ = self.evaluate_json(&format!(
                "document.execCommand('paste', false, {text_json})"
            ));
        }
        self.frame.invalidate();
    }
    pub fn select_all(&mut self) {
        let handled = {
            let mut doc = self.document.borrow_mut();
            doc.focused()
                .and_then(|key| {
                    doc.with_editor(key, |editor, fonts| {
                        let mut layout = parley::LayoutContext::new();
                        let mut driver = editor.driver(fonts, &mut layout);
                        driver.select_all();
                    })
                })
                .is_some()
        };
        if !handled {
            let _ = self.evaluate_json("document.execCommand('selectAll')");
        }
        self.frame.invalidate();
    }
    pub fn find_in_page(
        &mut self,
        query: &str,
        backwards: bool,
        case_sensitive: bool,
    ) -> (u32, u32) {
        let query_json = serde_json::to_string(query).unwrap_or_default();
        let js = format!(
            "window.find({query_json}, {}, {}, true); (window.__wegaBrowserFind || {{}})",
            if case_sensitive { "true" } else { "false" },
            if backwards { "true" } else { "false" }
        );
        let mut active = 0u32;
        let mut total = 0u32;
        if let Ok(result_str) = self.evaluate_json(&js) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&result_str) {
                if let Some(idx) = val.get("index").and_then(|v| v.as_u64()) {
                    active = (idx as u32) + 1;
                }
                if let Some(tot) = val.get("total").and_then(|v| v.as_u64()) {
                    total = tot as u32;
                }
            }
        }
        self.events.push_back(PageEvent::FindResult {
            active_match_ordinal: active,
            number_of_matches: total,
            query: query.to_string(),
        });
        self.frame.invalidate();
        (active, total)
    }
    pub fn notify_media_session_action(
        &mut self,
        action: wega_protocol::media::MediaSessionAction,
    ) {
        let action_str = match action {
            wega_protocol::media::MediaSessionAction::Play => "play",
            wega_protocol::media::MediaSessionAction::Pause => "pause",
            wega_protocol::media::MediaSessionAction::Stop => "stop",
            wega_protocol::media::MediaSessionAction::SeekBackward => "seekbackward",
            wega_protocol::media::MediaSessionAction::SeekForward => "seekforward",
            wega_protocol::media::MediaSessionAction::PreviousTrack => "previoustrack",
            wega_protocol::media::MediaSessionAction::NextTrack => "nexttrack",
            wega_protocol::media::MediaSessionAction::SkipAd => "skipad",
            wega_protocol::media::MediaSessionAction::SeekTo { .. } => "seekto",
        };
        let js = format!(
            "(function() {{                 if (typeof navigator !== \"undefined\" && navigator.mediaSession && navigator.mediaSession._handlers) {{                     const h = navigator.mediaSession._handlers[\"{}\"];                     if (typeof h === \"function\") {{ h({{ action: \"{}\" }}); }}                 }}             }})()",
            action_str, action_str
        );
        let _ = self.evaluate_json(&js);
    }
    pub fn post_frame_message(
        &mut self,
        _target: FrameId,
        origin: &str,
        target_origin: Option<&str>,
        message: &str,
    ) {
        let origin_json = serde_json::to_string(origin).unwrap_or_default();
        let target_origin_json = match target_origin {
            Some(to) => serde_json::to_string(to).unwrap_or_else(|_| "null".to_string()),
            None => "null".to_string(),
        };
        let message_json = serde_json::to_string(message).unwrap_or_default();
        let code = format!(
            "(() => {{ try {{ const origin = {origin_json}; const targetOrigin = {target_origin_json}; const currentOrigin = (window.location && window.location.origin) || ''; if (targetOrigin && targetOrigin !== '*' && targetOrigin !== '/' && targetOrigin !== currentOrigin) {{ return; }} let parsed; try {{ parsed = JSON.parse({message_json}); }} catch(_) {{ parsed = {message_json}; }} const MsgEvent = globalThis.MessageEvent || window.MessageEvent || Event; const event = new MsgEvent('message', {{ data: parsed, origin: origin, source: null }}); window.dispatchEvent(event); }} catch(e) {{ console.error('Frame postMessage dispatch error:', e); }} }})()"
        );
        let _ = self.evaluate_json(&code);
        self.frame.invalidate();
    }
    pub fn favicon_url(&self) -> Option<Url> {
        let doc = self.document.borrow();
        for selector in &[
            "link[rel~='icon']",
            "link[rel='shortcut icon']",
            "link[rel='icon']",
        ] {
            if let Ok(Some(id)) = doc.query_selector(selector) {
                if let Some(node) = doc.node(id) {
                    if node.element_name().is_some() {
                        let elem = node;
                        if let Some(href) = elem.attr("href") {
                            if let Ok(url) = self.url.join(href) {
                                return Some(url);
                            } else if let Ok(url) = Url::parse(href) {
                                return Some(url);
                            }
                        }
                    }
                }
            }
        }
        if matches!(self.url.scheme(), "http" | "https") {
            self.url.join("/favicon.ico").ok()
        } else {
            None
        }
    }
    pub(crate) fn check_favicon(&mut self) {
        if let Some(favicon) = self.favicon_url() {
            if self.last_favicon.as_ref() != Some(&favicon) {
                self.last_favicon = Some(favicon.clone());
                self.events.push_back(PageEvent::Favicon(favicon));
            }
        }
    }
    pub fn take_script_failure(&mut self) -> Option<String> {
        self.script_failure.take()
    }
    pub fn title(&self) -> String {
        let doc = self.document.borrow();
        doc.query_selector("title")
            .ok()
            .flatten()
            .and_then(|id| wega_dom::content::text_content(&*doc, id))
            .map(|text| text.split_ascii_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
    }
    pub fn next_navigation(&mut self) -> Option<Url> {
        let mut queue = self.navigate.borrow_mut();
        if queue.is_empty() {
            None
        } else {
            Some(queue.remove(0))
        }
    }
    pub fn next_wakeup(&self) -> Option<Duration> {
        if self.frozen || !self.scheduler.script_allowed() {
            return None;
        }
        let parser_runnable = self
            .host
            .parser
            .as_ref()
            .is_some_and(|parser| !parser.is_finished())
            && self.host.blocking.is_none();
        if parser_runnable || self.host.yielded {
            return Some(Duration::ZERO);
        }
        let mut wake = self.script.as_ref().and_then(|script| script.next_timer());
        if let Some(frames) = self.frames_next_wakeup() {
            wake = Some(wake.map_or(frames, |w| w.min(frames)));
        }
        let mut at = |instant: Instant| {
            let delay = instant.saturating_duration_since(Instant::now());
            wake = Some(wake.map_or(delay, |w| w.min(delay)));
        };
        if let Some((due, _)) = &self.refresh {
            at(*due);
        }
        if let Some(ready) = self.host.ready_at
            && !self.host.load_event_sent
        {
            at(ready + Duration::from_secs(30));
        }
        wake
    }
    pub fn heap_bytes(&self) -> usize {
        self.script.as_ref().map_or(0, |script| script.heap_bytes())
    }
    /// Whether this document's own history (pushState, fragments) can go back / forward.
    pub fn document_history(&self) -> (bool, bool) {
        self.script.as_ref().map_or((false, false), |script| {
            let (index, length) = script.history();
            (index > 0, index + 1 < length)
        })
    }
    /// Cookies for a download of `url`, as Netscape cookie-file lines.
    pub fn download_cookies(&self, url: &Url) -> String {
        self.network.cookies.netscape(url)
    }
    /// Reserve the budget of a **native** (inspector) DOM edit before performing it: `nodes` new
    /// nodes, `text_bytes` of new text and the resulting nesting `depth`. It charges the very same
    /// counters script host ops charge, so script and native edits share one budget and a native
    /// edit can neither exceed the node/text limits nor create nesting beyond what is known to lay
    /// out (121). Edits must call this first and perform the mutation only on `Ok`.
    pub fn reserve_dom_budget(
        &self,
        nodes: usize,
        text_bytes: usize,
        depth: usize,
    ) -> Result<(), String> {
        self.dom
            .as_ref()
            .ok_or_else(|| "The document has no script environment".to_string())?
            .reserve_edit(nodes, text_bytes, depth)
    }
    /// The document's mutation epoch (`wega_dom::MutationEpoch`) as an integer: it changes when
    /// the DOM, inline styles or stylesheet set change through any Wega mutation path (script
    /// host ops, the HTML parser, native edits) and starts again with each new document.
    pub fn mutation_epoch(&self) -> u64 {
        use wega_dom::MutationClock as _;
        self.document.borrow().mutation_epoch().as_u64()
    }
    /// Whether geometry currently reflects an older layout (or none). Never flushes.
    pub fn layout_pending(&self) -> bool {
        self.native_document
            .as_ref()
            .is_none_or(|native| native.borrow().is_dirty())
    }
    pub fn node_count(&self) -> usize {
        let doc = self.document.borrow();
        doc.node_count()
    }
    /// Script-created nodes and charged text bytes, for budget diagnostics.
    pub fn dom_budget(&self) -> Option<(usize, usize)> {
        self.script.as_ref().map(|script| script.budget())
    }
    pub fn needs_paint(&self) -> bool {
        self.frame.needs_paint(self.frozen) || self.frames_need_paint()
    }
    pub fn request_redraw(&mut self) {
        self.frame.invalidate();
    }
    pub fn freeze(&mut self, freeze: bool) {
        let changed = self.frozen != freeze;
        self.frozen = freeze;
        self.scheduler.freeze(freeze);
        if !freeze && let Some(script) = &mut self.script {
            script.freeze(false);
        }
        if changed {
            // Pause timers before hiding; resume them before delivering the
            // visible event so overdue callbacks are not postponed by this task.
            self.run(
                &format!("__wega_visibility({freeze})"),
                "wega:visibilitychange",
            );
        }
        if freeze && let Some(script) = &mut self.script {
            script.freeze(true);
        }
        if freeze {
            self.document.borrow_mut().release_image_cache();
            if let Some(raster) = &self.raster {
                raster.release();
            }
        }
        if !freeze {
            self.frame.invalidate();
        }
    }
    pub fn activate(&mut self) {
        self.activation
            .set(Some(Instant::now() + Duration::from_secs(5)));
    }
    /// Focus has one owner, the document. After anything that can move it, compare with the
    /// state before and dispatch the transition once: `blur` and `focusout` on the node that
    /// lost focus, `focus` and `focusin` on the one that gained it, each with the other as
    /// `relatedTarget`. Blitz's own focus events are not forwarded (see `generated_events`).
    pub(crate) fn focus_transition(&mut self, before: Option<NodeKey>) {
        let after = self.document.borrow().focused();
        if before == after {
            return;
        }
        let id = |key: Option<NodeKey>| match key {
            Some(key) => serde_json::json!(key.to_string()),
            None => serde_json::Value::Null,
        };
        let source = format!("__wega_focus_events({}, {})", id(before), id(after));
        self.run(&source, "wega:focus");
        self.frame.invalidate();
    }
    /// Where keyboard input goes: the focused node, else the document's fallback target.
    fn keyboard_target(&self) -> NodeKey {
        let doc = self.document.borrow();
        doc.keyboard_target().unwrap_or_else(|| doc.root())
    }
    /// Dispatch a mouse/pointer event to script; true when it was cancelled.
    fn dispatch_pointer(
        &mut self,
        kind: &str,
        target: NodeKey,
        pointer: &Pointer,
        related: Option<NodeKey>,
    ) -> bool {
        let init = serde_json::json!({
            "clientX": pointer.x, "clientY": pointer.y,
            "screenX": pointer.x, "screenY": pointer.y,
            "button": pointer.button, "buttons": pointer.buttons,
            "shiftKey": pointer.modifiers & 1 != 0, "ctrlKey": pointer.modifiers & 2 != 0,
            "altKey": pointer.modifiers & 4 != 0, "metaKey": pointer.modifiers & 8 != 0,
            "deltaX": pointer.delta_x, "deltaY": pointer.delta_y,
        });
        let event = NativeEvent::new(kind.to_owned(), target)
            .init(init)
            .related(related);
        self.send(&event)
    }
    /// Dispatch one native event to script; true when it was cancelled.
    fn send(&mut self, event: &NativeEvent) -> bool {
        self.evaluate_json(&event.script()).as_deref() == Ok("true")
    }
    /// Pointer input: hover, mouse/pointer events, Blitz defaults (focus, text
    /// selection, clicks), wheel scrolling and the cursor.
    pub fn pointer(&mut self, pointer: Pointer) {
        if let Some(native) = &self.native_document {
            native.borrow_mut().ensure_layout();
        }
        if self.route_pointer_to_frame(&pointer) {
            return;
        }
        let before = self.document.borrow().focused();
        self.pointer_input(pointer);
        self.focus_transition(before);
    }
    fn pointer_input(&mut self, pointer: Pointer) {
        self.native_pointer_input(pointer);
    }
    fn hover_changed(
        &mut self,
        previous: Option<NodeKey>,
        hovered: Option<NodeKey>,
        pointer: &Pointer,
    ) {
        let (old_chain, new_chain) = {
            let doc = self.document.borrow();
            let chain = |id: Option<NodeKey>| {
                id.map(|id| element_chain(&*doc, id).into_iter().collect::<Vec<_>>())
                    .unwrap_or_default()
            };
            (chain(previous), chain(hovered))
        };
        if let Some(target) = previous
            .filter(|id| wega_dom::DocumentView::node(&*self.document.borrow(), *id).is_some())
        {
            self.dispatch_pointer("pointerout", target, pointer, hovered);
            self.dispatch_pointer("mouseout", target, pointer, hovered);
            for node in old_chain.iter().filter(|n| !new_chain.contains(n)) {
                self.dispatch_pointer("pointerleave", *node, pointer, hovered);
                self.dispatch_pointer("mouseleave", *node, pointer, hovered);
            }
        }
        if let Some(target) = hovered {
            self.dispatch_pointer("pointerover", target, pointer, previous);
            self.dispatch_pointer("mouseover", target, pointer, previous);
            for node in new_chain.iter().rev().filter(|n| !old_chain.contains(n)) {
                self.dispatch_pointer("pointerenter", *node, pointer, previous);
                self.dispatch_pointer("mouseenter", *node, pointer, previous);
            }
        }
        let cursor = self
            .native_document
            .as_ref()
            .and_then(|n| {
                let mut native = n.borrow_mut();
                native.ensure_layout();
                hovered
                    .and_then(|key| native.find_box(key))
                    .map(|b| b.style.cursor.clone())
            })
            .unwrap_or_else(|| "default".into());
        if cursor != self.cursor {
            self.cursor = cursor.clone();
            self.events.push_back(PageEvent::Cursor(cursor));
        }
        let link_status = {
            let doc = self.document.borrow();
            new_chain.iter().find_map(|node_id| {
                let node = doc.node(*node_id)?;
                node.element_name()?;
                let elem = node;
                if elem.element_name().is_some_and(|n| n.local == "a") {
                    let href = elem.attr("href")?;
                    self.url.join(href).ok().map(|u| u.to_string())
                } else {
                    None
                }
            })
        };
        if link_status != self.status {
            self.status = link_status.clone();
            self.events.push_back(PageEvent::Status(link_status));
        }
    }
    /// A primary click at viewport coordinates: down, up and the click it generates.
    pub fn click(&mut self, x: f32, y: f32) {
        if self.frozen || !self.scheduler.script_allowed() {
            return;
        }
        if let Some(native) = &self.native_document {
            native.borrow_mut().ensure_layout();
        }
        for kind in [PointerKind::Move, PointerKind::Down, PointerKind::Up] {
            let mut pointer = Pointer::new(kind, x, y);
            if kind == PointerKind::Down {
                pointer.buttons = 1;
            }
            self.pointer(pointer);
        }
    }
    /// A click event: script listeners, then Wega's defaults (form submission
    /// events, new windows, fragment links), then Blitz's.
    fn click_event(&mut self, target: NodeKey, pointer: Option<&Pointer>) {
        let pointer = pointer
            .copied()
            .unwrap_or_else(|| Pointer::new(PointerKind::Up, 0.0, 0.0));
        let disabled = {
            let doc = self.document.borrow();
            let mut current = Some(target);
            let mut disabled = false;
            while let Some(key) = current {
                let Some(node) = doc.node(key) else { break };
                if node.attr("disabled").is_some() {
                    disabled = true;
                    break;
                }
                current = node.parent();
            }
            disabled
        };
        if disabled {
            return;
        }
        self.activate();
        if self.dispatch_pointer("click", target, &pointer, None) {
            return;
        }
        if !self.default_click(target) {
            self.control_click(target);
        }
    }
    fn control_click(&mut self, target: NodeKey) {
        let (kind, checked) = {
            let doc = self.document.borrow();
            let Some(node) = doc.node(target) else { return };
            if !node.element_name().is_some_and(|n| n.local == "input") {
                return;
            }
            (
                node.attr("type").unwrap_or_default().to_ascii_lowercase(),
                doc.checked(target),
            )
        };
        if kind == "checkbox" {
            self.document.borrow_mut().set_checked(target, !checked);
        } else if kind == "radio" && !checked {
            self.document.borrow_mut().toggle_radio(target);
        } else {
            return;
        }
        let source = script_all(&[
            NativeEvent::new("input", target),
            NativeEvent::new("change", target),
        ]);
        self.run(&source, "wega:native-toggle");
    }
    /// Wega's own click defaults; true when the click was fully handled.
    fn default_click(&mut self, target: NodeKey) -> bool {
        enum Action {
            Submit(NodeKey, NodeKey),
            Reset(NodeKey),
            FileDialog(NodeKey, bool, Vec<String>),
            NewWindow(Url),
            Navigate(Url),
            Fragment(Url),
        }
        let action = {
            let doc = self.document.borrow();
            let mut current = Some(target);
            let mut action = None;
            while let Some(id) = current {
                let Some(node) = doc.node(id) else { break };
                if let Some(name) = node.element_name() {
                    let element = node;
                    if element.attr("disabled").is_some() {
                        return true;
                    }
                    let tag = name.local;
                    let kind = element.attr("type").unwrap_or_default();
                    if matches!(tag, "input" | "button") && kind.eq_ignore_ascii_case("reset") {
                        if let Some(form) = crate::dom::Dom::form_owner(&*doc, id) {
                            action = Some(Action::Reset(form));
                            break;
                        }
                    }
                    if tag == "input" && kind.eq_ignore_ascii_case("file") {
                        let multiple = element.attr("multiple").is_some();
                        let accept = element
                            .attr("accept")
                            .map(|a| {
                                a.split(",")
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect()
                            })
                            .unwrap_or_default();
                        action = Some(Action::FileDialog(id, multiple, accept));
                        break;
                    }
                    if matches!(tag, "input" | "button")
                        && (tag == "button" && !matches!(kind, "button" | "reset")
                            || kind.eq_ignore_ascii_case("submit")
                            || (tag == "input" && kind.eq_ignore_ascii_case("image")))
                    {
                        action = crate::dom::Dom::form_owner(&*doc, id)
                            .map(|form| Action::Submit(form, id));
                        break;
                    }
                    if tag == "a"
                        && let Some(href) = element.attr("href")
                        && let Ok(url) = doc.url().join(href.trim())
                    {
                        let target_name = element
                            .attr("target")
                            .unwrap_or_default()
                            .to_ascii_lowercase();
                        if !target_name.is_empty()
                            && !matches!(target_name.as_str(), "_self" | "_top" | "_parent")
                        {
                            action = Some(Action::NewWindow(url));
                        } else if self.same_document(&url) {
                            action = Some(Action::Fragment(url));
                        } else {
                            action = Some(Action::Navigate(url));
                        }
                        break;
                    }
                }
                current = node.parent();
            }
            action
        };
        match action {
            None => false,
            Some(Action::Reset(form)) => {
                let cancelled = self.send(&NativeEvent::new("reset", form));
                if !cancelled {
                    self.reset_form(form);
                }
                true
            }
            Some(Action::FileDialog(target_id, multiple, accept)) => {
                let dialog_id = self.next_dialog_id;
                self.next_dialog_id += 1;
                self.pending_file_dialog = Some((dialog_id, target_id));
                self.events.push_back(PageEvent::FileDialog {
                    id: dialog_id,
                    multiple,
                    accept,
                });
                true
            }
            Some(Action::Submit(form, submitter)) => {
                let cancelled = self
                    .evaluate_json(&format!(
                        "__wega_submit_event({}, {})",
                        serde_json::json!(form.as_u64().to_string()),
                        serde_json::json!(submitter.as_u64().to_string())
                    ))
                    .as_deref()
                    == Ok("true");
                if !cancelled && !self.restrictions().forms_blocked {
                    self.document.borrow().submit_form(form, submitter);
                }
                true
            }
            Some(Action::Navigate(url)) => {
                if self.navigate.borrow().len() < 8 {
                    self.navigate.borrow_mut().push(url);
                }
                true
            }
            Some(Action::NewWindow(url)) => {
                if matches!(url.scheme(), "http" | "https") && !self.restrictions().popups_blocked {
                    self.events.push_back(PageEvent::OpenWindow(url));
                }
                true
            }
            Some(Action::Fragment(url)) => {
                self.navigate_fragment(&url);
                true
            }
        }
    }
    fn edited(
        &mut self,
        target: NodeKey,
        event: Option<crate::document::EditEvent>,
        input_options: serde_json::Value,
    ) {
        let kind = match event {
            Some(crate::document::EditEvent::Input) => "input",
            Some(crate::document::EditEvent::Select) => "select",
            _ => return,
        };
        let event = NativeEvent::new(kind, target).init(if kind == "input" {
            input_options
        } else {
            serde_json::json!({})
        });
        self.run(&event.script(), "wega:native-event");
    }
    pub fn key(&mut self, event: crate::events::KeyEvent) {
        if let Some(child) = self.focused_child() {
            child.key(event);
            self.frame.invalidate();
            return;
        }
        let before = self.document.borrow().focused();
        self.key_input(event);
        self.focus_transition(before);
    }
    fn key_input(&mut self, event: crate::events::KeyEvent) {
        if self.frozen || !self.scheduler.script_allowed() {
            return;
        }
        let target = self.keyboard_target();
        let disabled = self
            .document
            .borrow()
            .node(target)
            .is_some_and(|el| el.attr("disabled").is_some());
        if disabled {
            return;
        }
        let pressed = event.state.is_pressed();
        if pressed {
            self.activate();
        }
        let options = serde_json::json!({"key":event.key.to_string(),"code":event.code.to_string(),"repeat":event.is_auto_repeating,"isComposing":event.is_composing,"shiftKey":event.modifiers.contains(Modifiers::SHIFT),"ctrlKey":event.modifiers.contains(Modifiers::CONTROL),"altKey":event.modifiers.contains(Modifiers::ALT),"metaKey":event.modifiers.contains(Modifiers::META)});
        // Script sees key events on the focused element, else on <body>; Blitz's own editing
        // and navigation defaults keep using the fallback target (the root element).
        let script_target = {
            let doc = self.document.borrow();
            doc.focused()
                .or_else(|| doc.query_selector("body").ok().flatten())
                .unwrap_or(target)
        };
        let keyboard = NativeEvent::new(if pressed { "keydown" } else { "keyup" }, script_target)
            .init(options);
        if self.send(&keyboard) {
            return;
        }
        if pressed {
            let key = event.key.to_string();
            if key == "Tab" {
                let Some(native) = &self.native_document else {
                    return;
                };
                let mut native = native.borrow_mut();
                native.ensure_layout();
                let mut doc = self.document.borrow_mut();
                let keys: Vec<_> = doc
                    .focus_order()
                    .into_iter()
                    .filter(|key| native.find_box(*key).is_some_and(|b| b.style.visible))
                    .collect();
                if keys.is_empty() {
                    doc.clear_focus();
                    return;
                }
                let previous = doc
                    .focused()
                    .and_then(|key| keys.iter().position(|k| *k == key));
                let backward = event.modifiers.contains(Modifiers::SHIFT);
                let index = match (previous, backward) {
                    (Some(0), true) | (None, true) => keys.len() - 1,
                    (Some(index), true) => index - 1,
                    (Some(index), false) => (index + 1) % keys.len(),
                    (None, false) => 0,
                };
                let _ = doc.set_focus(keys[index]);
                self.frame.invalidate();
                return;
            }
            let is_ctrl_or_meta = event.modifiers.contains(Modifiers::CONTROL)
                || event.modifiers.contains(Modifiers::META);
            if is_ctrl_or_meta {
                if key.eq_ignore_ascii_case("c") {
                    self.copy();
                    return;
                }
                if key.eq_ignore_ascii_case("x") {
                    self.cut();
                    return;
                }
                if key.eq_ignore_ascii_case("a") {
                    self.select_all();
                    return;
                }
                if key.eq_ignore_ascii_case("v") {
                    self.paste("");
                    return;
                }
            }
            let is_btn = {
                let doc = self.document.borrow();
                doc.node(target).is_some_and(|el| {
                    el.element_name().is_some_and(|n| n.local == "button")
                        || (el.element_name().is_some_and(|n| n.local == "input")
                            && matches!(
                                el.attr("type")
                                    .unwrap_or_default()
                                    .to_ascii_lowercase()
                                    .as_str(),
                                "button" | "submit" | "reset"
                            ))
                })
            };
            if is_btn && (key == " " || key == "Space" || key == "Enter") {
                if self.document.borrow().node(target).is_some() {
                    self.click_event(target, None);
                    self.frame.invalidate();
                    return;
                }
            }
            if key == "Enter" && self.implicit_submission(target) {
                self.frame.invalidate();
                return;
            }
            if key == " " || key == "Space" {
                let toggled = {
                    let mut doc = self.document.borrow_mut();
                    let kind = doc
                        .node(target)
                        .filter(|n| n.element_name().is_some_and(|n| n.local == "input"))
                        .and_then(|n| n.attr("type"))
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    if kind == "checkbox" {
                        let checked = doc.checked(target);
                        doc.set_checked(target, !checked);
                        Some(true)
                    } else if kind == "radio" && !doc.checked(target) {
                        doc.toggle_radio(target);
                        Some(false)
                    } else {
                        None
                    }
                };
                if toggled.is_some() {
                    let source = script_all(&[
                        NativeEvent::new("input", target),
                        NativeEvent::new("change", target),
                    ]);
                    self.document.borrow_mut().record_external_mutation();
                    self.run(&source, "wega:native-toggle");
                    self.frame.invalidate();
                    return;
                }
            }
            let input_type = match key.as_str() {
                "Backspace" => "deleteContentBackward",
                "Delete" => "deleteContentForward",
                "Enter" => "insertLineBreak",
                _ => {
                    if event.text.as_ref().is_some_and(|t| !t.is_empty())
                        && !event
                            .modifiers
                            .intersects(Modifiers::CONTROL | Modifiers::META | Modifiers::ALT)
                    {
                        "insertText"
                    } else {
                        ""
                    }
                }
            };
            let input_options = serde_json::json!({"inputType":input_type,"data":event.text.as_ref().map(|text|text.as_str()),"isComposing":event.is_composing});
            if !input_type.is_empty() {
                let before = NativeEvent::new("beforeinput", target).init(input_options.clone());
                if self.send(&before) {
                    return;
                }
            }
            let edited = self.document.borrow_mut().edit_key(target, event);
            self.edited(target, edited, input_options);
        }
        self.frame.invalidate();
    }
    /// Enter in a single-line field submits its form (HTML implicit submission):
    /// through the default button when there is one, else when the form has
    /// only one field that blocks implicit submission.
    fn implicit_submission(&mut self, field: NodeKey) -> bool {
        let (form, submitter) = {
            let doc = self.document.borrow();
            let Some(element) = doc.node(field) else {
                return false;
            };
            let kind = element.attr("type").unwrap_or("text").to_ascii_lowercase();
            let single_line = element.element_name().is_some_and(|n| n.local == "input")
                && matches!(
                    kind.as_str(),
                    "text"
                        | "search"
                        | "email"
                        | "url"
                        | "tel"
                        | "password"
                        | "date"
                        | "month"
                        | "week"
                        | "time"
                        | "datetime-local"
                        | "number"
                );
            if !single_line {
                return false;
            }
            let Some(form) = crate::dom::Dom::form_owner(&*doc, field) else {
                return false;
            };
            let mut button = None;
            let mut blocking = 0;
            for id in wega_dom::preorder(&*doc, doc.root()) {
                let Some(control) = doc.node(id) else {
                    continue;
                };
                if crate::dom::Dom::form_owner(&*doc, id) != Some(form) {
                    continue;
                }
                let Some(tag) = control.element_name().map(|n| n.local) else {
                    continue;
                };
                let kind = control
                    .attr("type")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if button.is_none()
                    && matches!(tag, "input" | "button")
                    && (tag == "button" && !matches!(kind.as_str(), "button" | "reset")
                        || kind == "submit"
                        || (tag == "input" && kind == "image"))
                {
                    button = Some((id, control.attr("disabled").is_some()));
                }
                if tag == "input"
                    && matches!(
                        if kind.is_empty() {
                            "text"
                        } else {
                            kind.as_str()
                        },
                        "text"
                            | "search"
                            | "email"
                            | "url"
                            | "tel"
                            | "password"
                            | "date"
                            | "month"
                            | "week"
                            | "time"
                            | "datetime-local"
                            | "number"
                    )
                {
                    blocking += 1;
                }
            }
            match button {
                Some((_, true)) => return true,
                Some((id, false)) => (form, Some(id)),
                None if blocking <= 1 => (form, None),
                None => return true,
            }
        };
        if let Some(button) = submitter {
            // Activation behaviour of the default button: a synthetic click.
            self.click_event(button, None);
            return true;
        }
        let cancelled = self
            .evaluate_json(&format!(
                "__wega_submit_event({}, null)",
                serde_json::json!(form.as_u64().to_string())
            ))
            .as_deref()
            == Ok("true");
        if !cancelled && !self.restrictions().forms_blocked {
            self.document.borrow().submit_form(form, form);
        }
        true
    }
    pub fn ime(&mut self, event: crate::events::ImeEvent) {
        if let Some(child) = self.focused_child() {
            child.ime(event);
            self.frame.invalidate();
            return;
        }
        if self.frozen || !self.scheduler.script_allowed() {
            return;
        }
        use crate::events::ImeEvent;
        let target = self.keyboard_target();
        let (text, kind) = match &event {
            ImeEvent::Preedit(text, _) => (text, "compositionupdate"),
            ImeEvent::Commit(text) => (text, "compositionend"),
            _ => return,
        };
        if matches!(&event,ImeEvent::Preedit(text,_) if !text.is_empty()) && !self.composing {
            self.composing = true;
            let start =
                NativeEvent::new("compositionstart", target).init(serde_json::json!({"data": ""}));
            self.run(&start.script(), "wega:composition");
        }
        let commit = matches!(event, ImeEvent::Commit(_));
        let input_options = serde_json::json!({"inputType":if commit{"insertText"}else{"insertCompositionText"},"data":text,"isComposing":!commit});
        if commit {
            self.composing = false;
            let before = NativeEvent::new("beforeinput", target).init(input_options.clone());
            if self.send(&before) {
                return;
            }
        }
        let composition = NativeEvent::new(kind, target).init(serde_json::json!({"data": text}));
        self.run(&composition.script(), "wega:composition");
        let edited = self.document.borrow_mut().edit_ime(target, event);
        self.edited(target, edited, input_options);
        self.frame.invalidate();
    }
    pub fn scroll_at(&mut self, x: f64, y: f64, pointer_x: f32, pointer_y: f32) {
        if self.frozen || !self.scheduler.script_allowed() {
            return;
        }
        if self.route_scroll_to_frame(x, y, pointer_x, pointer_y) {
            return;
        }
        if let Some(native) = &self.native_document {
            let mut native = native.borrow_mut();
            native.ensure_layout();
            if let Some(k) = native.element_at(Point {
                x: pointer_x as f64,
                y: pointer_y as f64,
            }) {
                native.scroll_by_at(k, x, y);
            }
            self.frame.invalidate();
        }
    }
    /// Scroll the viewport (or the scroll container under the pointer, via `wheel`).
    pub fn scroll(&mut self, x: f64, y: f64) {
        if self.frozen || !self.scheduler.script_allowed() {
            return;
        }
        if let Some(native) = &self.native_document {
            let mut native = native.borrow_mut();
            let p = native.scroll;
            native.scroll_viewport_to(Point {
                x: p.x + x,
                y: p.y + y,
            });
            self.frame.invalidate();
        }
    }
    fn record_paint(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        zoom: f32,
        dark: bool,
    ) -> Result<anyrender::Scene, String> {
        if width == 0
            || height == 0
            || width as usize * height as usize > self.limits.max_pixels
            || !scale.is_finite()
            || scale <= 0.0
            || !zoom.is_finite()
            || !(0.3..=5.0).contains(&zoom)
        {
            return Err("Invalid or oversized viewport".into());
        }
        let viewport = ViewportConfig {
            width_px: width,
            height_px: height,
            scale,
            zoom,
            dark,
        };
        self.native_document
            .as_ref()
            .ok_or("Native layout is unavailable")?
            .borrow_mut()
            .viewport = viewport;
        let another_frame = if let Some(script) = &mut self.script
            && self.scheduler.script_allowed()
        {
            match script.frame(self.started.elapsed().as_secs_f64() * 1000.0) {
                Ok(pending) => pending,
                Err(error) => {
                    self.script_error(error);
                    true
                }
            }
        } else {
            false
        };
        self.native_document
            .as_ref()
            .ok_or("Native layout is unavailable")?
            .borrow_mut()
            .ensure_layout();

        let device = (viewport.scale * viewport.zoom) as f64;
        let mut child_scenes = Vec::new();
        let mut frames_animating = false;
        let mut boxes = Vec::new();
        for child in &self.frames.children {
            boxes.push(self.frame_box(child.node));
        }
        for (child, content) in self.frames.children.iter_mut().zip(boxes) {
            let Some(content) = content else {
                continue;
            };
            let x = content.x * device;
            let y = content.y * device;
            let w = (content.width * device).round().max(1.0);
            let h = (content.height * device).round().max(1.0);
            if self.native_document.is_none()
                && (x >= viewport.width_px as f64
                    || y >= viewport.height_px as f64
                    || x + w <= 0.0
                    || y + h <= 0.0)
            {
                continue;
            }
            if w * h > child.view.limits.max_pixels as f64 {
                continue;
            }
            if let Ok(recorded) = child.view.record_paint(
                w as u32,
                h as u32,
                viewport.scale,
                viewport.zoom,
                viewport.dark,
            ) {
                child_scenes.push((child.node, recorded));
                frames_animating |= child.view.frame_wants_more();
            }
        }

        if let (Some(dom), Some(native)) = (&self.dom, &self.native_document) {
            use anyrender::PaintScene;
            let native = native.borrow();
            for (key, image) in dom.canvas_images() {
                let Some(b) = native.find_box(key) else {
                    continue;
                };
                let rect = b.content_box();
                let mut scene = anyrender::Scene::new();
                let brush = peniko::ImageBrush::new(image.clone());
                scene.draw_image(
                    (&brush).into(),
                    kurbo::Affine::scale_non_uniform(
                        rect.width * device / image.width as f64,
                        rect.height * device / image.height as f64,
                    ),
                );
                child_scenes.push((key, scene));
            }
        }
        let scene = self
            .native_document
            .as_ref()
            .ok_or("Native layout is unavailable")?
            .borrow_mut()
            .record_scene_with_children(&viewport, child_scenes);

        self.frame.recorded(another_frame || frames_animating);
        if self.accessibility_enabled {
            if let Some(json) = self.accessibility_tree() {
                self.events
                    .retain(|event| !matches!(event, PageEvent::Accessibility(_)));
                self.events.push_back(PageEvent::Accessibility(json));
            }
        }
        Ok(scene)
    }
    pub fn configure_shared_frames(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.raster
            .as_ref()
            .ok_or("Nested frames do not rasterise")?
            .set_store(
                wega_platform::FrameStore::open(path, 4096 * 2160 * 4)
                    .map_err(|e| e.to_string())?,
            )
    }
    pub fn configure_shared_store(
        &mut self,
        store: wega_platform::FrameStore,
    ) -> Result<(), String> {
        self.raster
            .as_ref()
            .ok_or("Nested frames do not rasterise")?
            .set_store(store)
    }
    pub fn paint_pending(&self) -> bool {
        self.raster.as_ref().is_some_and(|raster| raster.pending())
    }
    /// Records a frame for `viewport` and hands it to the raster thread; the result arrives
    /// through [`Self::take_painted`] or [`Self::wait_painted`] carrying `serial`.
    pub fn begin_paint(
        &mut self,
        viewport: ViewportConfig,
        serial: u64,
        target: FrameTarget,
    ) -> Result<(), String> {
        let scene = self.record_paint(
            viewport.width_px,
            viewport.height_px,
            viewport.scale,
            viewport.zoom,
            viewport.dark,
        )?;
        self.raster
            .as_mut()
            .ok_or("Nested frames do not rasterise")?
            .start(
                FrameRequest {
                    serial,
                    width: viewport.width_px,
                    height: viewport.height_px,
                    target,
                },
                scene,
            )
    }
    pub fn take_painted(&mut self) -> Option<Result<crate::Painted, String>> {
        self.raster.as_mut()?.take()
    }
    pub fn wait_painted(&mut self) -> Result<crate::Painted, String> {
        self.raster
            .as_mut()
            .ok_or("Nested frames do not rasterise")?
            .wait()
    }
    pub fn paint(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        zoom: f32,
        dark: bool,
    ) -> Result<Frame, String> {
        if self.paint_pending() {
            let _ = self.wait_painted();
        }
        self.begin_paint(
            ViewportConfig {
                width_px: width,
                height_px: height,
                scale,
                zoom,
                dark,
            },
            0,
            FrameTarget::Owned,
        )?;
        let result = self.wait_painted()?;
        if let crate::Pixels::Owned(rgba) = result.pixels {
            Ok(Frame {
                width,
                height,
                rgba,
            })
        } else {
            Err("Unexpected shared raster result".into())
        }
    }
    /// Capture a screenshot of the rendered view as RGBA pixels.
    pub fn screenshot(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        zoom: f32,
        rect: Option<(u32, u32, u32, u32)>,
    ) -> Result<Vec<u8>, String> {
        let frame = self.paint(width, height, scale, zoom, false)?;
        if let Some((rx, ry, rw, rh)) = rect {
            let rx = rx.min(width);
            let ry = ry.min(height);
            let rw = rw.min(width - rx);
            let rh = rh.min(height - ry);
            let mut sub = Vec::with_capacity((rw * rh * 4) as usize);
            for y in ry..(ry + rh) {
                let start = ((y * width + rx) * 4) as usize;
                let end = start + (rw * 4) as usize;
                if end <= frame.rgba.len() {
                    sub.extend_from_slice(&frame.rgba[start..end]);
                }
            }
            Ok(sub)
        } else {
            Ok(frame.rgba)
        }
    }
    pub fn text(&self, selector: &str) -> Option<String> {
        let doc = self.document.borrow();
        doc.query_selector(selector)
            .ok()
            .flatten()
            .and_then(|id| wega_dom::content::text_content(&*doc, id))
    }
    pub fn node_id(&self, selector: &str) -> Option<NodeKey> {
        self.document
            .borrow()
            .query_selector(selector)
            .ok()
            .flatten()
    }
}
impl Drop for View {
    fn drop(&mut self) {
        if let Some(abort) = self.abort.take() {
            abort.abort();
        }
    }
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
/// Encode a form submission body for its enctype.
fn encode_body(body: &Body, content_type: Option<&str>) -> (Vec<u8>, String) {
    let entries: Vec<(String, String)> = match body {
        Body::Form(form) => form
            .0
            .iter()
            .map(|entry| (entry.name.clone(), entry.value.as_ref().to_owned()))
            .collect(),
        Body::Bytes(bytes) => {
            return (
                bytes.to_vec(),
                content_type
                    .unwrap_or("application/octet-stream")
                    .to_owned(),
            );
        }
        Body::Empty => Vec::new(),
    };
    match content_type.map(|t| t.to_ascii_lowercase()) {
        Some(kind) if kind.starts_with("multipart/form-data") => {
            let boundary = format!(
                "----WegaFormBoundary{:016x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos() as u64)
            );
            let mut out = Vec::new();
            if let Body::Form(form) = body {
                for entry in &form.0 {
                    let name = entry.name.replace('"', "%22").replace(['\r', '\n'], " ");
                    match &entry.value {
                        wega_network::document::net::EntryValue::String(text) => {
                            out.extend_from_slice(
                                format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{text}\r\n")
                                    .as_bytes(),
                            );
                        }
                        wega_network::document::net::EntryValue::File(path) => {
                            let filename = path
                                .file_name()
                                .map(|f| f.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let data = std::fs::read(path).unwrap_or_default();
                            out.extend_from_slice(
                                format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n")
                                    .as_bytes(),
                            );
                            out.extend_from_slice(&data);
                            out.extend_from_slice(b"\r\n");
                        }
                        wega_network::document::net::EntryValue::EmptyFile => {
                            out.extend_from_slice(
                                format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"\"\r\nContent-Type: application/octet-stream\r\n\r\n\r\n")
                                    .as_bytes(),
                            );
                        }
                    }
                }
            } else {
                for (name, value) in entries {
                    let name = name.replace('"', "%22").replace(['\r', '\n'], " ");
                    out.extend_from_slice(
                        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                            .as_bytes(),
                    );
                }
            }
            out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
            (out, format!("multipart/form-data; boundary={boundary}"))
        }
        Some(kind) if kind.starts_with("text/plain") => {
            let text: String = entries
                .iter()
                .map(|(name, value)| format!("{name}={value}\r\n"))
                .collect();
            (text.into_bytes(), "text/plain;charset=UTF-8".into())
        }
        _ => {
            let mut text = String::new();
            url::form_urlencoded::Serializer::new(&mut text).extend_pairs(entries);
            (
                text.into_bytes(),
                "application/x-www-form-urlencoded".into(),
            )
        }
    }
}

fn clip_text(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

impl crate::inspector::PagePort for View {
    fn inspect_style(
        &self,
        scope: wega_protocol::inspector::styles::StyleScope,
        query: &wega_protocol::inspector::styles::StyleQuery,
    ) -> Result<wega_protocol::inspector::styles::StylePage, wega_protocol::inspector::InspectorError>
    {
        let doc = self.document.borrow();
        let native = self.native_document.as_ref().map(|n| n.borrow());
        let read = crate::document::inspection::DocumentRead {
            doc: &doc,
            native: native.as_deref(),
        };
        crate::inspector::styles::inspect(&read, scope, query)
    }
    fn inspect_dom(
        &self,
        document: wega_protocol::DocumentContext,
        command: wega_protocol::inspector::InspectorCommand,
        args: &wega_protocol::inspector::InspectorArgs,
    ) -> Result<wega_protocol::inspector::dom::DomPage, wega_protocol::inspector::InspectorError>
    {
        crate::inspector::dom::inspect(&*self.document.borrow(), document, command, args)
    }
    fn inspect_geometry(
        &self,
        document: wega_protocol::DocumentContext,
        node: wega_protocol::inspector::dom::InspectorNodeId,
    ) -> Result<
        wega_protocol::inspector::geometry::GeometryReport,
        wega_protocol::inspector::InspectorError,
    > {
        if self.rendering_backend == crate::native_engine::RenderingBackend::WegaNative {
            if let Some(native_doc) = &self.native_document {
                if node.document != document {
                    return Err(wega_protocol::inspector::InspectorError::new(
                        wega_protocol::inspector::InspectorErrorCode::StaleDocument,
                        "Node belongs to a replaced document",
                    ));
                }
                let key = NodeKey::from_u64(node.key);
                let native_doc = native_doc.borrow();
                if native_doc.doc.borrow().node(key).is_none() {
                    return Err(wega_protocol::inspector::InspectorError::new(
                        wega_protocol::inspector::InspectorErrorCode::StaleNode,
                        "Node was dropped",
                    ));
                }
                return Ok(native_doc.inspect_geometry_report(document, key));
            }
        }
        Err(wega_protocol::inspector::InspectorError::new(
            wega_protocol::inspector::InspectorErrorCode::Unavailable,
            "Native layout is unavailable",
        ))
    }
    fn pick_node(
        &self,
        document: wega_protocol::DocumentContext,
        x: f32,
        y: f32,
    ) -> Result<
        wega_protocol::inspector::geometry::PickReport,
        wega_protocol::inspector::InspectorError,
    > {
        if self.rendering_backend == crate::native_engine::RenderingBackend::WegaNative {
            if let Some(native_doc) = &self.native_document {
                let pt = wega_dom::layout::Point {
                    x: x as f64,
                    y: y as f64,
                };
                let native_doc = native_doc.borrow();
                return native_doc.pick_node_report(document, pt);
            }
        }
        Err(wega_protocol::inspector::InspectorError::new(
            wega_protocol::inspector::InspectorErrorCode::Unavailable,
            "Native layout is unavailable",
        ))
    }
    fn apply_edit(
        &mut self,
        document: wega_protocol::DocumentContext,
        node: wega_protocol::inspector::dom::InspectorNodeId,
        epoch: u64,
        op: &wega_protocol::inspector::edit::MutationOp,
        allow_code: bool,
    ) -> Result<
        wega_protocol::inspector::edit::MutationOutcome,
        wega_protocol::inspector::InspectorError,
    > {
        use wega_protocol::inspector::{InspectorError, InspectorErrorCode};
        if self.mutation_epoch() != epoch {
            return Err(InspectorError::new(
                InspectorErrorCode::StaleMutation,
                "The document changed since the edit was prepared",
            ));
        }
        let plan = {
            let doc = self
                .document
                .try_borrow()
                .map_err(|_| InspectorError::new(InspectorErrorCode::Busy, "DOM is busy"))?;
            crate::inspector::mutate::plan(&*doc, document, node, op, allow_code)?
        };
        let dom = self.dom.as_ref().ok_or_else(|| {
            InspectorError::new(
                InspectorErrorCode::Unavailable,
                "The document has no script environment",
            )
        })?;
        let result = dom
            .call(&plan.args)
            .map_err(|message| crate::inspector::mutate::map_failure(&message))?;
        let changed = if plan.reports_change {
            result.as_bool().unwrap_or(true)
        } else {
            true
        };
        self.frame.invalidate();
        Ok(wega_protocol::inspector::edit::MutationOutcome {
            document,
            node,
            epoch: self.mutation_epoch(),
            changed,
        })
    }
    fn status_input(&self, sampled: crate::inspector::Sampled) -> crate::inspector::StatusInput {
        let history = self.document_history();
        crate::inspector::StatusInput {
            url: self.url.to_string(),
            title: self.title(),
            loading: self.loading,
            viewport: sampled.viewport,
            nodes: sampled.nodes,
            js_heap_bytes: sampled.js_heap_bytes,
            can_go_back: history.0,
            can_go_forward: history.1,
            mutation_epoch: self.mutation_epoch(),
            layout_pending: self.layout_pending(),
        }
    }
    fn console_first_seq(&self) -> u64 {
        self.console.borrow().first_seq()
    }
    fn console_next_seq(&self) -> u64 {
        self.console.borrow().next_seq()
    }
    fn console_len(&self) -> usize {
        self.console.borrow().len()
    }
    fn console_dropped(&self) -> u64 {
        self.console.borrow().dropped()
    }
    fn console_read(
        &self,
        after: u64,
        max_entries: usize,
        max_bytes: usize,
    ) -> Vec<wega_protocol::inspector::ConsoleMessage> {
        self.console
            .borrow()
            .read_after(after, max_entries, max_bytes)
    }
    fn console_clear(&mut self) {
        self.console.borrow_mut().clear();
    }
    fn document_origin(&self) -> Option<String> {
        // An opaque-origin frame has no tuple origin whatever its URL looks like: it can only be
        // authorised through the explicit opaque override.
        (matches!(self.url.scheme(), "http" | "https") && !self.restrictions().opaque)
            .then(|| self.url.origin().ascii_serialization())
    }
    fn evaluate(
        &mut self,
        expression: &str,
    ) -> Result<wega_protocol::inspector::EvalOutcome, String> {
        use wega_adapter_v8::InspectOutcome;
        use wega_protocol::inspector::{
            EvalOutcome, ExceptionDetails, ExceptionKind, Preview, PreviewKind, SourceLocation,
        };
        if !self.scheduler.script_allowed() {
            return Err("The document is suspended for navigation".into());
        }
        let script = self
            .script
            .as_mut()
            .ok_or("The page has no script context")?;
        let result = script.evaluate_inspect(expression);
        let result = crate::script::finish_task(script.as_mut(), result);
        self.frame.invalidate();
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                let fatal = error.fatal;
                let message = error.message.clone();
                if fatal {
                    self.script_error(error);
                    return Err(format!(
                        "The page's script runtime was terminated and the tab stopped: {}",
                        clip_text(&message, 512)
                    ));
                }
                return Err(clip_text(&message, 512));
            }
        };
        self.drain_requests();
        Ok(match outcome {
            InspectOutcome::Value { preview_json } => {
                let preview = serde_json::from_str::<Preview>(&preview_json)
                    .ok()
                    .filter(Preview::is_bounded)
                    .unwrap_or_else(|| {
                        Preview::simple(PreviewKind::Unavailable, "preview unavailable")
                    });
                EvalOutcome::Value { preview }
            }
            InspectOutcome::Exception(error) => EvalOutcome::Exception {
                details: ExceptionDetails {
                    kind: ExceptionKind::Evaluation,
                    name: clip_text(&error.name, 128),
                    message: clip_text(&error.message, 2048),
                    stack: error.stack.map(|stack| clip_text(&stack, 4096)),
                    source: error.url.map(|url| SourceLocation {
                        url: clip_text(&url, 4096),
                        line: error.line,
                        column: error.column,
                    }),
                },
            },
        })
    }
}

impl View {
    fn native_pointer_input(&mut self, pointer: Pointer) {
        if self.frozen || !self.scheduler.script_allowed() {
            return;
        }
        let Some(native) = self.native_document.clone() else {
            return;
        };
        let hovered = if pointer.kind == PointerKind::Leave {
            None
        } else {
            let mut native = native.borrow_mut();
            native.ensure_layout();
            native.element_at(Point {
                x: pointer.x as f64,
                y: pointer.y as f64,
            })
        };
        let previous = self.native_hover;
        self.native_hover = hovered;
        self.document.borrow_mut().set_hover(hovered);
        if previous != hovered {
            self.hover_changed(previous, hovered, &pointer);
        }
        let target = hovered.or_else(|| self.document.borrow().keyboard_target());
        let Some(target) = target else {
            return;
        };
        match pointer.kind {
            PointerKind::Leave => {}
            PointerKind::Move => {
                if !self.dispatch_pointer("pointermove", target, &pointer, None) {
                    self.dispatch_pointer("mousemove", target, &pointer, None);
                }
                if let Some(down) = self.native_down {
                    if let Some(ref native_doc) = self.native_document {
                        if let Some(b) = native_doc.borrow().find_box(down) {
                            let cb = b.content_box();
                            let rel_x = (pointer.x - cb.x as f32).max(0.0);
                            let rel_y = (pointer.y - cb.y as f32).max(0.0);
                            let mut doc = self.document.borrow_mut();
                            doc.with_editor(down, |editor, fonts| {
                                editor
                                    .driver(fonts, &mut parley::LayoutContext::new())
                                    .extend_selection_to_point(rel_x, rel_y);
                            });
                        }
                    }
                }
            }
            PointerKind::Down => {
                self.activate();
                self.native_down = hovered;
                self.click_count = if self.last_down.is_some_and(|(time, x, y)| {
                    time.elapsed() < Duration::from_millis(500)
                        && (x - pointer.x).abs() <= 2.0
                        && (y - pointer.y).abs() <= 2.0
                }) {
                    self.click_count.saturating_add(1)
                } else {
                    1
                };
                self.last_down = Some((Instant::now(), pointer.x, pointer.y));
                let cancelled = self.dispatch_pointer("pointerdown", target, &pointer, None)
                    || self.dispatch_pointer("mousedown", target, &pointer, None);
                if !cancelled {
                    self.document.borrow_mut().set_active(Some(target));
                    let focusable = self.document.borrow().node(target).is_some_and(|n| {
                        n.element_name().is_some_and(|e| {
                            matches!(e.local, "input" | "button" | "select" | "textarea" | "a")
                        }) || n.attr("tabindex").is_some()
                    });
                    if focusable {
                        let _ = self.document.borrow_mut().set_focus(target);
                    } else {
                        self.document.borrow_mut().clear_focus();
                    }
                    if let Some(ref native_doc) = self.native_document {
                        if let Some(b) = native_doc.borrow().find_box(target) {
                            let cb = b.content_box();
                            let rel_x = (pointer.x - cb.x as f32).max(0.0);
                            let rel_y = (pointer.y - cb.y as f32).max(0.0);
                            let mut doc = self.document.borrow_mut();
                            doc.with_editor(target, |editor, fonts| {
                                editor
                                    .driver(fonts, &mut parley::LayoutContext::new())
                                    .move_to_point(rel_x, rel_y);
                            });
                        }
                    }
                }
            }
            PointerKind::Up => {
                self.activate();
                self.dispatch_pointer("pointerup", target, &pointer, None);
                self.dispatch_pointer("mouseup", target, &pointer, None);
                self.document.borrow_mut().set_active(None);
                let down = self.native_down.take();
                if down == hovered && hovered.is_some() && pointer.button == 0 {
                    self.click_event(target, Some(&pointer));
                    if self.click_count == 2 {
                        self.dispatch_pointer("dblclick", target, &pointer, None);
                    }
                } else if down == hovered && hovered.is_some() && pointer.button == 2 {
                    self.dispatch_pointer("contextmenu", target, &pointer, None);
                }
            }
            PointerKind::Wheel => {
                if !self.dispatch_pointer("wheel", target, &pointer, None) {
                    native
                        .borrow_mut()
                        .scroll_by_at(target, pointer.delta_x, pointer.delta_y);
                    self.run("window.dispatchEvent(new Event('scroll'))", "wega:scroll");
                }
            }
        }
        self.frame.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn script_types_follow_the_html_mime_list() {
        assert_eq!(script_kind("", None), Some(false));
        assert_eq!(script_kind("module", None), Some(true));
        assert_eq!(script_kind(" Text/JavaScript ", None), Some(false));
        assert_eq!(script_kind("application/x-javascript", None), Some(false));
        assert_eq!(script_kind("application/json", None), None);
        assert_eq!(script_kind("text/template", None), None);
        assert_eq!(script_kind("", Some("JavaScript")), Some(false));
        assert_eq!(script_kind("", Some("vbscript")), None);
    }
    #[test]
    fn form_bodies_are_encoded_for_their_enctype() {
        use wega_network::document::net::{Entry, EntryValue, FormData};
        let form = Body::Form(FormData(vec![
            Entry {
                name: "q".into(),
                value: EntryValue::String("a b&c".into()),
            },
            Entry {
                name: "n".into(),
                value: EntryValue::String("1".into()),
            },
        ]));
        let (body, kind) = encode_body(&form, Some("application/x-www-form-urlencoded"));
        assert_eq!(
            (String::from_utf8(body).unwrap().as_str(), kind.as_str()),
            ("q=a+b%26c&n=1", "application/x-www-form-urlencoded")
        );
        let (body, kind) = encode_body(&form, Some("multipart/form-data"));
        let boundary = kind.split("boundary=").nth(1).unwrap();
        let text = String::from_utf8(body).unwrap();
        assert!(text.starts_with(&format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"q\"\r\n\r\na b&c\r\n"
        )));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    }
    #[test]
    fn form_bodies_support_file_entries_multipart() {
        use wega_network::document::net::{Entry, EntryValue, FormData};
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("upload.txt");
        std::fs::write(&file_path, b"Hello file content!").unwrap();

        let form = Body::Form(FormData(vec![
            Entry {
                name: "file".into(),
                value: EntryValue::File(file_path),
            },
            Entry {
                name: "empty".into(),
                value: EntryValue::EmptyFile,
            },
        ]));
        let (body, kind) = encode_body(&form, Some("multipart/form-data"));
        let boundary = kind.split("boundary=").nth(1).unwrap();
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"upload.txt\"\r\nContent-Type: application/octet-stream\r\n\r\nHello file content!\r\n")));
        assert!(text.contains(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"empty\"; filename=\"\"\r\nContent-Type: application/octet-stream\r\n\r\n\r\n")));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    }
    #[test]
    fn accessibility_tree_generates_valid_json() {
        let limits = Limits::default();
        let network = Network::new(Default::default(), Arc::new(|| {})).unwrap();
        let mut view = View::new(
            Url::parse("data:text/html,<button>Click me</button>").unwrap(),
            network,
            Default::default(),
            limits,
        )
        .unwrap();
        view.set_accessibility_enabled(true);
        let tree_json = view.accessibility_tree();
        assert!(tree_json.is_some());
        let json_str = tree_json.unwrap();
        assert!(json_str.contains("nodes"));
        let update: accesskit::TreeUpdate = serde_json::from_str(&json_str).unwrap();
        assert!(!update.nodes.is_empty());
    }
    #[test]
    fn file_dialog_canceling_leaks_no_grant() {
        let limits = Limits::default();
        let network = Network::new(Default::default(), Arc::new(|| {})).unwrap();
        let mut view = View::new(
            Url::parse("data:text/html,<input type='file' id='f'>").unwrap(),
            network,
            Default::default(),
            limits,
        )
        .unwrap();
        view.pending_file_dialog = Some((42, NodeKey::from_u64(1)));
        view.file_selected(42, vec![]);
        assert!(view.selected_files.is_empty());
        assert!(view.pending_file_dialog.is_none());
    }
    #[test]
    fn range_and_selection_dom_apis() {
        let limits = Limits::default();
        let network = Network::new(Default::default(), Arc::new(|| {})).unwrap();
        let mut view = View::new(
            Url::parse("about:blank").unwrap(),
            network,
            Default::default(),
            limits,
        )
        .unwrap();
        view.set_html("<!doctype html><div id='test'>Hello <b>beautiful</b> world</div>")
            .unwrap();
        let js = r#"
            const div = document.getElementById('test');
            const range = document.createRange();
            range.selectNodeContents(div);
            const sel = window.getSelection();
            sel.removeAllRanges();
            sel.addRange(range);
            const text = sel.toString();
            const count = sel.rangeCount;
            const collapsed = sel.isCollapsed;
            ({ text, count, collapsed });
        "#;
        let res = view.evaluate_json(js).unwrap();
        let val: serde_json::Value = serde_json::from_str(&res).unwrap();
        assert_eq!(val["count"], 1);
        assert_eq!(val["collapsed"], false);
        assert!(val["text"].as_str().unwrap().contains("Hello"));
    }
    #[test]
    fn content_editable_and_exec_command() {
        let limits = Limits::default();
        let network = Network::new(Default::default(), Arc::new(|| {})).unwrap();
        let mut view = View::new(
            Url::parse("about:blank").unwrap(),
            network,
            Default::default(),
            limits,
        )
        .unwrap();
        view.set_html("<!doctype html><div id='ed' contenteditable='true'>Initial text</div>")
            .unwrap();
        let js = r#"
            const el = document.getElementById('ed');
            el.focus();
            const ce = el.contentEditable;
            const isCe = el.isContentEditable;
            const supported = document.queryCommandSupported('insertText');
            document.execCommand('selectAll');
            document.execCommand('insertText', false, 'Replacement text');
            ({ ce, isCe, supported, text: el.textContent });
        "#;
        let res = view.evaluate_json(js).unwrap();
        let val: serde_json::Value = serde_json::from_str(&res).unwrap();
        assert_eq!(val["ce"], "true");
        assert_eq!(val["isCe"], true);
        assert_eq!(val["supported"], true);
        assert_eq!(val["text"], "Replacement text");
    }
    #[test]
    fn find_in_page_and_tree_walker() {
        let limits = Limits::default();
        let network = Network::new(Default::default(), Arc::new(|| {})).unwrap();
        let mut view = View::new(
            Url::parse("about:blank").unwrap(),
            network,
            Default::default(),
            limits,
        )
        .unwrap();
        view.set_html(
            "<!doctype html><p>The quick brown fox jumps over the lazy dog. The fox was quick.</p>",
        )
        .unwrap();
        let (active1, total1) = view.find_in_page("fox", false, false);
        assert_eq!(active1, 1);
        assert_eq!(total1, 2);

        let (active2, total2) = view.find_in_page("fox", false, false);
        assert_eq!(active2, 2);
        assert_eq!(total2, 2);

        // Check TreeWalker directly
        let js = r#"
            const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
            const textNodes = [];
            let n;
            while (n = walker.nextNode()) textNodes.push(n.data);
            ({ count: textNodes.length, sample: textNodes[0] || '' });
        "#;
        let res = view.evaluate_json(js).unwrap();
        let val: serde_json::Value = serde_json::from_str(&res).unwrap();
        assert!(val["count"].as_u64().unwrap() >= 1);
        assert!(val["sample"].as_str().unwrap().contains("quick"));
    }
    #[test]
    fn clipboard_and_datatransfer_apis() {
        let limits = Limits::default();
        let network = Network::new(Default::default(), Arc::new(|| {})).unwrap();
        let mut view = View::new(
            Url::parse("about:blank").unwrap(),
            network,
            Default::default(),
            limits,
        )
        .unwrap();
        view.set_html("<!doctype html><div>Test</div>").unwrap();
        let js = r#"
            navigator.clipboard.writeText('clipboard test payload');
            const dt = new DataTransfer();
            dt.setData('text/plain', 'drag payload');
            const dtRead = dt.getData('text/plain');
            const evt = new DragEvent('dragstart', { dataTransfer: dt });
            ({ dtRead, evtType: evt.type });
        "#;
        let res = view.evaluate_json(js).unwrap();
        let val: serde_json::Value = serde_json::from_str(&res).unwrap();
        assert_eq!(val["dtRead"], "drag payload");
        assert_eq!(val["evtType"], "dragstart");
    }
    fn blank_view(html: &str) -> View {
        let network = Network::new(Default::default(), Arc::new(|| {})).unwrap();
        let mut view = View::new(
            Url::parse("about:blank").unwrap(),
            network,
            Default::default(),
            Limits::default(),
        )
        .unwrap();
        view.set_html(html).unwrap();
        view
    }
    fn settle(view: &mut View, expression: &str) -> serde_json::Value {
        for _ in 0..200 {
            view.poll();
            let value: serde_json::Value =
                serde_json::from_str(&view.evaluate_json(expression).unwrap()).unwrap();
            if !value.is_null() {
                return value;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("{expression} did not settle");
    }
    #[test]
    fn clipboard_write_text_requires_activation_and_reaches_the_browser() {
        let mut view = blank_view("<!doctype html><div>Test</div>");
        let write = |tag: &str| {
            format!(
                "globalThis.{tag} = null; navigator.clipboard.writeText('os payload')\
                 .then(() => {{ globalThis.{tag} = 'ok'; }}, e => {{ globalThis.{tag} = e.name; }}); true"
            )
        };
        view.evaluate_json(&write("denied")).unwrap();
        assert_eq!(settle(&mut view, "globalThis.denied"), "NotAllowedError");
        assert!(
            !view
                .events
                .iter()
                .any(|event| matches!(event, PageEvent::ClipboardWrite(_)))
        );

        view.activate();
        view.evaluate_json(&write("allowed")).unwrap();
        assert_eq!(settle(&mut view, "globalThis.allowed"), "ok");
        assert!(
            view.events.iter().any(
                |event| matches!(event, PageEvent::ClipboardWrite(text) if text == "os payload")
            )
        );
    }
    #[test]
    fn html_elements_use_per_tag_interfaces() {
        let mut view =
            blank_view("<!doctype html><div id=d><p>x</p><table><tr><td>y</td></tr></table></div>");
        let js = r#"
            const name = tag => Object.getPrototypeOf(document.createElement(tag)).constructor.name;
            ({
                div: document.getElementById('d') instanceof HTMLDivElement,
                divIsHtml: document.getElementById('d') instanceof HTMLElement,
                p: document.querySelector('p') instanceof HTMLParagraphElement,
                td: document.querySelector('td') instanceof HTMLTableCellElement,
                head: document.head instanceof HTMLHeadElement,
                body: document.body instanceof HTMLBodyElement,
                h2: name('h2'),
                span: name('span'),
                input: name('input'),
                custom: name('x-unknown'),
                notDiv: document.createElement('span') instanceof HTMLDivElement,
                illegal: (() => { try { new HTMLDivElement(); return false; } catch { return true; } })(),
            })
        "#;
        let val: serde_json::Value =
            serde_json::from_str(&view.evaluate_json(js).unwrap()).unwrap();
        for key in ["div", "divIsHtml", "p", "td", "head", "body", "illegal"] {
            assert_eq!(val[key], true, "{key}");
        }
        assert_eq!(val["h2"], "HTMLHeadingElement");
        assert_eq!(val["span"], "HTMLSpanElement");
        assert_eq!(val["input"], "HTMLInputElement");
        assert_eq!(val["custom"], "HTMLElement");
        assert_eq!(val["notDiv"], false);
    }
    #[test]
    fn media_devices_and_missing_media_are_reported_truthfully() {
        let mut view = blank_view("<!doctype html><div>Test</div>");
        view.evaluate_json(
            "globalThis.devices = null; navigator.mediaDevices.enumerateDevices()\
             .then(list => { globalThis.devices = list.length; }); true",
        )
        .unwrap();
        assert_eq!(settle(&mut view, "globalThis.devices"), 0);

        let js = r#"
            const results = {};
            for (const src of ["https://media.invalid/missing-video.webm", "https://media.invalid/missing-audio.wav"]) {
                const el = document.createElement(src.endsWith("wav") ? "audio" : "video");
                let failed = false, loaded = false;
                el.addEventListener("error", () => { failed = true; });
                el.addEventListener("loadedmetadata", () => { loaded = true; });
                el.src = src;
                results[el.localName] = { failed, loaded, code: el.error ? el.error.code : 0 };
            }
            results
        "#;
        let val: serde_json::Value =
            serde_json::from_str(&view.evaluate_json(js).unwrap()).unwrap();
        for kind in ["video", "audio"] {
            assert_eq!(val[kind]["failed"], true, "{kind}");
            assert_eq!(val[kind]["loaded"], false, "{kind}");
            assert_eq!(val[kind]["code"], 4, "{kind}");
        }
    }
}
