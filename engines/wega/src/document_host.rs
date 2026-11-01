//! Parser, script ordering and document lifecycle ownership.
use super::*;

pub(super) enum ScriptState {
    Ready(String),
    Fetching(mpsc::Receiver<FetchResult>),
    Failed(String),
}
/// A script waiting to run: parser-blocking, deferred, ordered or async.
pub(super) struct PendingScript {
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
pub(super) struct ScriptElement {
    kind: Option<bool>,
    src: Option<String>,
    is_async: bool,
    defer: bool,
    nomodule: bool,
    nonce: String,
    text: String,
}
/// `Some(true)` for module scripts, `Some(false)` for classic, `None` for data blocks.
pub(super) fn script_kind(kind: &str, language: Option<&str>) -> Option<bool> {
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

pub(super) const MAX_SCRIPTS: usize = 1024;

pub(super) struct DocumentHost {
    pub(super) yielded: bool,
    pub(super) parser: Option<Parser>,
    pub(super) blocking: Option<PendingScript>,
    pub(super) deferred: VecDeque<PendingScript>,
    pub(super) ordered: VecDeque<PendingScript>,
    pub(super) asynchronous: Vec<PendingScript>,
    pub(super) started_scripts: Rc<RefCell<HashSet<NodeId>>>,
    pub(super) script_count: usize,
    pub(super) applied_meta: HashSet<NodeId>,
    pub(super) encoding: &'static Encoding,
    pub(super) ready_event_sent: bool,
    pub(super) load_event_sent: bool,
    pub(super) ready_at: Option<Instant>,
}
impl Default for DocumentHost {
    fn default() -> Self {
        Self {
            yielded: false,
            parser: None,
            blocking: None,
            deferred: VecDeque::new(),
            ordered: VecDeque::new(),
            asynchronous: Vec::new(),
            started_scripts: Default::default(),
            script_count: 0,
            applied_meta: HashSet::new(),
            encoding: encoding_rs::UTF_8,
            ready_event_sent: false,
            load_event_sent: false,
            ready_at: None,
        }
    }
}
impl View {
    /// Run the parser and the script queues as far as they can go without waiting.
    pub(super) fn advance(&mut self) {
        self.host.yielded = false;
        let turn = Instant::now();
        let mut steps = 0;
        loop {
            if steps >= 8 || turn.elapsed() >= Duration::from_millis(4) {
                self.host.yielded = true;
                break;
            }
            steps += 1;
            if self.script.is_none() {
                // A fatal script error ended scripting; finish parsing without scripts.
                self.host.blocking = None;
            }
            if let Some(blocking) = &mut self.host.blocking {
                if !blocking.ready(self.host.encoding) {
                    break;
                }
                let script = self.host.blocking.take().unwrap();
                self.execute(script);
                continue;
            }
            let Some(parser) = &mut self.host.parser else {
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
                        self.host.parser = None;
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
    pub(super) fn parsed(&mut self) {
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
    pub(super) fn apply_meta(&mut self) {
        let found: Vec<(NodeId, String)> = {
            let doc = self.document.borrow();
            doc.query_selector_all("meta[http-equiv]")
                .ok()
                .into_iter()
                .flatten()
                .filter(|id| !self.host.applied_meta.contains(id))
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
            self.host.applied_meta.insert(id);
            csp.add(&value);
        }
        let eval = csp.allows_eval();
        drop(csp);
        if let Some(script) = &mut self.script {
            script.allow_eval(eval);
        }
    }
    pub(super) fn read_script(&self, node: NodeId) -> Option<ScriptElement> {
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
    pub(super) fn prepare_script(&mut self, node: NodeId, parser: bool, in_order: bool) {
        if !self.host.started_scripts.borrow_mut().insert(node) {
            return;
        }
        let Some(element) = self.read_script(node) else {
            return;
        };
        let Some(module) = element.kind else { return };
        if element.nomodule && !module {
            return;
        }
        if self.host.script_count >= MAX_SCRIPTS {
            self.notice(format!(
                "Script limit of {MAX_SCRIPTS} reached; remaining scripts are skipped"
            ));
            return;
        }
        self.host.script_count += 1;
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
                (_, _, true, _) => self.host.asynchronous.push(pending),
                (true, true, _, _) | (true, false, false, true) => {
                    self.host.deferred.push_back(pending)
                }
                (true, false, false, false) if in_order => self.host.deferred.push_back(pending),
                (true, false, false, false) => self.host.blocking = Some(pending),
                // Script-inserted scripts: async unless script cleared `async`.
                (false, ..) => self.host.ordered.push_back(pending),
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
            url.set_query(Some(&format!(
                "__wega_inline_module={}",
                self.host.script_count
            )));
            url.to_string()
        } else {
            format!("{document} inline #{}", self.host.script_count)
        };
        let pending = PendingScript {
            node: Some(node),
            module,
            name,
            external: false,
            state: ScriptState::Ready(element.text),
        };
        if (module && parser && !element.is_async) || in_order {
            self.host.deferred.push_back(pending);
        } else {
            self.execute(pending);
        }
    }
    pub(super) fn script_failed(&mut self, node: NodeId, message: String) {
        self.notice(message);
        self.run(
            &format!(
                "__wega_script_done({}, false)",
                serde_json::json!(node.as_u64().to_string())
            ),
            "wega:script-error",
        );
    }
    pub(super) fn execute(&mut self, script: PendingScript) {
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
    pub(super) fn run_queues(&mut self) {
        let turn = Instant::now();
        let mut steps = 0;
        loop {
            if steps >= 8 || turn.elapsed() >= Duration::from_millis(4) {
                self.host.yielded = true;
                break;
            }
            steps += 1;
            let mut progressed = false;
            let parsed = self.host.parser.as_ref().is_none_or(Parser::is_finished)
                && self.host.blocking.is_none();
            if parsed
                && let Some(front) = self.host.deferred.front_mut()
                && front.ready(self.host.encoding)
            {
                let script = self.host.deferred.pop_front().unwrap();
                self.execute(script);
                progressed = true;
            }
            if let Some(front) = self.host.ordered.front_mut()
                && front.ready(self.host.encoding)
            {
                let script = self.host.ordered.pop_front().unwrap();
                self.execute(script);
                progressed = true;
            }
            if let Some(index) = self
                .host
                .asynchronous
                .iter_mut()
                .position(|script| script.ready(self.host.encoding))
            {
                let script = self.host.asynchronous.swap_remove(index);
                self.execute(script);
                progressed = true;
            }
            if !progressed {
                break;
            }
        }
    }
    /// DOMContentLoaded and load, as their own tasks after parsing and scripts.
    pub(super) fn lifecycle(&mut self) {
        let parsed = self.host.parser.as_ref().is_none_or(Parser::is_finished)
            && self.host.blocking.is_none();
        let modules = self
            .script
            .as_ref()
            .is_some_and(|script| script.modules_pending());
        if parsed
            && self.pending.is_none()
            && self.host.deferred.is_empty()
            && !modules
            && !self.host.ready_event_sent
        {
            self.host.ready_event_sent = true;
            self.host.ready_at = Some(Instant::now());
            self.run("document.readyState='interactive';document.dispatchEvent(new Event('readystatechange'));__wega_dispatch('DOMContentLoaded',document._id)", "wega:ready");
        }
        let settled = self.host.asynchronous.is_empty()
            && self.host.ordered.is_empty()
            && self.in_flight.load(Ordering::Relaxed) == 0
            && !modules;
        let expired = self
            .host
            .ready_at
            .is_some_and(|at| at.elapsed() > Duration::from_secs(30));
        if self.host.ready_event_sent && !self.host.load_event_sent && (settled || expired) {
            self.host.load_event_sent = true;
            self.loading = false;
            self.resource_events();
            self.run("document.readyState='complete';document.dispatchEvent(new Event('readystatechange'));window.dispatchEvent(new Event('load'))", "wega:load");
        }
    }
    /// `load`/`error` on images and stylesheets whose fetches finished.
    pub(super) fn resource_events(&mut self) {
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
    pub(super) fn drain_requests(&mut self) {
        let Some(script) = &mut self.script else {
            return;
        };
        let requests = script.take_requests();
        if !requests.writes.is_empty() {
            match &mut self.host.parser {
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
            let before = self.host.ordered.len();
            self.prepare_script(request.node, false, false);
            // Script-inserted scripts are async unless `async` was set to false.
            if request.force_async && self.host.ordered.len() > before {
                let script = self.host.ordered.pop_back().unwrap();
                self.host.asynchronous.push(script);
            }
        }
    }
}
