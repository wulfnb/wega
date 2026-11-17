//! Nested browsing contexts (`<iframe>`).
//!
//! A child browsing context is a [`View`] owned by its parent view (see
//! `docs/architecture/iframe-design.md`). The child brings its own document, script realm
//! (a separate isolate), network lifecycle, console and revision; this module adds the frame
//! identity, the committed security principal, the sandbox, message routing, compositing and
//! input routing around it. Nothing here shares an object between realms: cross-frame
//! communication goes through host-routed, serialized messages whose origin is computed here.
use super::*;

/// Maximum nesting depth of browsing contexts (the protocol allows 16; layout is far deeper).
pub(crate) const MAX_DEPTH: usize = 8;
/// Maximum live frames in one tab (all levels together).
pub(crate) const MAX_FRAMES: usize = 32;
/// Frames created per reconciliation: realm creation costs tens of milliseconds, so a page that
/// adds many iframes at once spreads the work over several turns instead of stalling one.
pub(crate) const MAX_CREATIONS_PER_TURN: usize = 2;
/// Maximum size of one serialized cross-frame message.
pub(crate) const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Maximum messages one realm may have queued for routing.
pub(crate) const MAX_OUTBOX: usize = 256;
const BLANK: &str = "<!doctype html><title></title><body></body>";

/// The sandbox flags Wega enforces. Everything not listed here is *denied*; a token that is not
/// enforced (for example `allow-pointer-lock`) never grants anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sandbox {
    /// A `sandbox` attribute (or an ancestor's) applies.
    pub active: bool,
    pub scripts: bool,
    pub same_origin: bool,
    pub forms: bool,
    pub popups: bool,
    pub modals: bool,
}
impl Sandbox {
    pub const NONE: Sandbox = Sandbox {
        active: false,
        scripts: true,
        same_origin: true,
        forms: true,
        popups: true,
        modals: true,
    };
    pub fn parse(attribute: Option<&str>) -> Self {
        let Some(value) = attribute else {
            return Self::NONE;
        };
        let mut sandbox = Self {
            active: true,
            scripts: false,
            same_origin: false,
            forms: false,
            popups: false,
            modals: false,
        };
        for token in value.split_ascii_whitespace() {
            match token.to_ascii_lowercase().as_str() {
                "allow-scripts" => sandbox.scripts = true,
                "allow-same-origin" => sandbox.same_origin = true,
                "allow-forms" => sandbox.forms = true,
                "allow-popups" => sandbox.popups = true,
                "allow-modals" => sandbox.modals = true,
                // Recognised but not enforced: they stay denied.
                _ => {}
            }
        }
        sandbox
    }
    /// A child's effective flags: restrictions only ever accumulate down the tree.
    pub fn inherit(self, child: Self) -> Self {
        Self {
            active: self.active || child.active,
            scripts: self.scripts && child.scripts,
            same_origin: self.same_origin && child.same_origin,
            forms: self.forms && child.forms,
            popups: self.popups && child.popups,
            modals: self.modals && child.modals,
        }
    }
    /// Without `allow-same-origin` a sandboxed document has an opaque origin.
    pub fn opaque(self) -> bool {
        self.active && !self.same_origin
    }
}

/// What a child view knows about itself. Set once, before its first document exists.
pub(crate) struct FrameRole {
    pub frame_id: u64,
    pub parent_frame_id: Option<u64>,
    pub depth: usize,
    pub sandbox: Sandbox,
    pub opaque: bool,
    /// The parent's CSP policies, inherited by `srcdoc` and `about:blank` documents.
    pub inherited_csp: Csp,
    /// The embedders' document URLs, nearest first, for `frame-ancestors`/`X-Frame-Options`.
    pub ancestors: Vec<Url>,
    /// This document is `srcdoc`/`about:blank` content (inherits the creator's origin).
    pub inherits_origin: bool,
    pub live: Rc<Cell<usize>>,
    pub next_key: Rc<Cell<u64>>,
}

/// Per-document capability limits derived from the sandbox and origin class.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Restrictions {
    pub opaque: bool,
    pub forms_blocked: bool,
    pub popups_blocked: bool,
    pub modals_blocked: bool,
}

/// Installed in every opaque-origin realm: origin-keyed storage and worker APIs are denied (an
/// opaque origin owns none of them). Each property must be successfully replaced or the realm is
/// not created.
pub(crate) const OPAQUE_LOCKDOWN: &str = r#"(() => {
  const deny = (target, name) => {
    Object.defineProperty(target, name, { configurable: false, enumerable: false, get() {
      throw new DOMException("The document is sandboxed and has an opaque origin.", "SecurityError");
    }, set() {} });
  };
  for (const name of ["localStorage", "sessionStorage", "indexedDB", "caches"]) deny(globalThis, name);
  for (const name of ["Worker", "SharedWorker", "BroadcastChannel"]) deny(globalThis, name);
  deny(navigator, "serviceWorker");
})()"#;

/// One row of the table a document's script host reads.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub node: NodeId,
    pub key: u64,
    pub same_origin: bool,
    pub name: String,
    pub length: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Child(u64),
    Parent,
    Top,
}
#[derive(Clone, Debug)]
pub(crate) enum Payload {
    Message(String),
    Navigate(Url),
}
#[derive(Clone, Debug)]
pub(crate) struct Outgoing {
    pub target: Target,
    pub target_origin: String,
    pub payload: Payload,
    /// The sender's origin, filled in by the first view that routes the message.
    pub sender: Option<String>,
}
#[derive(Clone)]
pub(crate) struct ChildRef {
    pub key: u64,
    pub node: NodeId,
    pub origin: String,
    pub opaque: bool,
    pub document: Rc<RefCell<BlitzDocument>>,
    pub dom: Option<Rc<crate::dom::Dom>>,
    pub document_generation: Rc<Cell<u64>>,
}

#[derive(Clone)]
pub(crate) struct ParentRef {
    pub origin: String,
    pub opaque: bool,
    pub frame_node: NodeId,
    pub document: std::rc::Weak<RefCell<BlitzDocument>>,
    pub dom: std::rc::Weak<crate::dom::Dom>,
    pub document_generation: std::rc::Weak<Cell<u64>>,
}

/// Shared between a view (writer, per document) and its script host (`Dom::call`).
#[derive(Default)]
pub(crate) struct FrameShared {
    pub entries: Vec<Entry>,
    pub outbox: Vec<Outgoing>,
    pub has_parent: bool,
    pub depth: usize,
    pub children: Vec<ChildRef>,
    pub parent: Option<ParentRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
struct Signature {
    src: Option<String>,
    srcdoc: Option<String>,
    sandbox: Option<String>,
}

pub(crate) struct ChildFrame {
    pub key: u64,
    pub node: NodeId,
    pub view: Box<View>,
    signature: Signature,
    /// Navigation epoch: bumped by every navigation of this frame; a commit for an older epoch
    /// can never replace a newer document.
    pub epoch: u64,
    loaded_epoch: u64,
    live: Rc<Cell<usize>>,
}
impl Drop for ChildFrame {
    fn drop(&mut self) {
        self.live.set(self.live.get().saturating_sub(1));
    }
}
#[derive(Default)]
pub(crate) struct FrameHost {
    pub children: Vec<ChildFrame>,
    seen_epoch: Option<u64>,
    /// Elements still waiting for their context (creation is rate limited).
    creation_deferred: bool,
    /// Iframe elements that were refused (limits, policy); not retried until they change.
    refused: HashSet<(NodeId, Signature)>,
    /// The frame that has keyboard focus (never more than one).
    pub focused: Option<u64>,
    hovered: Option<u64>,
}

/// `targetOrigin` against the receiver's committed origin (never URL equality of documents).
pub(crate) fn target_accepts(target_origin: &str, sender: &str, receiver: &str) -> bool {
    match target_origin {
        "*" => true,
        "/" => sender != "null" && sender == receiver,
        explicit => explicit != "null" && explicit == receiver,
    }
}

pub(crate) fn blocked_page(reason: &str) -> String {
    format!(
        "<!doctype html><title>Blocked</title><body style='font:14px system-ui;color:#555;padding:12px'>{}</body>",
        escape(reason)
    )
}

impl View {
    /// This document's origin as scripts and policy see it: `null` when opaque.
    pub(crate) fn origin_string(&self) -> String {
        if self.role.as_ref().is_some_and(|role| role.opaque) {
            return "null".into();
        }
        self.document.borrow().url().origin().ascii_serialization()
    }
    /// Child frames, for tests and tools: `(key, node)`.
    pub fn child_frames(&self) -> Vec<(u64, u64)> {
        self.frames
            .children
            .iter()
            .map(|child| (child.key, child.node.as_u64()))
            .collect()
    }
    pub fn child_by_key(&mut self, key: u64) -> Option<&mut View> {
        self.frames
            .children
            .iter_mut()
            .find(|child| child.key == key)
            .map(|child| &mut *child.view)
    }
    /// Live frames in the whole tab (this view's tree included).
    pub fn live_frames(&self) -> usize {
        fn count(view: &View) -> usize {
            view.frames
                .children
                .iter()
                .map(|child| 1 + count(&child.view))
                .sum()
        }
        count(self)
    }

    fn read_signature(&self, node: NodeId) -> Signature {
        let doc = self.document.borrow();
        let attr = |name: &str| {
            doc.get_node(node)
                .and_then(|node| node.element_data())
                .and_then(|element| element.attr(LocalName::from(name)).map(str::to_owned))
        };
        Signature {
            src: attr("src"),
            srcdoc: attr("srcdoc"),
            sandbox: attr("sandbox"),
        }
    }

    /// Reconcile child contexts with the `<iframe>` elements in the document and run them.
    pub(crate) fn sync_frames(&mut self) {
        self.reconcile_frames_now();
        if self.frames.children.is_empty() {
            return;
        }
        self.poll_frames();
        self.publish_frames();
    }

    /// Reconcile now if the document changed since the last reconciliation.
    pub(crate) fn reconcile_frames_now(&mut self) {
        let epoch = self.mutation_epoch();
        if self.frames.seen_epoch != Some(epoch) {
            self.frames.seen_epoch = Some(epoch);
            self.reconcile_frames();
            self.publish_frames();
        }
    }
    /// Every child has finished loading its current navigation.
    pub(crate) fn frames_loaded(&self) -> bool {
        !self.frames.creation_deferred
            && self.frames.children.iter().all(|child| {
                child.loaded_epoch == child.epoch
                    || (child.view.host.load_event_sent && child.view.pending.is_none())
            })
    }

    fn reconcile_frames(&mut self) {
        let nodes: Vec<NodeId> = self
            .document
            .borrow()
            .query_selector_all("iframe")
            .map(|ids| ids.to_vec())
            .unwrap_or_default();
        // Detach frames whose element left the document (removed, replaced, or its parent
        // re-created). Dropping the child tears down its realm, timers and requests.
        let before = self.frames.children.len();
        let mut removed = Vec::new();
        self.frames.children.retain(|child| {
            if nodes.contains(&child.node) {
                true
            } else {
                removed.push(child.key);
                false
            }
        });
        for key in removed {
            self.network.detach_frame(key);
            self.events.push_back(PageEvent::FrameDetached {
                frame: wega_protocol::identity::FrameId(key),
            });
        }
        if self.frames.children.len() != before {
            if let Some(focused) = self.frames.focused
                && !self.frames.children.iter().any(|c| c.key == focused)
            {
                self.frames.focused = None;
            }
            if let Some(hovered) = self.frames.hovered
                && !self.frames.children.iter().any(|c| c.key == hovered)
            {
                self.frames.hovered = None;
            }
        }
        self.frames.refused.retain(|(node, _)| nodes.contains(node));
        let mut created = 0;
        self.frames.creation_deferred = false;
        for node in nodes {
            let signature = self.read_signature(node);
            if let Some(index) = self.frames.children.iter().position(|c| c.node == node) {
                if self.frames.children[index].signature != signature {
                    self.navigate_frame(index, signature);
                }
            } else if !self.frames.refused.contains(&(node, signature.clone())) {
                if created >= MAX_CREATIONS_PER_TURN {
                    self.frames.creation_deferred = true;
                    continue;
                }
                created += 1;
                self.create_frame(node, signature);
            }
        }
        if self.frames.creation_deferred {
            // Come back next turn even if the document does not change again.
            self.frames.seen_epoch = None;
        }
    }

    fn refuse(&mut self, node: NodeId, signature: Signature, reason: &str) {
        self.notice(format!("iframe not loaded: {reason}"));
        self.frames.refused.insert((node, signature));
    }

    fn frame_role(&self, frame_id: u64, sandbox: Sandbox, opaque: bool, inherits: bool) -> FrameRole {
        let own = self.role.as_ref();
        let mut ancestors = vec![self.document.borrow().url().clone()];
        if let Some(role) = own {
            ancestors.extend(role.ancestors.iter().cloned());
        }
        FrameRole {
            frame_id,
            parent_frame_id: own.map(|r| r.frame_id).or(Some(1)),
            depth: own.map_or(0, |role| role.depth) + 1,
            sandbox,
            opaque,
            inherited_csp: self.csp().clone(),
            ancestors,
            inherits_origin: inherits,
            live: own.map_or_else(|| self.frames_live(), |role| role.live.clone()),
            next_key: own.map_or_else(|| self.frames_next_key(), |role| role.next_key.clone()),
        }
    }
    fn frames_live(&self) -> Rc<Cell<usize>> {
        self.frame_counters.0.clone()
    }
    fn frames_next_key(&self) -> Rc<Cell<u64>> {
        self.frame_counters.1.clone()
    }

    fn create_frame(&mut self, node: NodeId, signature: Signature) {
        let depth = self.role.as_ref().map_or(0, |role| role.depth);
        if depth + 1 > MAX_DEPTH {
            return self.refuse(node, signature, "frame nesting limit reached");
        }
        let live = self
            .role
            .as_ref()
            .map_or_else(|| self.frames_live(), |r| r.live.clone());
        if live.get() >= MAX_FRAMES {
            return self.refuse(node, signature, "too many frames");
        }
        let next = self
            .role
            .as_ref()
            .map_or_else(|| self.frames_next_key(), |r| r.next_key.clone());
        let key = next.get() + 1;
        next.set(key);
        let parent_sandbox = self
            .role
            .as_ref()
            .map_or(Sandbox::NONE, |role| role.sandbox);
        let sandbox = parent_sandbox.inherit(Sandbox::parse(signature.sandbox.as_deref()));
        let (content, inherits) = match self.frame_content(&signature) {
            Ok(content) => content,
            Err(reason) => return self.refuse(node, signature, &reason),
        };
        let opaque = sandbox.opaque() || self.role.as_ref().is_some_and(|r| r.opaque && inherits);
        let role = self.frame_role(key, sandbox, opaque, inherits);
        let mut limits = self.limits.clone();
        limits.js_heap_bytes = limits.js_heap_bytes.min(64 * 1024 * 1024);
        let view = View::build(
            self.document.borrow().url().clone(),
            self.network.clone(),
            self.hubs.clone(),
            limits,
            Some(role),
        );
        let Ok(mut view) = view else {
            return self.refuse(node, signature, "could not create the frame");
        };
        live.set(live.get() + 1);
        view.permissions = self.permissions.clone();
        let mut child = ChildFrame {
            key,
            node,
            view: Box::new(view),
            signature: Signature::default(),
            epoch: 0,
            loaded_epoch: 0,
            live,
        };
        let child_key = child.key;
        let child_sandbox = child.signature.sandbox.clone();
        self.start_frame_navigation(&mut child, signature, content);
        self.events.push_back(PageEvent::FrameAttached {
            parent: wega_protocol::identity::FrameId(self.role.as_ref().map_or(1, |r| r.frame_id)),
            frame: wega_protocol::identity::FrameId(child_key),
            url: child.view.document.borrow().url().to_string(),
            name: None,
            sandbox: child_sandbox,
        });
        self.frames.children.push(child);
    }

    /// What the frame shows: `(HTML or URL, inherits the creator's origin)`.
    fn frame_content(&self, signature: &Signature) -> Result<(FrameContent, bool), String> {
        if let Some(srcdoc) = &signature.srcdoc {
            if srcdoc.len() > self.limits.html_bytes {
                return Err("srcdoc exceeds the document size limit".into());
            }
            return Ok((FrameContent::Html(srcdoc.clone()), true));
        }
        let src = signature.src.as_deref().map(str::trim).unwrap_or_default();
        if src.is_empty() || src.eq_ignore_ascii_case("about:blank") {
            return Ok((FrameContent::Html(BLANK.into()), true));
        }
        let base = self.document.borrow().url().clone();
        let url = base
            .join(src)
            .map_err(|_| "invalid frame URL".to_string())?;
        let allowed = match url.scheme() {
            "http" | "https" => true,
            "file" => base.scheme() == "file",
            _ => false,
        };
        if !allowed {
            return Err(format!("frame scheme {} is not allowed", url.scheme()));
        }
        if !self.csp().allows_frame(&url, &base) {
            return Err(format!("Content Security Policy frame-src blocked {url}"));
        }
        Ok((FrameContent::Url(url), false))
    }

    fn navigate_frame(&mut self, index: usize, signature: Signature) {
        let (content, inherits) = match self.frame_content(&signature) {
            Ok(content) => content,
            Err(reason) => {
                // The frame keeps its element but shows nothing scriptable.
                let child = &mut self.frames.children[index];
                child.signature = signature;
                child.epoch += 1;
                child.view.navigation_generation = child.epoch;
                child.view.stop_pending();
                let _ = child.view.set_html(&blocked_page(&reason));
                self.notice(format!("iframe not loaded: {reason}"));
                return;
            }
        };
        let parent_sandbox = self
            .role
            .as_ref()
            .map_or(Sandbox::NONE, |role| role.sandbox);
        let sandbox = parent_sandbox.inherit(Sandbox::parse(signature.sandbox.as_deref()));
        let mut child = self.frames.children.remove(index);
        // The sandbox and inherited-origin class can change with the attribute: rebuild the
        // role so a new document is judged against the *new* flags, never the old ones.
        let opaque = sandbox.opaque() || self.role.as_ref().is_some_and(|r| r.opaque && inherits);
        let role = self.frame_role(child.key, sandbox, opaque, inherits);
        child.view.role = Some(role);
        self.start_frame_navigation(&mut child, signature, content);
        self.frames.children.insert(index, child);
    }

    fn start_frame_navigation(
        &mut self,
        child: &mut ChildFrame,
        signature: Signature,
        content: FrameContent,
    ) {
        child.signature = signature;
        child.epoch += 1;
        child.view.navigation_generation = child.epoch;
        let parent_url = self.document.borrow().url().clone();
        match content {
            FrameContent::Html(html) => {
                child.view.stop_pending();
                child.view.url = parent_url;
                if let Err(error) = child.view.set_html(&html) {
                    self.notice(format!("iframe not loaded: {error}"));
                }
            }
            FrameContent::Url(url) => {
                if let Err(error) =
                    child
                        .view
                        .load_frame(url, &parent_url, self.referrer_policy.clone())
                {
                    self.notice(format!("iframe not loaded: {error}"));
                }
            }
        }
    }

    /// Start a frame navigation to `url` with the embedder as initiator.
    pub(crate) fn load_frame(
        &mut self,
        url: Url,
        initiator: &Url,
        referrer_policy: Option<String>,
    ) -> Result<(), String> {
        let opaque = self.role.as_ref().is_some_and(|role| role.opaque);
        let frame_id = self.role.as_ref().map(|r| r.frame_id);
        let parent_frame_id = self.role.as_ref().and_then(|r| r.parent_frame_id);
        let context = FetchContext {
            destination: Destination::Frame,
            initiator: Some(initiator.clone()),
            referrer_policy,
            omit_credentials: opaque,
            frame: frame_id,
            parent_frame: parent_frame_id,
            ..Default::default()
        };
        self.load_request(url, context)
    }

    /// Whether a committed frame response may be shown inside its embedders.
    pub(crate) fn frame_response_allowed(&self, response: &Response) -> Result<(), String> {
        let Some(role) = &self.role else {
            return Ok(());
        };
        let ancestors: Vec<&Url> = role.ancestors.iter().collect();
        let values: Vec<&str> = response
            .headers
            .get_all("x-frame-options")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect();
        if !wega_security::csp::x_frame_options_allows(&values, &ancestors, &response.url) {
            return Err("X-Frame-Options forbids embedding".into());
        }
        let mut csp = Csp::default();
        for value in response.headers.get_all("content-security-policy") {
            if let Ok(value) = value.to_str() {
                csp.add(value);
            }
        }
        if !csp.allows_ancestors(&ancestors, &response.url) {
            return Err("Content Security Policy frame-ancestors forbids embedding".into());
        }
        Ok(())
    }

    fn poll_frames(&mut self) {
        let mut load_events = Vec::new();
        let mut navigations = Vec::new();
        for child in &mut self.frames.children {
            child.view.poll();
            // A frame's own navigation requests (location changes, links) navigate that
            // frame only. The request carries the frame's committed document as initiator.
            while let Some(url) = child.view.next_navigation() {
                navigations.push((child.key, url));
            }
            if child.view.host.load_event_sent
                && child.view.pending.is_none()
                && child.loaded_epoch != child.epoch
            {
                child.loaded_epoch = child.epoch;
                load_events.push(child.node);
            }
        }
        for (key, url) in navigations {
            self.navigate_frame_by_script(key, url);
        }
        for node in load_events {
            self.run(
                &format!(
                    "__wega_dispatch('load',{},new Event('load'))",
                    serde_json::json!(node.as_u64().to_string())
                ),
                "wega:frame-load",
            );
        }
        self.route_frame_messages();
    }

    fn navigate_frame_by_script(&mut self, key: u64, url: Url) {
        let Some(index) = self.frames.children.iter().position(|c| c.key == key) else {
            return;
        };
        let allowed = matches!(url.scheme(), "http" | "https")
            || (url.scheme() == "file" && self.document.borrow().url().scheme() == "file");
        if !allowed {
            return;
        }
        let base = self.document.borrow().url().clone();
        if !self.csp().allows_frame(&url, &base) {
            self.notice(format!("Content Security Policy frame-src blocked {url}"));
            return;
        }
        let mut child = self.frames.children.remove(index);
        let parent_url = base;
        child.epoch += 1;
        child.view.navigation_generation = child.epoch;
        // The signature keeps the element's attributes: the next attribute change navigates.
        if let Err(error) = child
            .view
            .load_frame(url, &parent_url, self.referrer_policy.clone())
        {
            self.notice(format!("iframe not loaded: {error}"));
        }
        self.frames.children.insert(index, child);
    }

    /// Refresh the table the document's script host reads.
    fn publish_frames(&mut self) {
        let mut entries = Vec::with_capacity(self.frames.children.len());
        let mut children = Vec::with_capacity(self.frames.children.len());
        let own_origin = self.origin_string();
        let own_opaque = self.restrictions().opaque;
        for child in &self.frames.children {
            let origin = child.view.origin_string();
            let opaque = child.view.restrictions().opaque;
            let same_origin = !own_opaque && !opaque && origin != "null" && origin == own_origin;
            entries.push(Entry {
                node: child.node,
                key: child.key,
                same_origin,
                name: child.view.frame_name_hint(&self.document, child.node),
                length: child.view.frames.children.len(),
            });
            children.push(ChildRef {
                key: child.key,
                node: child.node,
                origin: origin.clone(),
                opaque,
                document: child.view.document.clone(),
                dom: child.view.dom.clone(),
                document_generation: child.view.document_generation.clone(),
            });
            let parent_ref = ParentRef {
                origin: own_origin.clone(),
                opaque: own_opaque,
                frame_node: child.node,
                document: Rc::downgrade(&self.document),
                dom: self.dom.as_ref().map(Rc::downgrade).unwrap_or_default(),
                document_generation: Rc::downgrade(&self.document_generation),
            };
            child.view.frame_shared.borrow_mut().parent = Some(parent_ref);
        }
        let mut shared = self.frame_shared.borrow_mut();
        shared.entries = entries;
        shared.children = children;
        shared.depth = self.role.as_ref().map_or(0, |role| role.depth);
        shared.has_parent = self.role.is_some();
    }
    fn frame_name_hint(&self, parent: &Rc<RefCell<BlitzDocument>>, node: NodeId) -> String {
        let doc = parent.borrow();
        doc.get_node(node)
            .and_then(|node| node.element_data())
            .and_then(|element| element.attr(LocalName::from("name")).map(str::to_owned))
            .unwrap_or_default()
    }

    /// Route queued `postMessage`/navigation requests: from this view's own script to its
    /// children, and from the children's scripts up to this view (or onwards to the top).
    fn route_frame_messages(&mut self) {
        for _ in 0..4 {
            let own = std::mem::take(&mut self.frame_shared.borrow_mut().outbox);
            let mut up = Vec::new();
            let mut progressed = !own.is_empty();
            let sender = self.origin_string();
            for message in own {
                match message.target {
                    Target::Child(key) => self.deliver_down(key, &sender, message),
                    Target::Parent | Target::Top => up.push(message),
                }
            }
            if !up.is_empty() {
                self.frame_shared.borrow_mut().outbox.extend(up);
            }
            // Collect what children's scripts posted upwards.
            let mut from_children = Vec::new();
            for child in &self.frames.children {
                let mut outbox = std::mem::take(&mut child.view.frame_shared.borrow_mut().outbox);
                let origin = child.view.origin_string();
                outbox.retain_mut(|message| match message.target {
                    Target::Parent | Target::Top => {
                        message.sender.get_or_insert_with(|| origin.clone());
                        from_children.push((child.key, message.clone()));
                        false
                    }
                    Target::Child(_) => true,
                });
                child.view.frame_shared.borrow_mut().outbox.extend(outbox);
            }
            progressed |= !from_children.is_empty();
            for (key, message) in from_children {
                self.deliver_up(key, message);
            }
            if !progressed {
                break;
            }
        }
    }

    fn deliver_down(&mut self, key: u64, sender: &str, message: Outgoing) {
        let Some(index) = self.frames.children.iter().position(|c| c.key == key) else {
            return;
        };
        match message.payload {
            Payload::Message(data) => {
                let child = &mut self.frames.children[index].view;
                if !target_accepts(&message.target_origin, sender, &child.origin_string()) {
                    return;
                }
                child.deliver_frame_message(sender, "{\"kind\":\"parent\"}", &data);
            }
            Payload::Navigate(url) => self.navigate_frame_by_script(key, url),
        }
    }

    fn deliver_up(&mut self, child_key: u64, message: Outgoing) {
        let sender = message.sender.clone().unwrap_or_else(|| "null".into());
        let Payload::Message(data) = message.payload.clone() else {
            return;
        };
        match message.target {
            Target::Parent => {
                if !target_accepts(&message.target_origin, &sender, &self.origin_string()) {
                    return;
                }
                let source = format!("{{\"kind\":\"child\",\"key\":{child_key}}}");
                self.deliver_frame_message(&sender, &source, &data);
            }
            Target::Top => {
                if self.role.is_some() {
                    // Not the top: keep climbing with the original sender origin.
                    self.frame_shared.borrow_mut().outbox.push(message);
                } else {
                    if !target_accepts(&message.target_origin, &sender, &self.origin_string()) {
                        return;
                    }
                    // A non-adjacent sender has no WindowProxy for the receiver: `source` is null.
                    self.deliver_frame_message(&sender, "null", &data);
                }
            }
            Target::Child(_) => {}
        }
    }

    /// Queue a cross-frame message on this view's own realm (a task, never synchronous).
    pub(crate) fn deliver_frame_message(&mut self, origin: &str, source: &str, data: &str) {
        if self.script.is_none() {
            return;
        }
        let call = format!(
            "__wega_frame_message({},{},{})",
            serde_json::json!(origin),
            source,
            serde_json::json!(data)
        );
        self.run(&call, "wega:frame-message");
        self.drain_requests();
    }

    // ---- compositing ----

    /// The content box of child `index` in parent viewport CSS pixels, if it paints.
    fn frame_box(&self, node: NodeId) -> Option<wega_dom::layout::Rect> {
        let doc = self.document.borrow();
        let n = doc.get_node(node)?;
        if n.primary_styles().is_none_or(|style| {
            style.clone_visibility() != style::computed_values::visibility::T::Visible
        }) {
            return None;
        }
        match doc.inspect_geometry(node.key()) {
            wega_dom::layout::Geometry::Current(boxes)
            | wega_dom::layout::Geometry::Stale(boxes) => {
                let content = boxes.content_box();
                (content.width >= 1.0 && content.height >= 1.0).then_some(content)
            }
            _ => None,
        }
    }

    /// Record every visible child into `scene` at its content box.
    pub(crate) fn compose_frames(
        &mut self,
        scene: &mut <BlitzDocument as PaintSource>::Scene,
        viewport: &ViewportConfig,
    ) -> bool {
        let mut another = false;
        let device = (viewport.scale * viewport.zoom) as f64;
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
            // Cull frames wholly outside the viewport.
            if x >= viewport.width_px as f64
                || y >= viewport.height_px as f64
                || x + w <= 0.0
                || y + h <= 0.0
            {
                continue;
            }
            if w * h > child.view.limits.max_pixels as f64 {
                continue;
            }
            let Ok(recorded) = child.view.record_paint(
                w as u32,
                h as u32,
                viewport.scale,
                viewport.zoom,
                viewport.dark,
            ) else {
                continue;
            };
            <BlitzDocument as PaintSource>::compose_child(
                scene,
                recorded,
                wega_dom::render::DeviceRect {
                    x,
                    y,
                    width: w,
                    height: h,
                },
            );
            another |= child.view.frame_wants_more();
        }
        another
    }
    pub(crate) fn frame_wants_more(&self) -> bool {
        self.frame.needs_paint(self.frozen)
    }
    pub(crate) fn frames_need_paint(&self) -> bool {
        self.frames.children.iter().any(|c| c.view.needs_paint())
    }
    pub(crate) fn frames_next_wakeup(&self) -> Option<Duration> {
        self.frames
            .children
            .iter()
            .filter_map(|c| c.view.next_wakeup())
            .min()
    }

    // ---- input ----

    /// The topmost child under a parent-viewport point, with the point converted into the
    /// child's own viewport. Frames are painted in document order, so the last one wins.
    fn frame_at(&self, x: f32, y: f32) -> Option<(usize, f32, f32)> {
        for (index, child) in self.frames.children.iter().enumerate().rev() {
            let Some(content) = self.frame_box(child.node) else {
                continue;
            };
            let (px, py) = (x as f64, y as f64);
            if px >= content.x
                && py >= content.y
                && px < content.x + content.width
                && py < content.y + content.height
            {
                return Some((index, (px - content.x) as f32, (py - content.y) as f32));
            }
        }
        None
    }

    /// Route a pointer event into the frame under it. Returns true when a frame consumed it:
    /// the element behind the frame in the parent never sees it.
    pub(crate) fn route_pointer_to_frame(&mut self, pointer: &Pointer) -> bool {
        if self.frames.children.is_empty() {
            return false;
        }
        let hit = if pointer.kind == PointerKind::Leave {
            None
        } else {
            self.frame_at(pointer.x, pointer.y)
        };
        let hit_key = hit.map(|(index, _, _)| self.frames.children[index].key);
        if self.frames.hovered != hit_key {
            if let Some(previous) = self.frames.hovered
                && let Some(child) = self.child_by_key(previous)
            {
                child.pointer(Pointer::new(PointerKind::Leave, -1.0, -1.0));
            }
            self.frames.hovered = hit_key;
        }
        let Some((index, x, y)) = hit else {
            if pointer.kind == PointerKind::Down {
                // Activating the parent moves keyboard focus back to the parent.
                self.frames.focused = None;
            }
            return false;
        };
        if pointer.kind == PointerKind::Down {
            self.frames.focused = Some(self.frames.children[index].key);
        }
        let mut local = *pointer;
        local.x = x;
        local.y = y;
        self.frames.children[index].view.pointer(local);
        self.frame.invalidate();
        true
    }

    /// Wheel/scroll routing for `scroll_at`.
    pub(crate) fn route_scroll_to_frame(&mut self, x: f64, y: f64, px: f32, py: f32) -> bool {
        let Some((index, lx, ly)) = self.frame_at(px, py) else {
            return false;
        };
        self.frames.children[index].view.scroll_at(x, y, lx, ly);
        self.frame.invalidate();
        true
    }

    /// The focused leaf frame for keyboard input, following nested focus.
    pub(crate) fn focused_child(&mut self) -> Option<&mut View> {
        let key = self.frames.focused?;
        self.child_by_key(key)
    }
}

/// One row of the inspector's frame tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameDescription {
    /// Stable frame identity (never reused in this tab). The top-level document is not listed.
    pub key: u64,
    /// The embedding frame; `None` for children of the top-level document.
    pub parent: Option<u64>,
    pub url: String,
    /// The committed principal: a tuple origin or `null`.
    pub origin: String,
    pub name: String,
    pub sandboxed: bool,
    /// The active document's revision (changes on every navigation of this frame).
    pub revision: u64,
}
/// A frame as a D3 inspector target: its own document stamp and the page port to serve it.
/// The stamp's document id is the frame key, so node handles minted for one frame can never be
/// accepted by another, and the revision invalidates them when the frame navigates.
pub struct InspectedFrame<'a> {
    pub context: wega_protocol::DocumentContext,
    pub loading: bool,
    pub port: &'a mut View,
}
impl View {
    /// Every live descendant frame, parents before children.
    pub fn frame_tree(&self) -> Vec<FrameDescription> {
        fn walk(view: &View, parent: Option<u64>, out: &mut Vec<FrameDescription>) {
            for child in &view.frames.children {
                let v = &child.view;
                out.push(FrameDescription {
                    key: child.key,
                    parent,
                    url: v.document.borrow().url().to_string(),
                    origin: v.origin_string(),
                    name: v.frame_name_hint(&view.document, child.node),
                    sandboxed: v.role.as_ref().is_some_and(|r| r.sandbox.active),
                    revision: v.document_revision(),
                });
                walk(v, Some(child.key), out);
            }
        }
        let mut out = Vec::new();
        walk(self, None, &mut out);
        out
    }
    /// The inspector target for frame `key`, or `None` once it is gone (a stale target never
    /// resolves to a newer frame because keys are not reused).
    pub fn frame_target(&mut self, key: u64) -> Option<InspectedFrame<'_>> {
        if let Some(index) = self.frames.children.iter().position(|c| c.key == key) {
            let view = &mut *self.frames.children[index].view;
            return Some(InspectedFrame {
                context: wega_protocol::DocumentContext {
                    document: wega_protocol::DocumentId(key),
                    revision: wega_protocol::DocumentRevision(view.document_revision()),
                },
                loading: view.loading,
                port: view,
            });
        }
        let index = self
            .frames
            .children
            .iter()
            .position(|c| c.view.contains_frame(key))?;
        self.frames.children[index].view.frame_target(key)
    }
    fn contains_frame(&self, key: u64) -> bool {
        self.frames
            .children
            .iter()
            .any(|c| c.key == key || c.view.contains_frame(key))
    }
}

pub(crate) enum FrameContent {
    Html(String),
    Url(Url),
}

impl crate::dom::Dom {
    /// The document origin scripts and servers see: `null` for an opaque origin.
    pub(crate) fn origin(&self) -> String {
        if self.restrictions.opaque {
            "null".into()
        } else {
            self.document.borrow().url().origin().ascii_serialization()
        }
    }

    /// Frame calls from the script host. They read the table the view publishes and append
    /// to the outbox; they never touch another document.
    pub(crate) fn frame_call(
        &self,
        args: &[serde_json::Value],
    ) -> Result<serde_json::Value, String> {
        use serde_json::{Value, json};
        let text = |index: usize| args.get(index).and_then(Value::as_str).unwrap_or_default();
        let row = |entry: &Entry| {
            json!({"key": entry.key.to_string(), "sameOrigin": entry.same_origin,
                   "name": entry.name, "length": entry.length})
        };
        let mut shared = self.frames.borrow_mut();
        Ok(match text(0) {
            "frameWindow" => {
                let node: u64 = text(1).parse().map_err(|_| "Invalid node identity")?;
                shared
                    .entries
                    .iter()
                    .find(|entry| entry.node.as_u64() == node)
                    .map_or(Value::Null, row)
            }
            "frameDocument" => {
                let caller_origin = self.origin();
                if caller_origin == "null" {
                    return Ok(Value::Null);
                }
                if text(1).starts_with("p:") {
                    // Frame element in child referencing its own document
                    let root_id = self.document.borrow().root_node().id.as_u64();
                    json!(root_id.to_string())
                } else {
                    let node: u64 = text(1).parse().map_err(|_| "Invalid node identity")?;
                    if let Some(child) = shared.children.iter().find(|c| c.node.as_u64() == node) {
                        if !child.opaque && child.origin != "null" && child.origin == caller_origin {
                            let root_id = child.document.borrow().root_node().id.as_u64();
                            let generation = child.document_generation.get();
                            json!(format!("f:{}:{}:{}", child.key, generation, root_id))
                        } else {
                            Value::Null
                        }
                    } else {
                        Value::Null
                    }
                }
            }
            "frameWindowDocument" => {
                let key: u64 = text(1).parse().map_err(|_| "Invalid frame identity")?;
                let caller_origin = self.origin();
                if caller_origin == "null" {
                    return Ok(Value::Null);
                }
                if let Some(child) = shared.children.iter().find(|c| c.key == key) {
                    if !child.opaque && child.origin != "null" && child.origin == caller_origin {
                        let root_id = child.document.borrow().root_node().id.as_u64();
                        let generation = child.document_generation.get();
                        json!(format!("f:{}:{}:{}", child.key, generation, root_id))
                    } else {
                        Value::Null
                    }
                } else {
                    Value::Null
                }
            }
            "frameParentDocument" => {
                let caller_origin = self.origin();
                if caller_origin == "null" {
                    return Ok(Value::Null);
                }
                if let Some(parent) = &shared.parent {
                    if !parent.opaque && parent.origin != "null" && parent.origin == caller_origin {
                        let doc = parent.document.upgrade().ok_or("Parent document detached")?;
                        let gen_rc = parent.document_generation.upgrade().ok_or("Parent document detached")?;
                        let root_id = doc.borrow().root_node().id.as_u64();
                        json!(format!("p:{}:{}", gen_rc.get(), root_id))
                    } else {
                        Value::Null
                    }
                } else {
                    Value::Null
                }
            }
            "frameElement" => {
                let caller_origin = self.origin();
                if caller_origin == "null" {
                    return Ok(Value::Null);
                }
                if let Some(parent) = &shared.parent {
                    if !parent.opaque && parent.origin != "null" && parent.origin == caller_origin {
                        let gen_rc = parent.document_generation.upgrade().ok_or("Parent document detached")?;
                        json!(format!("p:{}:{}", gen_rc.get(), parent.frame_node.as_u64()))
                    } else {
                        Value::Null
                    }
                } else {
                    Value::Null
                }
            }
            "frameList" => Value::Array(shared.entries.iter().map(row).collect()),
            "frameInfo" => {
                let key: u64 = text(1).parse().map_err(|_| "Invalid frame identity")?;
                shared
                    .entries
                    .iter()
                    .find(|entry| entry.key == key)
                    .map_or(Value::Null, row)
            }
            "frameRole" => json!({"hasParent": shared.has_parent, "depth": shared.depth}),
            "framePost" | "frameNavigate" => {
                let navigate = text(0) == "frameNavigate";
                let (kind, key) = text(1).split_once(':').unwrap_or((text(1), ""));
                let target = match kind {
                    "child" => {
                        let key: u64 = key.parse().map_err(|_| "Invalid frame identity")?;
                        // A frame that is gone (or never existed) is silently not a target.
                        if !shared.entries.iter().any(|entry| entry.key == key) {
                            return Ok(Value::Null);
                        }
                        Target::Child(key)
                    }
                    "parent" if shared.has_parent => Target::Parent,
                    "top" if shared.has_parent => Target::Top,
                    _ => return Ok(Value::Null),
                };
                if shared.outbox.len() >= MAX_OUTBOX {
                    return Err("QuotaExceededError: too many pending frame messages".into());
                }
                let payload = if navigate {
                    let url = self
                        .document
                        .borrow()
                        .url()
                        .join(text(2))
                        .map_err(|error| error.to_string())?;
                    Payload::Navigate(url)
                } else {
                    let data = text(3);
                    if data.len() > MAX_MESSAGE_BYTES {
                        return Err("DataCloneError: message is too large".into());
                    }
                    Payload::Message(data.to_owned())
                };
                shared.outbox.push(Outgoing {
                    target,
                    target_origin: text(2).to_owned(),
                    payload,
                    sender: None,
                });
                Value::Null
            }
            other => return Err(format!("Unknown frame call {other}")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sandbox_flags_only_ever_accumulate_restrictions() {
        let parent = Sandbox::parse(Some("allow-scripts"));
        let child = Sandbox::parse(Some("allow-scripts allow-same-origin allow-forms"));
        let effective = parent.inherit(child);
        assert!(effective.scripts && !effective.same_origin && !effective.forms);
        assert!(effective.opaque());
        assert!(!Sandbox::NONE.opaque());
        assert!(Sandbox::NONE.inherit(Sandbox::parse(Some(""))).active);
        assert!(!Sandbox::parse(Some("allow-pointer-lock allow-top-navigation")).popups);
    }
    #[test]
    fn target_origin_is_matched_against_the_receivers_principal() {
        assert!(target_accepts("*", "https://a.test", "null"));
        assert!(target_accepts("/", "https://a.test", "https://a.test"));
        assert!(!target_accepts("/", "null", "null"));
        assert!(!target_accepts("https://a.test", "https://b.test", "null"));
        assert!(!target_accepts("null", "null", "null"));
        assert!(target_accepts("https://a.test", "x", "https://a.test"));
        assert!(!target_accepts("https://a.test", "x", "https://b.test"));
    }
}
