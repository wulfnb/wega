//! Browser-owned browsing contexts. Renderer requests never mint frame IDs,
//! select principals/processes, or turn requested navigation into a commit.
use crate::{
    DocumentContext, DocumentRevision, ErrorCode, ProtocolError,
    identity::{FrameId, ProcessSessionId, SessionIdentity},
};
use std::collections::{BTreeMap, BTreeSet};
use url::Url;

pub const MAX_CONTEXTS: usize = 128;
pub const MAX_DEPTH: usize = 16;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextOrigin {
    Tuple(String),
    /// Nonces are assigned by the coordinator, never the renderer. Inherited
    /// about:blank contexts can share an opaque origin without trusting "null".
    Opaque {
        process: ProcessSessionId,
        frame: FrameId,
        revision: DocumentRevision,
    },
}
impl ContextOrigin {
    pub fn serialized(&self) -> &str {
        match self {
            Self::Tuple(origin) => origin,
            Self::Opaque { .. } => "null",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sandbox {
    pub enabled: bool,
    pub same_origin: bool,
    pub scripts: bool,
    pub forms: bool,
    pub popups: bool,
    pub popups_escape: bool,
    pub top_navigation: bool,
}
impl Sandbox {
    pub fn parse(value: Option<&str>) -> Self {
        let Some(value) = value else {
            return Self::default();
        };
        let tokens: BTreeSet<_> = value
            .split_ascii_whitespace()
            .map(str::to_ascii_lowercase)
            .collect();
        Self {
            enabled: true,
            same_origin: tokens.contains("allow-same-origin"),
            scripts: tokens.contains("allow-scripts"),
            forms: tokens.contains("allow-forms"),
            popups: tokens.contains("allow-popups"),
            popups_escape: tokens.contains("allow-popups-to-escape-sandbox"),
            top_navigation: tokens.contains("allow-top-navigation"),
        }
    }
    pub fn inherited(self, child: Self) -> Self {
        if !self.enabled {
            return child;
        }
        if !child.enabled {
            return self;
        }
        Self {
            enabled: true,
            same_origin: self.same_origin && child.same_origin,
            scripts: self.scripts && child.scripts,
            forms: self.forms && child.forms,
            popups: self.popups && child.popups,
            popups_escape: self.popups_escape && child.popups_escape,
            top_navigation: self.top_navigation && child.top_navigation,
        }
    }
    pub fn scripts_allowed(self) -> bool {
        !self.enabled || self.scripts
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placement {
    SameAgent(FrameId),
    Isolated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextKey {
    pub session: SessionIdentity,
    pub document: DocumentContext,
}

#[derive(Clone, Debug)]
pub struct BrowsingContext {
    pub id: FrameId,
    pub parent: Option<FrameId>,
    pub opener: Option<FrameId>,
    pub element: Option<u64>,
    pub key: ContextKey,
    pub url: Url,
    pub origin: ContextOrigin,
    pub sandbox: Sandbox,
    pub placement: Placement,
    pending: Option<Url>,
    bound: bool,
}

pub struct FrameTree {
    root: FrameId,
    next_id: u64,
    contexts: BTreeMap<FrameId, BrowsingContext>,
}
fn invalid(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidRequest, message)
}
fn stale() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::StaleDocument,
        "Stale or foreign browsing context",
    )
}
fn tuple(url: &Url) -> Option<ContextOrigin> {
    matches!(url.scheme(), "http" | "https")
        .then(|| ContextOrigin::Tuple(url.origin().ascii_serialization()))
}
impl FrameTree {
    pub fn new(key: ContextKey, url: Url) -> Result<Self, ProtocolError> {
        key.session.validate()?;
        let root = key.session.frame;
        let origin = tuple(&url).unwrap_or(ContextOrigin::Opaque {
            process: key.session.process,
            frame: root,
            revision: key.document.revision,
        });
        let context = BrowsingContext {
            id: root,
            parent: None,
            opener: None,
            element: None,
            key,
            url,
            origin,
            sandbox: Sandbox::default(),
            placement: Placement::SameAgent(root),
            pending: None,
            bound: true,
        };
        Ok(Self {
            root,
            next_id: root
                .0
                .checked_add(1)
                .ok_or_else(|| invalid("Frame IDs exhausted"))?,
            contexts: [(root, context)].into(),
        })
    }
    pub fn root(&self) -> FrameId {
        self.root
    }
    pub fn get(&self, id: FrameId) -> Option<&BrowsingContext> {
        self.contexts.get(&id)
    }
    pub fn validate(&self, key: ContextKey) -> Result<&BrowsingContext, ProtocolError> {
        self.get(key.session.frame)
            .filter(|frame| frame.bound && frame.key == key)
            .ok_or_else(stale)
    }
    pub fn children(&self, parent: FrameId) -> impl Iterator<Item = &BrowsingContext> {
        self.contexts
            .values()
            .filter(move |context| context.parent == Some(parent))
    }
    pub fn can_access(&self, source: ContextKey, target: FrameId) -> Result<bool, ProtocolError> {
        let source = self.validate(source)?;
        let target = self
            .get(target)
            .filter(|target| target.bound)
            .ok_or_else(stale)?;
        Ok(source.origin == target.origin)
    }
    fn origin_for(
        &self,
        parent: &BrowsingContext,
        id: FrameId,
        url: &Url,
        sandbox: Sandbox,
    ) -> ContextOrigin {
        if sandbox.enabled && !sandbox.same_origin {
            return ContextOrigin::Opaque {
                process: parent.key.session.process,
                frame: id,
                revision: parent.key.document.revision,
            };
        }
        if url.as_str() == "about:blank" || url.as_str() == "about:srcdoc" {
            return parent.origin.clone();
        }
        tuple(url).unwrap_or(ContextOrigin::Opaque {
            process: parent.key.session.process,
            frame: id,
            revision: parent.key.document.revision,
        })
    }
    pub fn attach(
        &mut self,
        parent: ContextKey,
        element: u64,
        requested: &str,
        sandbox: Sandbox,
    ) -> Result<FrameId, ProtocolError> {
        if element == 0 {
            return Err(invalid("Invalid frame element"));
        }
        let parent = self.validate(parent)?.clone();
        if self.contexts.len() >= MAX_CONTEXTS {
            return Err(invalid("Frame admission limit exceeded"));
        }
        if self
            .children(parent.id)
            .any(|frame| frame.element == Some(element))
        {
            return Err(invalid("Frame element already attached"));
        }
        let mut depth = 1;
        let mut ancestor = parent.parent;
        while let Some(id) = ancestor {
            depth += 1;
            ancestor = self.get(id).ok_or_else(stale)?.parent;
        }
        if depth > MAX_DEPTH {
            return Err(invalid("Frame nesting limit exceeded"));
        }
        let url = parent
            .url
            .join(requested)
            .map_err(|_| invalid("Invalid frame URL"))?;
        if !matches!(url.scheme(), "http" | "https" | "about" | "data")
            || (url.scheme() == "about" && !matches!(url.path(), "blank" | "srcdoc"))
        {
            return Err(invalid("Frame scheme is not supported"));
        }
        let id = FrameId(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| invalid("Frame IDs exhausted"))?;
        let sandbox = parent.sandbox.inherited(sandbox);
        let origin = self.origin_for(&parent, id, &url, sandbox);
        let placement = if origin == parent.origin {
            Placement::SameAgent(parent.id)
        } else {
            Placement::Isolated
        };
        self.contexts.insert(
            id,
            BrowsingContext {
                id,
                parent: Some(parent.id),
                opener: None,
                element: Some(element),
                key: ContextKey {
                    session: SessionIdentity {
                        frame: id,
                        ..parent.key.session
                    },
                    document: parent.key.document,
                },
                url,
                origin,
                sandbox,
                placement,
                pending: None,
                bound: false,
            },
        );
        Ok(id)
    }
    /// The coordinator binds a real admitted process, not a renderer-selected PID.
    pub fn bind_process(&mut self, id: FrameId, key: ContextKey) -> Result<(), ProtocolError> {
        key.session.validate()?;
        let context = self.get(id).ok_or_else(stale)?;
        if context.bound {
            return Err(invalid("Context is already bound"));
        }
        let parent = self
            .get(context.parent.ok_or_else(stale)?)
            .ok_or_else(stale)?;
        if key.session.service != parent.key.session.service
            || key.document.document.0 == 0
            || key.document.revision.0 == 0
        {
            return Err(stale());
        }
        if matches!(context.placement, Placement::SameAgent(_))
            && key.session.process != parent.key.session.process
        {
            return Err(invalid("Same-origin contexts must share their agent"));
        }
        let context = self.contexts.get_mut(&id).unwrap();
        if key.session.frame != id || key.session.tab != context.key.session.tab {
            return Err(stale());
        }
        if context.placement == Placement::Isolated
            && key.session.process == context.key.session.process
        {
            return Err(invalid(
                "An isolated frame requires its own content process",
            ));
        }
        context.key = key;
        context.bound = true;
        Ok(())
    }
    pub fn begin_navigation(
        &mut self,
        source: ContextKey,
        target: FrameId,
        requested: &str,
    ) -> Result<Url, ProtocolError> {
        let source = self.validate(source)?;
        let context = self.get(target).ok_or_else(stale)?;
        if source.id != target
            && context.parent != Some(source.id)
            && source.origin != context.origin
        {
            return Err(invalid("Frame navigation is not authorized"));
        }
        let url = context
            .url
            .join(requested)
            .map_err(|_| invalid("Invalid navigation URL"))?;
        if !matches!(url.scheme(), "http" | "https" | "about" | "data")
            || (url.scheme() == "about" && !matches!(url.path(), "blank" | "srcdoc"))
        {
            return Err(invalid("Frame scheme is not supported"));
        }
        let mut ancestor = source.parent;
        while let Some(id) = ancestor {
            if id == target && source.sandbox.enabled && !source.sandbox.top_navigation {
                return Err(invalid("Sandbox blocks ancestor navigation"));
            }
            ancestor = self.get(id).ok_or_else(stale)?.parent;
        }
        self.contexts.get_mut(&target).unwrap().pending = Some(url.clone());
        Ok(url)
    }
    /// Only a host-observed commit may change the origin. A pending request is
    /// never exposed as committed data, and navigation keeps the frame's ID.
    pub fn commit(
        &mut self,
        target: FrameId,
        observed: Url,
        key: ContextKey,
    ) -> Result<Vec<FrameId>, ProtocolError> {
        key.session.validate()?;
        let old = self
            .get(target)
            .filter(|context| context.bound)
            .ok_or_else(stale)?
            .clone();
        if old.pending.as_ref() != Some(&observed)
            || key.session.frame != target
            || key.session.tab != old.key.session.tab
            || key.document.revision <= old.key.document.revision
            || key.session.service != old.key.session.service
            || key.document.document.0 == 0
        {
            return Err(stale());
        }
        let parent = old
            .parent
            .and_then(|parent| self.get(parent))
            .unwrap_or(&old);
        let mut origin = self.origin_for(parent, target, &observed, old.sandbox);
        if matches!(origin, ContextOrigin::Opaque { .. })
            && !(matches!(observed.as_str(), "about:blank" | "about:srcdoc")
                && (!old.sandbox.enabled || old.sandbox.same_origin))
        {
            origin = ContextOrigin::Opaque {
                process: key.session.process,
                frame: target,
                revision: key.document.revision,
            };
        }
        let placement = if origin == parent.origin {
            Placement::SameAgent(parent.id)
        } else {
            Placement::Isolated
        };
        if placement == Placement::Isolated && key.session.process == parent.key.session.process {
            return Err(invalid("Cross-origin commit requires isolated placement"));
        }
        let removed = self.remove_descendants(target);
        let context = self.contexts.get_mut(&target).unwrap();
        context.key = key;
        context.url = observed;
        context.origin = origin;
        context.placement = placement;
        context.pending = None;
        Ok(removed)
    }
    pub fn detach(
        &mut self,
        parent: ContextKey,
        target: FrameId,
    ) -> Result<Vec<FrameId>, ProtocolError> {
        let parent = self.validate(parent)?;
        if self.get(target).ok_or_else(stale)?.parent != Some(parent.id) {
            return Err(invalid("Frame detach is not authorized"));
        }
        let mut removed = self.remove_descendants(target);
        self.contexts.remove(&target);
        removed.push(target);
        Ok(removed)
    }
    fn remove_descendants(&mut self, parent: FrameId) -> Vec<FrameId> {
        let children: Vec<_> = self.children(parent).map(|frame| frame.id).collect();
        let mut removed = Vec::new();
        for id in children {
            removed.extend(self.remove_descendants(id));
            self.contexts.remove(&id);
            removed.push(id);
        }
        removed
    }
    pub fn message(
        &self,
        source: ContextKey,
        target: FrameId,
        target_origin: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<PostedMessage, ProtocolError> {
        let source = self.validate(source)?;
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(invalid("Structured message exceeds its byte budget"));
        }
        let receiver = self
            .get(target)
            .filter(|context| context.bound)
            .ok_or_else(stale)?;
        let target_origin = match target_origin.unwrap_or("/") {
            "*" => MessageTarget::Any,
            "/" => MessageTarget::Origin(source.origin.clone()),
            value => MessageTarget::Origin(
                tuple(&Url::parse(value).map_err(|_| invalid("Invalid targetOrigin"))?)
                    .ok_or_else(|| invalid("Opaque targetOrigin requires wildcard"))?,
            ),
        };
        Ok(PostedMessage {
            source: source.id,
            source_key: source.key,
            target_key: receiver.key,
            source_origin: source.origin.clone(),
            target,
            target_origin,
            bytes,
        })
    }
    pub fn accepts_message(&self, message: &PostedMessage) -> bool {
        self.get(message.source).is_some_and(|source| {
            source.key == message.source_key && source.origin == message.source_origin
        }) && self.get(message.target).is_some_and(|target| {
            target.key == message.target_key
                && match &message.target_origin {
                    MessageTarget::Any => true,
                    MessageTarget::Origin(origin) => &target.origin == origin,
                }
        })
    }
}
#[derive(Clone, Debug)]
pub enum MessageTarget {
    Any,
    Origin(ContextOrigin),
}
#[derive(Clone, Debug)]
pub struct PostedMessage {
    pub source: FrameId,
    pub source_key: ContextKey,
    pub target_key: ContextKey,
    pub source_origin: ContextOrigin,
    pub target: FrameId,
    pub target_origin: MessageTarget,
    pub bytes: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DocumentId,
        identity::{PartitionId, ServiceIdentity, ServiceSessionId, TabId},
    };
    fn key(frame: u64, process: u8, revision: u64) -> ContextKey {
        ContextKey {
            session: SessionIdentity {
                tab: TabId(1),
                frame: FrameId(frame),
                process: ProcessSessionId([process; 16]),
                service: Some(ServiceIdentity {
                    service: ServiceSessionId([7; 16]),
                    partition: PartitionId([8; 16]),
                }),
            },
            document: DocumentContext {
                document: DocumentId(frame),
                revision: DocumentRevision(revision),
            },
        }
    }
    fn tree() -> FrameTree {
        FrameTree::new(
            key(1, 1, 1),
            Url::parse("https://parent.test/index").unwrap(),
        )
        .unwrap()
    }
    fn child(tree: &mut FrameTree, element: u64, url: &str, sandbox: Sandbox) -> ContextKey {
        let id = tree.attach(key(1, 1, 1), element, url, sandbox).unwrap();
        let process = if tree.get(id).unwrap().placement == Placement::Isolated {
            2
        } else {
            1
        };
        let child = key(id.0, process, 1);
        tree.bind_process(id, child).unwrap();
        child
    }
    #[test]
    fn identity_admission_and_opaque_origins_are_not_renderer_strings() {
        let mut tree = tree();
        let same = child(&mut tree, 5, "about:blank", Sandbox::default());
        assert!(tree.can_access(key(1, 1, 1), same.session.frame).unwrap());
        let opaque = child(
            &mut tree,
            6,
            "about:srcdoc",
            Sandbox::parse(Some("allow-scripts")),
        );
        let second = child(
            &mut tree,
            7,
            "about:srcdoc",
            Sandbox::parse(Some("allow-scripts")),
        );
        assert_eq!(
            tree.get(opaque.session.frame).unwrap().origin.serialized(),
            "null"
        );
        assert_ne!(
            tree.get(opaque.session.frame).unwrap().origin,
            tree.get(second.session.frame).unwrap().origin
        );
        assert!(!tree.can_access(opaque, second.session.frame).unwrap());
        assert!(!tree.can_access(key(1, 1, 1), opaque.session.frame).unwrap());
        let mut forged = opaque;
        forged.session.process = ProcessSessionId([9; 16]);
        assert!(tree.can_access(forged, same.session.frame).is_err());
        assert!(
            tree.attach(key(1, 1, 0), 8, "about:blank", Sandbox::default())
                .is_err()
        );
        assert!(
            tree.attach(key(1, 1, 1), 5, "about:blank", Sandbox::default())
                .is_err()
        );
    }
    #[test]
    fn provisional_contexts_and_foreign_partitions_do_not_grant_access() {
        let mut tree = tree();
        let id = tree
            .attach(key(1, 1, 1), 2, "https://foreign.test", Sandbox::default())
            .unwrap();
        assert!(tree.can_access(key(1, 1, 1), id).is_err());
        assert!(tree.bind_process(id, key(id.0, 1, 1)).is_err());
        let mut foreign = key(id.0, 2, 1);
        foreign.session.service = None;
        assert!(tree.bind_process(id, foreign).is_err());
        tree.bind_process(id, key(id.0, 2, 1)).unwrap();
        assert!(tree.bind_process(id, key(id.0, 3, 1)).is_err());
        assert!(!tree.can_access(key(1, 1, 1), id).unwrap());
    }
    #[test]
    fn requested_navigation_never_commits_and_detach_revokes_descendants() {
        let mut tree = tree();
        let same = child(&mut tree, 2, "about:blank", Sandbox::default());
        let grandchild = tree
            .attach(same, 3, "about:blank", Sandbox::default())
            .unwrap();
        tree.bind_process(grandchild, key(grandchild.0, 1, 1))
            .unwrap();
        let requested = tree
            .begin_navigation(key(1, 1, 1), same.session.frame, "https://foreign.test/new")
            .unwrap();
        assert_eq!(
            tree.get(same.session.frame).unwrap().url.as_str(),
            "about:blank"
        );
        assert!(
            tree.commit(
                same.session.frame,
                requested.clone(),
                key(same.session.frame.0, 1, 2)
            )
            .is_err()
        );
        let new = key(same.session.frame.0, 2, 2);
        assert_eq!(
            tree.commit(same.session.frame, requested, new).unwrap(),
            vec![grandchild]
        );
        assert!(tree.validate(same).is_err());
        assert!(!tree.can_access(key(1, 1, 1), same.session.frame).unwrap());
        assert_eq!(
            tree.detach(key(1, 1, 1), same.session.frame).unwrap(),
            vec![same.session.frame]
        );
        assert!(tree.validate(new).is_err());
        let replacement = tree
            .attach(key(1, 1, 1), 2, "about:blank", Sandbox::default())
            .unwrap();
        assert!(replacement.0 > same.session.frame.0);
    }
    #[test]
    fn messages_authenticate_both_documents_and_normalize_target_origin() {
        let mut tree = tree();
        let receiver = child(&mut tree, 2, "about:blank", Sandbox::default());
        let message = tree
            .message(
                key(1, 1, 1),
                receiver.session.frame,
                Some("https://parent.test:443/path"),
                vec![1, 2],
            )
            .unwrap();
        assert!(tree.accepts_message(&message));
        let mismatch = tree
            .message(
                key(1, 1, 1),
                receiver.session.frame,
                Some("https://other.test"),
                vec![],
            )
            .unwrap();
        assert!(!tree.accepts_message(&mismatch));
        assert!(
            tree.message(
                key(1, 1, 1),
                receiver.session.frame,
                Some("relative"),
                vec![]
            )
            .is_err()
        );
        assert!(
            tree.message(
                key(1, 1, 1),
                receiver.session.frame,
                None,
                vec![0; MAX_MESSAGE_BYTES + 1]
            )
            .is_err()
        );
        let next = tree
            .begin_navigation(
                key(1, 1, 1),
                receiver.session.frame,
                "https://parent.test/new",
            )
            .unwrap();
        tree.commit(
            receiver.session.frame,
            next,
            key(receiver.session.frame.0, 1, 2),
        )
        .unwrap();
        assert!(
            !tree.accepts_message(&message),
            "queued messages cannot reach a replacement document"
        );
    }
    #[test]
    fn sandbox_inheritance_cannot_relax_ancestors_or_enable_top_navigation() {
        let mut tree = tree();
        let sandbox = Sandbox::parse(Some("allow-same-origin allow-scripts"));
        let first = child(&mut tree, 2, "about:blank", sandbox);
        let nested = tree
            .attach(
                first,
                3,
                "about:blank",
                Sandbox::parse(Some("allow-same-origin allow-scripts allow-top-navigation")),
            )
            .unwrap();
        tree.bind_process(nested, key(nested.0, 1, 1)).unwrap();
        assert!(!tree.get(nested).unwrap().sandbox.top_navigation);
        assert!(
            tree.begin_navigation(key(nested.0, 1, 1), FrameId(1), "https://parent.test/new")
                .is_err()
        );
        assert!(
            tree.begin_navigation(key(1, 1, 1), first.session.frame, "about:config")
                .is_err()
        );
        assert!(
            tree.begin_navigation(key(1, 1, 1), first.session.frame, "javascript:alert(1)")
                .is_err()
        );
    }
    #[test]
    fn frame_depth_and_population_have_explicit_bounds() {
        let mut tree = tree();
        let mut parent = key(1, 1, 1);
        for index in 0..MAX_DEPTH {
            let id = tree
                .attach(
                    parent,
                    (index + 1) as u64,
                    "about:blank",
                    Sandbox::default(),
                )
                .unwrap();
            parent = key(id.0, 1, 1);
            tree.bind_process(id, parent).unwrap();
        }
        assert!(
            tree.attach(parent, 999, "about:blank", Sandbox::default())
                .is_err()
        );
        let mut tree =
            FrameTree::new(key(1, 1, 1), Url::parse("https://parent.test").unwrap()).unwrap();
        for index in 1..MAX_CONTEXTS {
            child(&mut tree, index as u64, "about:blank", Sandbox::default());
        }
        assert!(
            tree.attach(key(1, 1, 1), 999, "about:blank", Sandbox::default())
                .is_err()
        );
    }
}
