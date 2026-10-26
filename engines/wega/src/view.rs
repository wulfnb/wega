use crate::{Limits, network::Network, script::ScriptHost};

use blitz_dom::{BaseDocument, DocumentConfig, NodeId, traversal::TreeTraverser};
use blitz_html::HtmlDocument;
use blitz_traits::{
    navigation::{NavigationOptions, NavigationProvider},
    net::{AbortController, AbortSignal, Bytes, NetHandler, NetProvider, Request},
    shell::{ColorScheme, Viewport},
};
use html5ever::LocalName;
use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
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
type FetchResult = Result<(Url, Vec<u8>), String>;

struct Resources {
    network: Network,
    local: Option<std::path::PathBuf>,
    limit: usize,
    revision: Arc<AtomicU64>,
}
impl NetProvider for Resources {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let local = self.local.clone();
        let policy = self.network.policy.clone();
        let wake = self.network.wake.clone();
        let revision = self.revision.clone();
        let limit = self.limit;
        // HTTP GET subresources only; no access to local files from web pages.
        if request.method != blitz_traits::net::http::Method::GET {
            handler.bytes(request.url.to_string(), Bytes::new());
            return;
        }
        let handler = Arc::new(std::sync::Mutex::new(Some(handler)));
        let fallback = handler.clone();
        let failed_url = request.url.to_string();
        let failed_wake = self.network.wake.clone();
        let failed_revision = self.revision.clone();
        let task = move |client: &crate::network::Client| {
            if request.signal.as_ref().is_some_and(AbortSignal::aborted) {
                return;
            }
            let policy = policy
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let response = Network::fetch(client, &policy, &request.url, limit, local.as_deref());
            if request.signal.as_ref().is_some_and(AbortSignal::aborted) {
                return;
            }
            let (url, bytes) = match response {
                Ok((url, bytes)) => (url, bytes),
                Err(error) => {
                    eprintln!("Wega resource: {error}");
                    (request.url, vec![])
                }
            };
            if let Some(handler) = handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                handler.bytes(url.to_string(), Bytes::from(bytes));
            }
            revision.fetch_add(1, Ordering::Relaxed);
            wake();
        };
        if let Err(error) = self.network.spawn(task) {
            eprintln!("Wega resource: {error}");
            if let Some(handler) = fallback
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                handler.bytes(failed_url, Bytes::new());
            }
            failed_revision.fetch_add(1, Ordering::Relaxed);
            failed_wake();
        }
    }
}
struct Navigation(mpsc::Sender<Url>, Arc<dyn Fn() + Send + Sync>);
impl NavigationProvider for Navigation {
    fn navigate_to(&self, options: NavigationOptions) {
        if options.method == blitz_traits::net::http::Method::GET {
            let _ = self.0.send(options.url);
            (self.1)();
        }
    }
}
enum Script {
    Inline(String, String, bool),
    External(Url, bool),
}

pub struct View {
    document: Rc<RefCell<BaseDocument>>,
    script: Option<Box<dyn ScriptHost>>,
    network: Network,
    limits: Limits,
    abort: Option<AbortController>,
    pending: Option<mpsc::Receiver<FetchResult>>,
    scripts: VecDeque<Script>,
    pending_script: Option<mpsc::Receiver<FetchResult>>,
    pending_script_module: bool,
    navigate: Rc<RefCell<Vec<Url>>>,
    navigation_rx: mpsc::Receiver<Url>,
    revision: Arc<AtomicU64>,
    last_revision: u64,
    raster: crate::raster::Raster,
    pub compatibility: Option<String>,
    started: Instant,
    dirty: bool,
    frozen: bool,
    ready_event_sent: bool,
    composing: bool,
    pub url: Url,
    pub loading: bool,
    pub notices: VecDeque<String>,
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
        let mut view = Self {
            document: Rc::new(RefCell::new(document)),
            script: None,
            network,
            limits,
            abort: None,
            pending: None,
            scripts: VecDeque::new(),
            pending_script: None,
            pending_script_module: false,
            navigate: Default::default(),
            navigation_rx,
            revision: Arc::new(AtomicU64::new(0)),
            last_revision: 0,
            raster,
            compatibility: None,
            started: Instant::now(),
            dirty: true,
            frozen: false,
            ready_event_sent: false,
            composing: false,
            url: url.clone(),
            loading: false,
            notices: VecDeque::new(),
        };
        view.load(url)?;
        Ok(view)
    }
    fn fetch(&self, url: Url, limit: usize) -> Result<mpsc::Receiver<FetchResult>, String> {
        let (sender, receiver) = mpsc::channel();
        let wake = self.network.wake.clone();
        let policy = self.network.policy.clone();
        let local = if self.url.scheme() == "file" {
            self.url
                .to_file_path()
                .ok()
                .and_then(|p| p.canonicalize().ok())
                .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
        } else {
            None
        };
        self.network.spawn(move |client| {
            let policy = policy
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let result = Network::fetch(client, &policy, &url, limit, local.as_deref());
            let _ = sender.send(result);
            wake();
        })?;
        Ok(receiver)
    }
    pub fn load(&mut self, url: Url) -> Result<(), String> {
        self.stop();
        self.url = url.clone();
        self.loading = true;
        self.compatibility = None;
        self.ready_event_sent = false;
        self.composing = false;
        if url.as_str() == "about:blank" {
            self.set_html("<!doctype html><title>New tab</title><body></body>")?;
        } else {
            self.pending = Some(self.fetch(url, self.limits.html_bytes)?);
        }
        Ok(())
    }
    pub fn stop(&mut self) {
        if let Some(abort) = self.abort.take() {
            abort.abort();
        }
        self.pending = None;
        self.pending_script = None;
        self.scripts.clear();
        self.script = None;
        self.loading = false;
    }
    pub fn set_html(&mut self, html: &str) -> Result<(), String> {
        if html.len() > self.limits.html_bytes {
            return Err("HTML document exceeds the size limit".into());
        }
        if let Some(abort) = self.abort.take() {
            abort.abort();
        }
        let abort = AbortController::default();
        let (sender, receiver) = mpsc::channel();
        self.navigation_rx = receiver;
        let local = if self.url.scheme() == "file" {
            self.url
                .to_file_path()
                .ok()
                .and_then(|p| p.canonicalize().ok())
                .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
        } else {
            None
        };
        let config = DocumentConfig {
            base_url: Some(self.url.to_string()),
            abort_signal: Some(abort.signal.clone()),
            net_provider: Some(Arc::new(Resources {
                network: self.network.clone(),
                local,
                limit: self.limits.resource_bytes,
                revision: self.revision.clone(),
            })),
            navigation_provider: Some(Arc::new(Navigation(sender, self.network.wake.clone()))),
            ..Default::default()
        };
        let doc = HtmlDocument::from_html(html, config).into_inner();
        if TreeTraverser::new(&doc)
            .take(self.limits.dom_nodes + 1)
            .count()
            > self.limits.dom_nodes
        {
            return Err("DOM node limit exceeded; use Servo for this page".into());
        }
        let mut scripts = VecDeque::new();
        for (index, id) in doc
            .query_selector_all("script")
            .map_err(|_| "Unable to inspect scripts")?
            .iter()
            .take(64)
            .enumerate()
        {
            let node = doc.get_node(*id).unwrap();
            let element = node.element_data().unwrap();
            let kind = element.attr(LocalName::from("type")).unwrap_or_default();
            let module = kind == "module";
            if !kind.is_empty()
                && !matches!(
                    kind,
                    "text/javascript" | "application/javascript" | "module"
                )
            {
                continue;
            }
            if let Some(source) = element.attr(LocalName::from("src")) {
                if let Ok(url) = self.url.join(source) {
                    scripts.push_back(Script::External(url, module));
                }
            } else {
                scripts.push_back(Script::Inline(
                    node.text_content(),
                    if module {
                        let mut url = self.url.clone();
                        url.set_query(Some(&format!("__wega_inline_module={}", index + 1)));
                        url.to_string()
                    } else {
                        format!("{} inline #{}", self.url, index + 1)
                    },
                    module,
                ));
            }
        }
        self.script = None;
        self.document = Rc::new(RefCell::new(doc));
        self.abort = Some(abort);
        self.scripts = scripts;
        self.script = Some(crate::script::create(
            self.document.clone(),
            &self.limits,
            self.navigate.clone(),
            html.len(),
            self.network.clone(),
        )?);
        self.dirty = true;
        self.ready_event_sent = false;
        self.composing = false;
        Ok(())
    }
    pub fn poll(&mut self) {
        if self.frozen {
            return;
        }
        let result = self.pending.as_ref().and_then(|r| r.try_recv().ok());
        if let Some(result) = result {
            self.pending = None;
            match result {
                Ok((url, bytes)) => {
                    self.url = url;
                    let html = String::from_utf8_lossy(&bytes);
                    if let Err(error) = self.set_html(&html) {
                        self.load_error(error);
                    }
                }
                Err(error) => self.load_error(error),
            }
        }
        self.document.borrow_mut().handle_messages();
        let revision = self.revision.load(Ordering::Relaxed);
        if revision != self.last_revision {
            self.last_revision = revision;
            self.dirty = true;
        }
        let script_result = self.pending_script.as_ref().and_then(|r| r.try_recv().ok());
        if let Some(result) = script_result {
            self.pending_script = None;
            match result {
                Ok((url, bytes)) => self.run_program(
                    &String::from_utf8_lossy(&bytes),
                    url.as_str(),
                    self.pending_script_module,
                ),
                Err(error) => self.notice(error),
            }
        }
        if self.pending.is_none()
            && self.pending_script.is_none()
            && let Some(script) = self.scripts.pop_front()
        {
            match script {
                Script::Inline(source, name, module) => self.run_program(&source, &name, module),
                Script::External(url, module) => match self.fetch(url, 1024 * 1024) {
                    Ok(rx) => {
                        self.pending_script = Some(rx);
                        self.pending_script_module = module;
                    }
                    Err(error) => self.notice(error),
                },
            }
        }
        if self.pending.is_none()
            && self.pending_script.is_none()
            && self.scripts.is_empty()
            && !self
                .script
                .as_ref()
                .is_some_and(|script| script.modules_pending())
            && !self.ready_event_sent
        {
            self.ready_event_sent = true;
            self.loading = false;
            self.run("document.readyState='interactive';document.dispatchEvent(new Event('readystatechange'));__wega_dispatch('DOMContentLoaded',document._id);document.readyState='complete';document.dispatchEvent(new Event('readystatechange'));window.dispatchEvent(new Event('load'))", "wega:load");
        }
        if let Some(script) = &mut self.script {
            let due = script.next_timer().is_some_and(|t| t.is_zero());
            if due {
                let result = script.tick();
                if let Err(error) = result {
                    self.script_error(error);
                }
                self.dirty = true;
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
        if self.pending.is_none() && !self.loading {
            self.url = self.document.borrow().url().clone();
        }
        while let Ok(url) = self.navigation_rx.try_recv() {
            self.navigate.borrow_mut().push(url);
        }
    }
    fn notice(&mut self, error: String) {
        if self.notices.len() >= 16 {
            self.notices.pop_front();
        }
        self.notices.push_back(error);
        self.dirty = true;
    }
    fn load_error(&mut self, error: String) {
        let escaped = error
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let _=self.set_html(&format!("<!doctype html><title>Page unavailable</title><body style='font:18px system-ui;padding:36px;background:white;color:#142b24'><h1>This page could not be loaded</h1><p>{escaped}</p><p>Retry, or choose Servo using the engine selector below.</p>"));
        self.loading = false;
        self.notice(error);
    }
    fn script_error(&mut self, error: crate::script::ScriptError) {
        if error.fatal || error.message.contains("; missing:") {
            self.compatibility
                .get_or_insert_with(|| error.message.clone());
        }
        if error.fatal {
            self.script = None;
            self.scripts.clear();
            self.pending_script = None;
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
            self.script = None;
        }
        result.map_err(|e| e.to_string())
    }
    pub fn evaluate_json(&mut self, source: &str) -> Result<String, String> {
        let result = self
            .script
            .as_mut()
            .ok_or("No script context")?
            .evaluate_json(source);
        self.dirty = true;
        if let Err(error) = &result
            && error.fatal
        {
            self.script = None;
        }
        result.map_err(|e| e.to_string())
    }
    pub fn title(&self) -> String {
        let doc = self.document.borrow();
        doc.query_selector("title")
            .ok()
            .flatten()
            .and_then(|id| doc.get_node(id))
            .map(|n| n.text_content())
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
            None
        } else if !self.scripts.is_empty() && self.pending_script.is_none() {
            Some(Duration::ZERO)
        } else {
            self.script.as_ref().and_then(|script| script.next_timer())
        }
    }
    pub fn heap_bytes(&self) -> usize {
        self.script.as_ref().map_or(0, |script| script.heap_bytes())
    }
    pub fn node_count(&self) -> usize {
        TreeTraverser::new(&self.document.borrow()).count()
    }
    pub fn needs_paint(&self) -> bool {
        self.dirty && !self.frozen
    }
    pub fn request_redraw(&mut self) {
        self.dirty = true;
    }
    pub fn freeze(&mut self, freeze: bool) {
        self.frozen = freeze;
        if let Some(script) = &mut self.script {
            script.freeze(freeze);
        }
        if freeze {
            self.document.borrow_mut().release_image_cache();
            self.raster.release();
        }
        if !freeze {
            self.dirty = true;
        }
    }
    pub fn click(&mut self, x: f32, y: f32) {
        let id = self.document.borrow().element_from_point(x, y);
        if let Some(id) = id {
            let cancelled = self
                .script
                .as_mut()
                .map(|script| script.dispatch("click", id))
                .transpose();
            match cancelled {
                Err(error) => self.script_error(error),
                Ok(Some(true)) => {}
                _ => {
                    let generated = {
                        let mut generated = Vec::new();
                        let mut doc = self.document.borrow_mut();
                        if let Some(node) = doc.get_node(id) {
                            let mut event = blitz_traits::events::DomEvent::new(
                                id,
                                node.synthetic_click_event(Default::default()),
                            );
                            doc.handle_dom_event(&mut event, |event| generated.push(event));
                        }
                        generated
                    };
                    self.generated_events(generated, serde_json::json!({}));
                }
            }
            self.dirty = true;
        }
    }
    fn generated_events(
        &mut self,
        events: Vec<blitz_traits::events::DomEvent>,
        input_options: serde_json::Value,
    ) {
        for event in events {
            let event_type = event.data.name();
            let source = format!(
                "__wega_native_event({}, {}, {})",
                serde_json::json!(event_type),
                serde_json::json!(event.target.as_u64().to_string()),
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
        use blitz_traits::events::{DomEvent, DomEventData};
        use keyboard_types::Modifiers;
        let target = self
            .document
            .borrow()
            .get_focussed_node_id()
            .unwrap_or(self.document.borrow().root_node().id);
        let pressed = event.state.is_pressed();
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
            self.generated_events(generated, input_options);
        }
        self.dirty = true;
    }
    pub fn ime(&mut self, event: blitz_traits::events::BlitzImeEvent) {
        use blitz_traits::events::{BlitzImeEvent, DomEvent, DomEventData};
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
        self.generated_events(generated, input_options);
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
        self.generated_events(events, serde_json::json!({}));
        self.dirty = true;
    }
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
        self.dirty = another_frame;
        Ok(scene)
    }
    pub fn configure_shared_frames(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.raster.set_store(
            wega_platform::FrameStore::open(path, 4096 * 2160 * 4).map_err(|e| e.to_string())?,
        )
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
    pub fn paint(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        zoom: f32,
        dark: bool,
    ) -> Result<Frame, String> {
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
