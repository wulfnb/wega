use crate::{
    Limits,
    csp::Csp,
    encoding::{self, DocumentKind},
    network::{Destination, FetchContext, Network, Response},
    parser::{Parser, Step},
    script::{HostContext, ScriptHost},
};
use blitz_dom::{BaseDocument, DocumentConfig, NodeId, traversal::TreeTraverser};
use blitz_html::HtmlDocument;
use blitz_traits::{
    events::{
        BlitzPointerEvent, DomEvent, DomEventData, MouseEventButton, MouseEventButtons,
        PointerCoords,
    },
    navigation::{NavigationOptions, NavigationProvider},
    net::{AbortController, AbortSignal, Body, Bytes, NetHandler, NetProvider, Request},
    shell::{ColorScheme, Viewport},
};
use encoding_rs::Encoding;
use html5ever::LocalName;
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
    fn modifiers(&self) -> Modifiers {
        let mut flags = Modifiers::empty();
        for (bit, flag) in [
            (1, Modifiers::SHIFT),
            (2, Modifiers::CONTROL),
            (4, Modifiers::ALT),
            (8, Modifiers::META),
        ] {
            if self.modifiers & bit != 0 {
                flags.insert(flag);
            }
        }
        flags
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
        // HTTP GET subresources only; no access to local files from web pages.
        let blocked = request.method != blitz_traits::net::http::Method::GET
            || (matches!(request.url.scheme(), "http" | "https")
                && !self
                    .csp
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .allows_subresource(&request.url, &self.document));
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
        let mut context = FetchContext::subresource(&self.document, destination(&request.url));
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

enum ScriptState {
    Ready(String),
    Fetching(mpsc::Receiver<FetchResult>),
    Failed(String),
}
/// A script waiting to run: parser-blocking, deferred, ordered or async.
struct PendingScript {
    node: Option<NodeId>,
    module: bool,
    name: String,
    external: bool,
    state: ScriptState,
}
impl PendingScript {
    fn ready(&mut self, fallback: &'static Encoding) -> bool {
        let ScriptState::Fetching(receiver) = &self.state else {
            return true;
        };
        match receiver.try_recv() {
            Ok(Ok(response)) => {
                self.name = response.url.to_string();
                self.state = ScriptState::Ready(encoding::decode_text(
                    &response.bytes,
                    response.header("content-type"),
                    fallback,
                ));
                true
            }
            Ok(Err(error)) => {
                self.state = ScriptState::Failed(format!("{}: {error}", self.name));
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.state = ScriptState::Failed(format!("{}: fetch was cancelled", self.name));
                true
            }
        }
    }
}

/// Script attributes read from the DOM.
struct ScriptElement {
    kind: Option<bool>,
    src: Option<String>,
    is_async: bool,
    defer: bool,
    nomodule: bool,
    nonce: String,
    text: String,
}
/// `Some(true)` for module scripts, `Some(false)` for classic, `None` for data blocks.
fn script_kind(kind: &str, language: Option<&str>) -> Option<bool> {
    let kind = kind.trim().to_ascii_lowercase();
    if kind == "module" {
        return Some(true);
    }
    let classic = [
        "application/ecmascript",
        "application/javascript",
        "application/x-ecmascript",
        "application/x-javascript",
        "text/ecmascript",
        "text/javascript",
        "text/javascript1.0",
        "text/javascript1.1",
        "text/javascript1.2",
        "text/javascript1.3",
        "text/javascript1.4",
        "text/javascript1.5",
        "text/jscript",
        "text/livescript",
        "text/x-ecmascript",
        "text/x-javascript",
    ];
    if kind.is_empty() {
        return match language.map(|l| l.trim().to_ascii_lowercase()) {
            None => Some(false),
            Some(language) if language.is_empty() => Some(false),
            Some(language) => classic
                .contains(&format!("text/{language}").as_str())
                .then_some(false),
        };
    }
    classic.contains(&kind.as_str()).then_some(false)
}

const MAX_SCRIPTS: usize = 1024;

pub struct View {
    document: Rc<RefCell<BaseDocument>>,
    document_revision: u64,
    navigation_generation: u64,
    document_generation: Rc<Cell<u64>>,
    script: Option<Box<dyn ScriptHost>>,
    network: Network,
    limits: Limits,
    abort: Option<AbortController>,
    pending: Option<mpsc::Receiver<FetchResult>>,
    parser: Option<Parser>,
    blocking: Option<PendingScript>,
    deferred: VecDeque<PendingScript>,
    ordered: VecDeque<PendingScript>,
    asynchronous: Vec<PendingScript>,
    started_scripts: Rc<RefCell<HashSet<NodeId>>>,
    script_count: usize,
    applied_meta: HashSet<NodeId>,
    encoding: &'static Encoding,
    referrer_policy: Option<String>,
    csp: Arc<RwLock<Csp>>,
    dialog: Rc<RefCell<Option<crate::dom::DocumentDialogHandler>>>,
    activation: Rc<Cell<Option<Instant>>>,
    navigate: Rc<RefCell<Vec<Url>>>,
    navigation_rx: mpsc::Receiver<NavigationOptions>,
    revision: Arc<AtomicU64>,
    last_revision: u64,
    raster: crate::raster::Raster,
    pub compatibility: Option<String>,
    in_flight: Arc<AtomicUsize>,
    completed: Arc<Mutex<Vec<(String, bool)>>>,
    started: Instant,
    dirty: bool,
    reported_frames: bool,
    frozen: bool,
    ready_event_sent: bool,
    load_event_sent: bool,
    ready_at: Option<Instant>,
    refresh: Option<(Instant, Url)>,
    composing: bool,
    showing_error: bool,
    cursor: String,
    pub url: Url,
    pub loading: bool,
    pub notices: VecDeque<String>,
    pub events: VecDeque<PageEvent>,
}
impl View {
    pub(crate) fn new(url: Url, network: Network, limits: Limits) -> Result<Self, String> {
        let document = HtmlDocument::from_html(
            "<!doctype html><title>Wega</title>",
            DocumentConfig::default(),
        )
        .into_inner();
        let (_tx, navigation_rx) = mpsc::channel();
        let raster = crate::raster::Raster::new(network.wake.clone())?;
        let csp = Arc::new(RwLock::new(Csp::default()));
        let mut view = Self {
            document: Rc::new(RefCell::new(document)),
            document_revision: 0,
            navigation_generation: 0,
            document_generation: Rc::new(Cell::new(0)),
            script: None,
            network,
            limits,
            abort: None,
            pending: None,
            parser: None,
            blocking: None,
            deferred: VecDeque::new(),
            ordered: VecDeque::new(),
            asynchronous: Vec::new(),
            started_scripts: Default::default(),
            script_count: 0,
            applied_meta: HashSet::new(),
            encoding: encoding_rs::UTF_8,
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
            in_flight: Arc::new(AtomicUsize::new(0)),
            completed: Arc::new(Mutex::new(Vec::new())),
            started: Instant::now(),
            dirty: true,
            reported_frames: false,
            frozen: false,
            ready_event_sent: false,
            load_event_sent: false,
            ready_at: None,
            refresh: None,
            composing: false,
            showing_error: false,
            cursor: String::new(),
            url: url.clone(),
            loading: false,
            notices: VecDeque::new(),
            events: VecDeque::new(),
        };
        view.load(url)?;
        Ok(view)
    }
    /// Answer `alert`, `confirm` and `prompt`; without a handler they return
    /// their default (no) answer immediately.
    pub fn set_dialog_handler(&mut self, mut handler: crate::dom::DialogHandler) {
        self.set_document_dialog_handler(Box::new(move |_, _, _, kind, message, default| {
            handler(kind, message, default)
        }));
    }
    pub fn set_document_dialog_handler(&mut self, handler: crate::dom::DocumentDialogHandler) {
        *self.dialog.borrow_mut() = Some(handler);
    }
    fn local_base(&self) -> Option<std::path::PathBuf> {
        if self.url.scheme() == "file" {
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
        self.events.clear();
        self.navigate.borrow_mut().clear();
        self.navigation_generation = generation;
        self.load(url)
    }
    pub fn stop_for_navigation(&mut self, generation: u64) {
        self.navigation_generation = generation;
        self.stop();
    }
    pub fn load(&mut self, url: Url) -> Result<(), String> {
        if self.same_document(&url) {
            self.document_generation.set(self.navigation_generation);
            self.navigate_fragment(&url);
            return Ok(());
        }
        let context = FetchContext {
            destination: Destination::Document,
            ..Default::default()
        };
        self.load_request(url, context)
    }
    fn load_request(&mut self, url: Url, context: FetchContext) -> Result<(), String> {
        self.stop_pending();
        self.url = url.clone();
        self.loading = true;
        self.compatibility = None;
        self.ready_event_sent = false;
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
        self.document_generation.set(self.navigation_generation);
        self.url = self.document.borrow().url().clone();
    }
    fn stop_pending(&mut self) {
        if let Some(abort) = self.abort.take() {
            abort.abort();
        }
        self.pending = None;
        self.parser = None;
        self.blocking = None;
        self.deferred.clear();
        self.ordered.clear();
        self.asynchronous.clear();
        self.loading = false;
    }
    /// Replace the document with `html` (UTF-8, no response headers).
    pub fn set_html(&mut self, html: &str) -> Result<(), String> {
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
        let mut csp = Csp::default();
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
        let config = DocumentConfig {
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
            })),
            navigation_provider: Some(Arc::new(Navigation(sender, self.network.wake.clone()))),
            ..Default::default()
        };
        self.script = None;
        self.parser = None;
        self.blocking = None;
        self.deferred.clear();
        self.ordered.clear();
        self.asynchronous.clear();
        self.started_scripts = Default::default();
        self.applied_meta.clear();
        self.script_count = 0;
        self.encoding = encoding;
        let document = if xml {
            HtmlDocument::from_xml(html, config).into_inner()
        } else {
            blitz_dom::BaseDocument::new(config)
        };
        self.document = Rc::new(RefCell::new(document));
        self.document_revision = document_revision;
        self.document_generation = Rc::new(Cell::new(self.navigation_generation));
        self.events.clear();
        self.navigate.borrow_mut().clear();
        self.refresh = response
            .and_then(|r| r.header("refresh"))
            .and_then(|value| self.parse_refresh(value));
        self.abort = Some(abort);
        let mut host = crate::script::create(
            HostContext {
                document: self.document.clone(),
                navigate: self.navigate.clone(),
                network: self.network.clone(),
                csp: self.csp.clone(),
                dialog: self.dialog.clone(),
                document_generation: self.document_generation.clone(),
                document_revision: self.document_revision,
                activation: self.activation.clone(),
                started_scripts: self.started_scripts.clone(),
            },
            &self.limits,
            html.len(),
        )?;
        host.allow_eval(self.csp().allows_eval());
        host.run(
            include_str!("../../../src/page_state.js"),
            "wega:page-state",
        )
        .map_err(|e| e.to_string())?;
        self.script = Some(host);
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
            self.parser = Some(Parser::new(
                self.document.clone(),
                html,
                self.limits.dom_nodes,
            ));
        }
        self.dirty = true;
        self.ready_event_sent = false;
        self.load_event_sent = false;
        self.ready_at = None;
        self.composing = false;
        self.loading = true;
        // Parse (and run inline scripts) as far as possible before returning.
        self.advance();
        Ok(())
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
        self.document.borrow_mut().handle_messages();
        let revision = self.revision.load(Ordering::Relaxed);
        if revision != self.last_revision {
            self.last_revision = revision;
            self.dirty = true;
        }
        self.advance();
        self.lifecycle();
        if let Some(script) = &mut self.script {
            let due = script.next_timer().is_some_and(|t| t.is_zero());
            if due {
                let result = script.tick();
                if let Err(error) = result {
                    self.script_error(error);
                }
                self.dirty = true;
                self.drain_requests();
            }
        }
        self.resource_events();
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
        if self.pending.is_none() {
            self.url = self.document.borrow().url().clone();
        }
        if let Some((due, url)) = self.refresh.clone()
            && Instant::now() >= due
            && self.load_event_sent
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
        self.url = response.url.clone();
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
    /// Run the parser and the script queues as far as they can go without waiting.
    fn advance(&mut self) {
        loop {
            if self.script.is_none() {
                // A fatal script error ended scripting; finish parsing without scripts.
                self.blocking = None;
            }
            if let Some(blocking) = &mut self.blocking {
                if !blocking.ready(self.encoding) {
                    break;
                }
                let script = self.blocking.take().unwrap();
                self.execute(script);
                continue;
            }
            let Some(parser) = &mut self.parser else {
                break;
            };
            if parser.is_finished() {
                break;
            }
            match parser.step() {
                Step::Script(node) => {
                    self.apply_meta();
                    self.prepare_script(node, true, false);
                }
                Step::Finished => {
                    self.apply_meta();
                    self.parsed();
                }
                Step::Overflow => {
                    if self.showing_error {
                        // Even the error page exceeds the budget; show what parsed.
                        self.parser = None;
                        break;
                    }
                    self.load_error("DOM node limit exceeded; use Servo for this page".into());
                    continue;
                }
            }
            self.drain_requests();
        }
        self.run_queues();
    }
    /// After parsing: enforce the node budget and read meta refresh.
    fn parsed(&mut self) {
        let count = self.node_count();
        if count > self.limits.dom_nodes {
            self.load_error("DOM node limit exceeded; use Servo for this page".into());
            return;
        }
        if let Some(script) = &mut self.script {
            script.sync_nodes(count);
        }
        if self.refresh.is_none() {
            let value = {
                let doc = self.document.borrow();
                doc.query_selector_all("meta[http-equiv]")
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter_map(|id| doc.get_node(id).and_then(|n| n.element_data()))
                    .find(|e| {
                        e.attr(LocalName::from("http-equiv"))
                            .is_some_and(|v| v.eq_ignore_ascii_case("refresh"))
                    })
                    .and_then(|e| e.attr(LocalName::from("content")).map(str::to_owned))
            };
            if let Some(value) = value {
                self.refresh = self.parse_refresh(&value);
            }
        }
    }
    /// Apply `<meta http-equiv=content-security-policy>` seen so far.
    fn apply_meta(&mut self) {
        let found: Vec<(NodeId, String)> = {
            let doc = self.document.borrow();
            doc.query_selector_all("meta[http-equiv]")
                .ok()
                .into_iter()
                .flatten()
                .filter(|id| !self.applied_meta.contains(id))
                .filter_map(|id| {
                    let element = doc.get_node(id)?.element_data()?;
                    element
                        .attr(LocalName::from("http-equiv"))
                        .is_some_and(|v| v.eq_ignore_ascii_case("content-security-policy"))
                        .then(|| {
                            element
                                .attr(LocalName::from("content"))
                                .map(|c| (id, c.to_owned()))
                        })
                        .flatten()
                })
                .collect()
        };
        if found.is_empty() {
            return;
        }
        let mut csp = self
            .csp
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (id, value) in found {
            self.applied_meta.insert(id);
            csp.add(&value);
        }
        let eval = csp.allows_eval();
        drop(csp);
        if let Some(script) = &mut self.script {
            script.allow_eval(eval);
        }
    }
    fn read_script(&self, node: NodeId) -> Option<ScriptElement> {
        let doc = self.document.borrow();
        let element = doc.get_node(node)?.element_data()?;
        let attr = |name: &str| element.attr(LocalName::from(name)).map(str::to_owned);
        Some(ScriptElement {
            kind: script_kind(
                &attr("type").unwrap_or_default(),
                attr("language").as_deref(),
            ),
            src: attr("src"),
            is_async: attr("async").is_some(),
            defer: attr("defer").is_some(),
            nomodule: attr("nomodule").is_some(),
            nonce: attr("nonce").unwrap_or_default(),
            text: doc.get_node(node)?.text_content(),
        })
    }
    /// Prepare a script element: run it now, fetch it, or queue it.
    /// `parser`: inserted by the parser (else by script). `in_order`: queue
    /// even blocking scripts in document order (XML documents).
    fn prepare_script(&mut self, node: NodeId, parser: bool, in_order: bool) {
        if !self.started_scripts.borrow_mut().insert(node) {
            return;
        }
        let Some(element) = self.read_script(node) else {
            return;
        };
        let Some(module) = element.kind else { return };
        if element.nomodule && !module {
            return;
        }
        if self.script_count >= MAX_SCRIPTS {
            self.notice(format!(
                "Script limit of {MAX_SCRIPTS} reached; remaining scripts are skipped"
            ));
            return;
        }
        self.script_count += 1;
        let document = self.document.borrow().url().clone();
        if let Some(src) = element.src {
            let Ok(url) = document.join(src.trim()) else {
                self.script_failed(node, format!("Invalid script address: {src}"));
                return;
            };
            let csp = self.csp();
            let allowed = csp.allows_url("script-src-elem", &url, &document, Some(&element.nonce))
                || (!parser && csp.allows_dynamic_script(&url, &document));
            drop(csp);
            if !allowed {
                self.script_failed(
                    node,
                    format!("Refused to load script {url}: Content Security Policy"),
                );
                return;
            }
            let mut context = FetchContext::subresource(&document, Destination::Script);
            context.referrer_policy = self.referrer_policy.clone();
            let state = match self.fetch(url.clone(), self.limits.resource_bytes, context) {
                Ok(receiver) => ScriptState::Fetching(receiver),
                Err(error) => ScriptState::Failed(error),
            };
            let pending = PendingScript {
                node: Some(node),
                module,
                name: url.to_string(),
                external: true,
                state,
            };
            match (parser, module, element.is_async, element.defer) {
                (_, _, true, _) => self.asynchronous.push(pending),
                (true, true, _, _) | (true, false, false, true) => self.deferred.push_back(pending),
                (true, false, false, false) if in_order => self.deferred.push_back(pending),
                (true, false, false, false) => self.blocking = Some(pending),
                // Script-inserted scripts: async unless script cleared `async`.
                (false, ..) => self.ordered.push_back(pending),
            }
            return;
        }
        if !self
            .csp()
            .allows_inline_script(Some(&element.nonce), &element.text)
        {
            self.notice("Refused to run an inline script: Content Security Policy".into());
            return;
        }
        let name = if module {
            let mut url = document.clone();
            url.set_query(Some(&format!("__wega_inline_module={}", self.script_count)));
            url.to_string()
        } else {
            format!("{document} inline #{}", self.script_count)
        };
        let pending = PendingScript {
            node: Some(node),
            module,
            name,
            external: false,
            state: ScriptState::Ready(element.text),
        };
        if (module && parser && !element.is_async) || in_order {
            self.deferred.push_back(pending);
        } else {
            self.execute(pending);
        }
    }
    fn script_failed(&mut self, node: NodeId, message: String) {
        self.notice(message);
        self.run(
            &format!(
                "__wega_script_done({}, false)",
                serde_json::json!(node.as_u64().to_string())
            ),
            "wega:script-error",
        );
    }
    fn execute(&mut self, script: PendingScript) {
        let node = script
            .node
            .map(|id| serde_json::json!(id.as_u64().to_string()));
        match script.state {
            ScriptState::Ready(source) => {
                if let Some(node) = &node {
                    self.run(
                        &format!("__wega_current_script({node})"),
                        "wega:current-script",
                    );
                }
                self.run_program(&source, &script.name, script.module);
                if let Some(node) = &node {
                    self.run("__wega_current_script(null)", "wega:current-script");
                    if script.external {
                        self.run(
                            &format!("__wega_script_done({node}, true)"),
                            &format!("{} load event", script.name),
                        );
                    }
                }
            }
            ScriptState::Failed(error) => {
                self.notice(error);
                if let Some(node) = &node {
                    self.run(
                        &format!("__wega_script_done({node}, false)"),
                        "wega:script-error",
                    );
                }
            }
            ScriptState::Fetching(_) => unreachable!("only ready scripts execute"),
        }
        self.drain_requests();
    }
    /// Deferred, ordered and async scripts.
    fn run_queues(&mut self) {
        loop {
            let mut progressed = false;
            let parsed =
                self.parser.as_ref().is_none_or(Parser::is_finished) && self.blocking.is_none();
            if parsed
                && let Some(front) = self.deferred.front_mut()
                && front.ready(self.encoding)
            {
                let script = self.deferred.pop_front().unwrap();
                self.execute(script);
                progressed = true;
            }
            if let Some(front) = self.ordered.front_mut()
                && front.ready(self.encoding)
            {
                let script = self.ordered.pop_front().unwrap();
                self.execute(script);
                progressed = true;
            }
            if let Some(index) = self
                .asynchronous
                .iter_mut()
                .position(|script| script.ready(self.encoding))
            {
                let script = self.asynchronous.swap_remove(index);
                self.execute(script);
                progressed = true;
            }
            if !progressed {
                break;
            }
        }
    }
    /// DOMContentLoaded and load, as their own tasks after parsing and scripts.
    fn lifecycle(&mut self) {
        let parsed =
            self.parser.as_ref().is_none_or(Parser::is_finished) && self.blocking.is_none();
        let modules = self
            .script
            .as_ref()
            .is_some_and(|script| script.modules_pending());
        if parsed
            && self.pending.is_none()
            && self.deferred.is_empty()
            && !modules
            && !self.ready_event_sent
        {
            self.ready_event_sent = true;
            self.ready_at = Some(Instant::now());
            self.run("document.readyState='interactive';document.dispatchEvent(new Event('readystatechange'));__wega_dispatch('DOMContentLoaded',document._id)", "wega:ready");
        }
        let settled = self.asynchronous.is_empty()
            && self.ordered.is_empty()
            && self.in_flight.load(Ordering::Relaxed) == 0
            && !modules;
        let expired = self
            .ready_at
            .is_some_and(|at| at.elapsed() > Duration::from_secs(30));
        if self.ready_event_sent && !self.load_event_sent && (settled || expired) {
            self.load_event_sent = true;
            self.loading = false;
            self.resource_events();
            self.run("document.readyState='complete';document.dispatchEvent(new Event('readystatechange'));window.dispatchEvent(new Event('load'))", "wega:load");
        }
    }
    /// `load`/`error` on images and stylesheets whose fetches finished.
    fn resource_events(&mut self) {
        let done = std::mem::take(
            &mut *self
                .completed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        if done.is_empty() || self.script.is_none() {
            return;
        }
        self.run(
            &format!("__wega_resources_done({})", serde_json::json!(done)),
            "wega:resources",
        );
    }
    /// Act on what page script asked for since the last task.
    fn drain_requests(&mut self) {
        let Some(script) = &mut self.script else {
            return;
        };
        let requests = script.take_requests();
        if !requests.writes.is_empty() {
            match &mut self.parser {
                Some(parser) if !parser.is_finished() => parser.write(&requests.writes),
                _ => self.notice("document.write after the document loaded was ignored".into()),
            }
        }
        for url in requests.opened {
            self.events.push_back(PageEvent::OpenWindow(url));
        }
        for delta in requests.traverse {
            self.events.push_back(PageEvent::Traverse(delta));
        }
        for node in requests.activate {
            // element.click(): script already saw the event; run the defaults.
            if self.default_click(node) {
                continue;
            }
            let event = self
                .document
                .borrow()
                .get_node(node)
                .map(|n| n.synthetic_click_event_data(Modifiers::empty()));
            if let Some(event) = event {
                let generated = {
                    let mut generated = Vec::new();
                    let mut doc = self.document.borrow_mut();
                    let mut dom_event = DomEvent::new(node, DomEventData::Click(event));
                    doc.handle_dom_event(&mut dom_event, |event| generated.push(event));
                    generated
                };
                self.generated_events(generated, serde_json::json!({}), None);
            }
        }
        for request in requests.scripts {
            let before = self.ordered.len();
            self.prepare_script(request.node, false, false);
            // Script-inserted scripts are async unless `async` was set to false.
            if request.force_async && self.ordered.len() > before {
                let script = self.ordered.pop_back().unwrap();
                self.asynchronous.push(script);
            }
        }
    }
    /// A navigation started by the page (link, form, script).
    fn navigation(&mut self, options: NavigationOptions) {
        if options.method == blitz_traits::net::http::Method::GET {
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
            body: Some(body),
            content_type: Some(content_type),
            initiator: Some(self.document.borrow().url().clone()),
            referrer_policy: self.referrer_policy.clone(),
            destination: Destination::Document,
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
        self.dirty = true;
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
        self.dirty = true;
    }
    fn notice(&mut self, error: String) {
        if self.notices.len() >= 16 {
            self.notices.pop_front();
        }
        self.notices.push_back(error);
        self.dirty = true;
    }
    fn load_error(&mut self, error: String) {
        let escaped = escape(&error);
        self.showing_error = false;
        let _=self.set_html(&format!("<!doctype html><title>Page unavailable</title><body style='font:18px system-ui;padding:36px;background:white;color:#142b24'><h1>This page could not be loaded</h1><p>{escaped}</p><p>Retry, or choose Servo using the engine selector below.</p>"));
        self.showing_error = true;
        self.notice(error);
    }
    fn script_error(&mut self, error: crate::script::ScriptError) {
        crate::diagnostics::Diagnostic::error(
            crate::diagnostics::Category::Javascript,
            error.to_string(),
            Some(self.url.to_string()),
        )
        .emit();
        if error.fatal || error.message.contains("; missing:") {
            self.compatibility
                .get_or_insert_with(|| error.message.clone());
        }
        if error.fatal {
            self.script = None;
            self.blocking = None;
            self.deferred.clear();
            self.ordered.clear();
            self.asynchronous.clear();
        }
        self.notice(error.to_string());
    }
    fn run(&mut self, source: &str, name: &str) {
        self.run_program(source, name, false);
    }
    fn run_program(&mut self, source: &str, name: &str, module: bool) {
        if let Some(script) = &mut self.script {
            let result = if module {
                script.run_module(source, name)
            } else {
                script.run(source, name)
            }
            .and_then(|_| script.tick());
            if let Err(error) = result {
                self.script_error(error);
            }
            self.dirty = true;
        }
    }
    pub fn evaluate(&mut self, source: &str) -> Result<String, String> {
        let result = self
            .script
            .as_mut()
            .ok_or("No script context")?
            .run(source, "evaluate")
            .and_then(|value| {
                self.script.as_mut().unwrap().tick()?;
                Ok(value)
            });
        self.dirty = true;
        if let Err(error) = &result
            && error.fatal
        {
            crate::diagnostics::Diagnostic::error(
                crate::diagnostics::Category::Javascript,
                error.to_string(),
                Some(self.url.to_string()),
            )
            .emit();
            self.script = None;
        }
        self.drain_requests();
        result.map_err(|e| e.to_string())
    }
    pub fn evaluate_json(&mut self, source: &str) -> Result<String, String> {
        let result = self
            .script
            .as_mut()
            .ok_or("No script context")?
            .evaluate_json(source)
            .and_then(|value| {
                self.script.as_mut().unwrap().tick()?;
                Ok(value)
            });
        self.dirty = true;
        if let Err(error) = &result {
            crate::diagnostics::Diagnostic::error(
                crate::diagnostics::Category::Javascript,
                error.to_string(),
                Some(self.url.to_string()),
            )
            .emit();
            if error.fatal {
                self.script = None;
            }
        }
        self.drain_requests();
        result.map_err(|e| e.to_string())
    }
    pub fn title(&self) -> String {
        let doc = self.document.borrow();
        doc.query_selector("title")
            .ok()
            .flatten()
            .and_then(|id| doc.get_node(id))
            .map(|n| {
                n.text_content()
                    .split_ascii_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
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
        if self.frozen {
            return None;
        }
        let parser_runnable = self
            .parser
            .as_ref()
            .is_some_and(|parser| !parser.is_finished())
            && self.blocking.is_none();
        if parser_runnable {
            return Some(Duration::ZERO);
        }
        let mut wake = self.script.as_ref().and_then(|script| script.next_timer());
        let mut at = |instant: Instant| {
            let delay = instant.saturating_duration_since(Instant::now());
            wake = Some(wake.map_or(delay, |w| w.min(delay)));
        };
        if let Some((due, _)) = &self.refresh {
            at(*due);
        }
        if let Some(ready) = self.ready_at
            && !self.load_event_sent
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
    pub fn node_count(&self) -> usize {
        TreeTraverser::new(&self.document.borrow()).count()
    }
    /// Script-created nodes and charged text bytes, for budget diagnostics.
    pub fn dom_budget(&self) -> Option<(usize, usize)> {
        self.script.as_ref().map(|script| script.budget())
    }
    pub fn needs_paint(&self) -> bool {
        self.dirty && !self.frozen
    }
    pub fn request_redraw(&mut self) {
        self.dirty = true;
    }
    pub fn freeze(&mut self, freeze: bool) {
        let changed = self.frozen != freeze;
        self.frozen = freeze;
        if let Some(script) = &mut self.script {
            script.freeze(freeze);
        }
        if changed {
            // Pause timers before hiding; resume them before delivering the
            // visible event so overdue callbacks are not postponed by this task.
            self.run(
                &format!("__wega_visibility({freeze})"),
                "wega:visibilitychange",
            );
        }
        if freeze {
            self.document.borrow_mut().release_image_cache();
            self.raster.release();
        }
        if !freeze {
            self.dirty = true;
        }
    }
    fn activate(&mut self) {
        self.activation.set(Some(Instant::now()));
    }
    fn page_point(&self, x: f32, y: f32) -> (f32, f32) {
        let scroll = self.document.borrow().viewport_scroll();
        (x + scroll.x as f32, y + scroll.y as f32)
    }
    fn pointer_event(&self, target: NodeId, pointer: &Pointer) -> Option<BlitzPointerEvent> {
        let doc = self.document.borrow();
        let mut event = doc
            .get_node(target)?
            .synthetic_click_event_data(pointer.modifiers());
        let (page_x, page_y) = self.page_point(pointer.x, pointer.y);
        event.coords = PointerCoords {
            page_x,
            page_y,
            screen_x: pointer.x,
            screen_y: pointer.y,
            client_x: pointer.x,
            client_y: pointer.y,
        };
        event.button = match pointer.button {
            1 => MouseEventButton::Auxiliary,
            2 => MouseEventButton::Secondary,
            3 => MouseEventButton::Fourth,
            4 => MouseEventButton::Fifth,
            _ => MouseEventButton::Main,
        };
        event.buttons = MouseEventButtons::from_bits_truncate(pointer.buttons);
        event.mods = pointer.modifiers();
        Some(event)
    }
    /// Dispatch a mouse/pointer event to script; true when it was cancelled.
    fn dispatch_pointer(
        &mut self,
        kind: &str,
        target: NodeId,
        pointer: &Pointer,
        related: Option<NodeId>,
    ) -> bool {
        let options = serde_json::json!({
            "clientX": pointer.x, "clientY": pointer.y,
            "screenX": pointer.x, "screenY": pointer.y,
            "button": pointer.button, "buttons": pointer.buttons,
            "shiftKey": pointer.modifiers & 1 != 0, "ctrlKey": pointer.modifiers & 2 != 0,
            "altKey": pointer.modifiers & 4 != 0, "metaKey": pointer.modifiers & 8 != 0,
            "deltaX": pointer.delta_x, "deltaY": pointer.delta_y,
            "relatedTarget": related.map(|id| id.as_u64().to_string()),
        });
        let source = format!(
            "__wega_native_event({}, {}, {})",
            serde_json::json!(kind),
            serde_json::json!(target.as_u64().to_string()),
            options
        );
        self.evaluate_json(&source).as_deref() == Ok("true")
    }
    /// Pointer input: hover, mouse/pointer events, Blitz defaults (focus, text
    /// selection, clicks), wheel scrolling and the cursor.
    pub fn pointer(&mut self, pointer: Pointer) {
        // Hit testing needs current layout (scrolling and script change it).
        self.document
            .borrow_mut()
            .resolve(self.started.elapsed().as_secs_f64());
        let (page_x, page_y) = self.page_point(pointer.x, pointer.y);
        let previous = self.document.borrow().get_hover_node_id();
        if pointer.kind == PointerKind::Leave {
            self.document.borrow_mut().clear_hover();
        } else {
            self.document.borrow_mut().set_hover_to(page_x, page_y);
        }
        let hovered = self.document.borrow().get_hover_node_id();
        if previous != hovered {
            // Restyle for :hover so the cursor below reflects it.
            self.document
                .borrow_mut()
                .resolve(self.started.elapsed().as_secs_f64());
            self.hover_changed(previous, hovered, &pointer);
        }
        let target = hovered.or_else(|| {
            self.document
                .borrow()
                .try_root_element()
                .map(|root| root.id)
        });
        let Some(target) = target else { return };
        match pointer.kind {
            PointerKind::Leave => {}
            PointerKind::Move => {
                let cancelled = self.dispatch_pointer("pointermove", target, &pointer, None);
                if !cancelled {
                    self.dispatch_pointer("mousemove", target, &pointer, None);
                }
                self.blitz_pointer(target, &pointer, DomEventData::PointerMove);
            }
            PointerKind::Down => {
                self.activate();
                let cancelled = self.dispatch_pointer("pointerdown", target, &pointer, None);
                let cancelled =
                    cancelled || self.dispatch_pointer("mousedown", target, &pointer, None);
                {
                    let mut doc = self.document.borrow_mut();
                    doc.active_node();
                    doc.set_mousedown_node_id(Some(target));
                }
                if !cancelled {
                    self.blitz_pointer(target, &pointer, DomEventData::PointerDown);
                }
            }
            PointerKind::Up => {
                self.activate();
                self.dispatch_pointer("pointerup", target, &pointer, None);
                self.dispatch_pointer("mouseup", target, &pointer, None);
                self.document.borrow_mut().unactive_node();
                // Blitz generates the click (or contextmenu) unless this was a drag.
                self.blitz_pointer(target, &pointer, DomEventData::PointerUp);
            }
            PointerKind::Wheel => {
                let cancelled = self.dispatch_pointer("wheel", target, &pointer, None);
                if !cancelled {
                    let mut doc = self.document.borrow_mut();
                    // Blitz scrolls with winit's sign; DOM deltas are positive downwards.
                    doc.scroll_node_by(target, -pointer.delta_x, -pointer.delta_y, |_| {});
                    drop(doc);
                    self.run("window.dispatchEvent(new Event('scroll'))", "wega:scroll");
                }
            }
        }
        let cursor = self
            .document
            .borrow()
            .get_cursor()
            .map(|cursor| cursor.name().to_owned())
            .unwrap_or_else(|| "default".into());
        if cursor != self.cursor {
            self.cursor = cursor.clone();
            self.events.push_back(PageEvent::Cursor(cursor));
        }
        self.dirty = true;
    }
    fn hover_changed(
        &mut self,
        previous: Option<NodeId>,
        hovered: Option<NodeId>,
        pointer: &Pointer,
    ) {
        let (old_chain, new_chain) = {
            let doc = self.document.borrow();
            let chain = |id: Option<NodeId>| {
                id.filter(|id| doc.get_node(*id).is_some())
                    .map(|id| doc.node_chain(id))
                    .unwrap_or_default()
            };
            (chain(previous), chain(hovered))
        };
        if let Some(target) = previous.filter(|id| self.document.borrow().get_node(*id).is_some()) {
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
    }
    /// Run Blitz's default action for a pointer event and handle what it generates.
    fn blitz_pointer(
        &mut self,
        target: NodeId,
        pointer: &Pointer,
        make: fn(BlitzPointerEvent) -> DomEventData,
    ) {
        let Some(event) = self.pointer_event(target, pointer) else {
            return;
        };
        let generated = {
            let mut generated = Vec::new();
            let mut doc = self.document.borrow_mut();
            let mut event = DomEvent::new(target, make(event));
            doc.handle_dom_event(&mut event, |event| generated.push(event));
            generated
        };
        self.generated_events(generated, serde_json::json!({}), Some(pointer));
    }
    /// A primary click at viewport coordinates: down, up and the click it generates.
    pub fn click(&mut self, x: f32, y: f32) {
        self.document
            .borrow_mut()
            .resolve(self.started.elapsed().as_secs_f64());
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
    fn click_event(&mut self, target: NodeId, event: BlitzPointerEvent, pointer: Option<&Pointer>) {
        let fallback = Pointer::new(
            PointerKind::Up,
            event.coords.client_x,
            event.coords.client_y,
        );
        let pointer = *pointer.unwrap_or(&fallback);
        self.activate();
        if self.dispatch_pointer("click", target, &pointer, None) {
            return;
        }
        if self.default_click(target) {
            return;
        }
        let generated = {
            let mut generated = Vec::new();
            let mut doc = self.document.borrow_mut();
            let mut dom_event = DomEvent::new(target, DomEventData::Click(event));
            doc.handle_dom_event(&mut dom_event, |event| generated.push(event));
            generated
        };
        self.generated_events(generated, serde_json::json!({}), Some(&pointer));
    }
    /// Wega's own click defaults; true when the click was fully handled.
    fn default_click(&mut self, target: NodeId) -> bool {
        enum Action {
            Submit(NodeId, NodeId),
            NewWindow(Url),
            Fragment(Url),
        }
        let action = {
            let doc = self.document.borrow();
            let mut current = Some(target);
            let mut action = None;
            while let Some(id) = current {
                let Some(node) = doc.get_node(id) else { break };
                if let Some(element) = node.element_data() {
                    if element.attr(LocalName::from("disabled")).is_some() {
                        break;
                    }
                    let tag = &*element.name.local;
                    let kind = element.attr(LocalName::from("type")).unwrap_or_default();
                    if matches!(tag, "input" | "button")
                        && (element.is_submit_button()
                            || kind.eq_ignore_ascii_case("submit")
                            || (tag == "input" && kind.eq_ignore_ascii_case("image")))
                    {
                        action = crate::dom::Dom::form_owner(&doc, id)
                            .map(|form| Action::Submit(form, id));
                        break;
                    }
                    if tag == "a"
                        && let Some(href) = element.attr(LocalName::from("href"))
                        && let Ok(url) = doc.url().join(href.trim())
                    {
                        let target_name = element
                            .attr(LocalName::from("target"))
                            .unwrap_or_default()
                            .to_ascii_lowercase();
                        if !target_name.is_empty()
                            && !matches!(target_name.as_str(), "_self" | "_top" | "_parent")
                        {
                            action = Some(Action::NewWindow(url));
                        } else if self.same_document(&url) {
                            action = Some(Action::Fragment(url));
                        }
                        break;
                    }
                }
                current = node.parent;
            }
            action
        };
        match action {
            None => false,
            Some(Action::Submit(form, submitter)) => {
                let cancelled = self
                    .evaluate_json(&format!(
                        "__wega_submit_event({}, {})",
                        serde_json::json!(form.as_u64().to_string()),
                        serde_json::json!(submitter.as_u64().to_string())
                    ))
                    .as_deref()
                    == Ok("true");
                if !cancelled {
                    self.document.borrow().submit_form(form, submitter);
                }
                true
            }
            Some(Action::NewWindow(url)) => {
                if matches!(url.scheme(), "http" | "https") {
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
    fn generated_events(
        &mut self,
        events: Vec<DomEvent>,
        input_options: serde_json::Value,
        pointer: Option<&Pointer>,
    ) {
        for event in events {
            let target = event.target;
            match event.data {
                DomEventData::Click(click) => {
                    self.click_event(target, click, pointer);
                    continue;
                }
                // Blitz counts clicks for double clicks.
                DomEventData::DoubleClick(_) => {
                    if let Some(pointer) = pointer {
                        self.dispatch_pointer("dblclick", target, pointer, None);
                    }
                    continue;
                }
                DomEventData::ContextMenu(_) => {
                    if let Some(pointer) = pointer {
                        self.dispatch_pointer("contextmenu", target, pointer, None);
                    }
                    continue;
                }
                _ => {}
            }
            let event_type = event.data.name();
            let source = format!(
                "__wega_native_event({}, {}, {})",
                serde_json::json!(event_type),
                serde_json::json!(target.as_u64().to_string()),
                if event_type == "input" {
                    input_options.clone()
                } else {
                    serde_json::json!({})
                }
            );
            self.run(&source, "wega:native-event");
        }
    }
    /// Native editing goes through Blitz/Parley after cancelable DOM events.
    pub fn key(&mut self, event: blitz_traits::events::BlitzKeyEvent) {
        let target = self
            .document
            .borrow()
            .get_focussed_node_id()
            .unwrap_or(self.document.borrow().root_node().id);
        let pressed = event.state.is_pressed();
        if pressed {
            self.activate();
        }
        let options = serde_json::json!({"key":event.key.to_string(),"code":event.code.to_string(),"repeat":event.is_auto_repeating,"isComposing":event.is_composing,"shiftKey":event.modifiers.contains(Modifiers::SHIFT),"ctrlKey":event.modifiers.contains(Modifiers::CONTROL),"altKey":event.modifiers.contains(Modifiers::ALT),"metaKey":event.modifiers.contains(Modifiers::META)});
        let source = format!(
            "__wega_native_event({}, {}, {})",
            serde_json::json!(if pressed { "keydown" } else { "keyup" }),
            serde_json::json!(target.as_u64().to_string()),
            options
        );
        if self.evaluate_json(&source).as_deref() == Ok("true") {
            return;
        }
        if pressed {
            let key = event.key.to_string();
            if key == "Enter" && self.implicit_submission(target) {
                self.dirty = true;
                return;
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
                let source = format!(
                    "__wega_native_event('beforeinput',{}, {})",
                    serde_json::json!(target.as_u64().to_string()),
                    input_options
                );
                if self.evaluate_json(&source).as_deref() == Ok("true") {
                    return;
                }
            }
            let generated = {
                let mut generated = Vec::new();
                self.document.borrow_mut().handle_dom_event(
                    &mut DomEvent::new(target, DomEventData::KeyDown(event)),
                    |event| generated.push(event),
                );
                generated
            };
            self.generated_events(generated, input_options, None);
        }
        self.dirty = true;
    }
    /// Enter in a single-line field submits its form (HTML implicit submission):
    /// through the default button when there is one, else when the form has
    /// only one field that blocks implicit submission.
    fn implicit_submission(&mut self, field: NodeId) -> bool {
        let (form, submitter) = {
            let doc = self.document.borrow();
            let Some(element) = doc.get_node(field).and_then(|n| n.element_data()) else {
                return false;
            };
            let kind = element
                .attr(LocalName::from("type"))
                .unwrap_or("text")
                .to_ascii_lowercase();
            let single_line = &*element.name.local == "input"
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
            let Some(form) = crate::dom::Dom::form_owner(&doc, field) else {
                return false;
            };
            let mut button = None;
            let mut blocking = 0;
            for id in TreeTraverser::new(&doc) {
                let Some(control) = doc.get_node(id).and_then(|n| n.element_data()) else {
                    continue;
                };
                if crate::dom::Dom::form_owner(&doc, id) != Some(form) {
                    continue;
                }
                let tag = &*control.name.local;
                let kind = control
                    .attr(LocalName::from("type"))
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if button.is_none()
                    && matches!(tag, "input" | "button")
                    && (control.is_submit_button()
                        || kind == "submit"
                        || (tag == "input" && kind == "image"))
                {
                    button = Some((id, control.attr(LocalName::from("disabled")).is_some()));
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
            let event = self
                .document
                .borrow()
                .get_node(button)
                .map(|n| n.synthetic_click_event_data(Modifiers::empty()));
            if let Some(event) = event {
                self.click_event(button, event, None);
            }
            return true;
        }
        let cancelled = self
            .evaluate_json(&format!(
                "__wega_submit_event({}, null)",
                serde_json::json!(form.as_u64().to_string())
            ))
            .as_deref()
            == Ok("true");
        if !cancelled {
            self.document.borrow().submit_form(form, form);
        }
        true
    }
    pub fn ime(&mut self, event: blitz_traits::events::BlitzImeEvent) {
        use blitz_traits::events::BlitzImeEvent;
        let target = self
            .document
            .borrow()
            .get_focussed_node_id()
            .unwrap_or(self.document.borrow().root_node().id);
        let (text, kind) = match &event {
            BlitzImeEvent::Preedit(text, _) => (text, "compositionupdate"),
            BlitzImeEvent::Commit(text) => (text, "compositionend"),
            _ => return,
        };
        if matches!(&event,BlitzImeEvent::Preedit(text,_) if !text.is_empty()) && !self.composing {
            self.composing = true;
            self.run(
                &format!(
                    "__wega_native_event('compositionstart',{},{{data:''}})",
                    serde_json::json!(target.as_u64().to_string())
                ),
                "wega:composition",
            );
        }
        let commit = matches!(event, BlitzImeEvent::Commit(_));
        let input_options = serde_json::json!({"inputType":if commit{"insertText"}else{"insertCompositionText"},"data":text,"isComposing":!commit});
        if commit {
            self.composing = false;
            if self
                .evaluate_json(&format!(
                    "__wega_native_event('beforeinput',{}, {})",
                    serde_json::json!(target.as_u64().to_string()),
                    input_options
                ))
                .as_deref()
                == Ok("true")
            {
                return;
            }
        }
        self.run(
            &format!(
                "__wega_native_event({}, {},{{data:{}}})",
                serde_json::json!(kind),
                serde_json::json!(target.as_u64().to_string()),
                serde_json::json!(text)
            ),
            "wega:composition",
        );
        let generated = {
            let mut generated = Vec::new();
            self.document.borrow_mut().handle_dom_event(
                &mut DomEvent::new(target, DomEventData::Ime(event)),
                |event| generated.push(event),
            );
            generated
        };
        self.generated_events(generated, input_options, None);
        self.dirty = true;
    }
    pub fn scroll_at(&mut self, x: f64, y: f64, pointer_x: f32, pointer_y: f32) {
        let mut doc = self.document.borrow_mut();
        let target = doc
            .hit(pointer_x, pointer_y)
            .map(|hit| hit.node_id)
            .unwrap_or(doc.root_element().id);
        let mut events = vec![];
        doc.scroll_node_by(target, -x, -y, |event| events.push(event));
        drop(doc);
        self.generated_events(events, serde_json::json!({}), None);
        self.dirty = true;
    }
    /// Scroll the viewport (or the scroll container under the pointer, via `wheel`).
    pub fn scroll(&mut self, x: f64, y: f64) {
        let mut doc = self.document.borrow_mut();
        let p = doc.viewport_scroll();
        doc.set_viewport_scroll(blitz_dom::Point {
            x: p.x + x,
            y: p.y + y,
        });
        self.dirty = true;
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
        let mut doc = self.document.borrow_mut();
        doc.set_viewport(Viewport {
            window_size: (width, height),
            hidpi_scale: scale,
            zoom,
            color_scheme: if dark {
                ColorScheme::Dark
            } else {
                ColorScheme::Light
            },
        });
        let large = doc.tree().len() >= 512;
        doc.set_style_threading(if large {
            blitz_dom::StyleThreading::Parallel
        } else {
            blitz_dom::StyleThreading::Sequential
        });
        doc.resolve(self.started.elapsed().as_secs_f64());
        drop(doc);
        let another_frame = if let Some(script) = &mut self.script {
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
        let mut doc = self.document.borrow_mut();
        doc.resolve(self.started.elapsed().as_secs_f64());
        if !self.reported_frames
            && doc.query_selector_all("iframe").is_ok_and(|ids| {
                ids.iter().any(|id| {
                    doc.get_node(*id).is_some_and(|node| {
                        let layout = node.final_layout();
                        layout.content_box_width() > 1.0
                            && layout.content_box_height() > 1.0
                            && node.has_boxes()
                            && node.primary_styles().is_some_and(|style| {
                                style.clone_visibility()
                                    == style::computed_values::visibility::T::Visible
                            })
                    })
                })
            })
        {
            self.reported_frames = true;
            self.compatibility.get_or_insert_with(||"Visible embedded frames require Servo; Wega does not yet render frame documents".into());
        }
        let mut scene = anyrender::Scene::new();
        blitz_paint::paint_scene(
            &mut scene,
            &mut doc,
            (scale * zoom) as f64,
            width,
            height,
            0,
            0,
        );
        self.dirty = another_frame || doc.is_animating();
        Ok(scene)
    }
    pub fn configure_shared_frames(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.raster.set_store(
            wega_platform::FrameStore::open(path, 4096 * 2160 * 4).map_err(|e| e.to_string())?,
        )
    }
    pub fn configure_shared_store(
        &mut self,
        store: wega_platform::FrameStore,
    ) -> Result<(), String> {
        self.raster.set_store(store)
    }
    pub fn paint_pending(&self) -> bool {
        self.raster.pending()
    }
    pub fn begin_paint(
        &mut self,
        viewport: Viewport,
        serial: u64,
        target: Option<(u8, u64)>,
    ) -> Result<(), String> {
        let (width, height) = viewport.window_size;
        let scene = self.record_paint(
            width,
            height,
            viewport.hidpi_scale,
            viewport.zoom,
            matches!(viewport.color_scheme, ColorScheme::Dark),
        )?;
        self.raster.start(serial, width, height, scene, target)
    }
    pub fn take_painted(&mut self) -> Option<Result<crate::Painted, String>> {
        self.raster.take()
    }
    pub fn wait_painted(&mut self) -> Result<crate::Painted, String> {
        self.raster.wait()
    }
    pub fn paint(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        zoom: f32,
        dark: bool,
    ) -> Result<Frame, String> {
        if self.raster.pending() {
            let _ = self.raster.wait();
        }
        self.begin_paint(
            Viewport {
                window_size: (width, height),
                hidpi_scale: scale,
                zoom,
                color_scheme: if dark {
                    ColorScheme::Dark
                } else {
                    ColorScheme::Light
                },
            },
            0,
            None,
        )?;
        let result = self.raster.wait()?;
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
    pub fn text(&self, selector: &str) -> Option<String> {
        let doc = self.document.borrow();
        doc.query_selector(selector)
            .ok()
            .flatten()
            .and_then(|id| doc.get_node(id))
            .map(|n| n.text_content())
    }
    pub fn node_id(&self, selector: &str) -> Option<NodeId> {
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
            for (name, value) in entries {
                let name = name.replace('"', "%22").replace(['\r', '\n'], " ");
                out.extend_from_slice(
                    format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                        .as_bytes(),
                );
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
        use blitz_traits::net::{Entry, EntryValue, FormData};
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
}
