//! The only mapping from Blitz's document onto the Wega document contract (`wega-dom`).
#![forbid(unsafe_code)]
use blitz_dom::{
    BaseDocument, DocumentConfig, DocumentMutator, Node, NodeId, node::Attribute, node::NodeData,
};
use blitz_html::HtmlDocument;
use blitz_traits::{
    navigation::NavigationProvider,
    net::{AbortSignal, NetProvider},
};
use html5ever::QualName;
use url::Url;
use wega_dom::input::{FocusState, HoverChange, InputControl};
use wega_dom::{
    DocumentMutation, DocumentView, MutationError, Mutator, NewAttribute, NodeKey, NodeKind,
    NodeView,
};

/// `NodeKey` and Blitz's `NodeId` are both foreign to this module's eventual crate, so they
/// cannot implement `From` for each other; these explicit conversions replace that. Both are
/// the same `u64`, so they compile to nothing.
pub trait ToKey {
    fn key(self) -> NodeKey;
}
pub trait ToNodeId {
    fn node_id(self) -> NodeId;
}
impl ToKey for NodeId {
    #[inline(always)]
    fn key(self) -> NodeKey {
        NodeKey::from_u64(self.as_u64())
    }
}
impl ToNodeId for NodeKey {
    #[inline(always)]
    fn node_id(self) -> NodeId {
        NodeId::from_u64(self.as_u64())
    }
}

/// The document the engine owns: Blitz's `BaseDocument`, held by value (no second tree, no
/// extra allocation or indirection) in a local type so the Wega contract can be implemented
/// for it. It dereferences to `BaseDocument`, so the layout, paint, style and form APIs that
/// are not behind the contract yet stay reachable.
pub struct BlitzDocument(BaseDocument);
impl BlitzDocument {
    pub fn new(document: BaseDocument) -> Self {
        Self(document)
    }
    pub fn into_inner(self) -> BaseDocument {
        self.0
    }
}
impl From<BaseDocument> for BlitzDocument {
    fn from(document: BaseDocument) -> Self {
        Self(document)
    }
}
impl std::ops::Deref for BlitzDocument {
    type Target = BaseDocument;
    #[inline(always)]
    fn deref(&self) -> &BaseDocument {
        &self.0
    }
}
impl std::ops::DerefMut for BlitzDocument {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut BaseDocument {
        &mut self.0
    }
}

/// What the engine decides about a new document. The adapter turns it into Blitz's own
/// configuration, so the engine neither builds a `DocumentConfig` nor chooses how a
/// `BaseDocument` is constructed.
#[derive(Default)]
pub struct DocumentSetup {
    /// The URL relative references resolve against.
    pub base_url: Option<String>,
    /// Font file bytes supplied by the browser process (brokered fonts). `None` uses the
    /// system fonts.
    pub fonts: Option<std::sync::Arc<Vec<Vec<u8>>>>,
    /// Cancelled when the document is replaced, to stop its in-flight resource loads.
    pub abort_signal: Option<AbortSignal>,
    pub net_provider: Option<std::sync::Arc<dyn NetProvider>>,
    pub navigation_provider: Option<std::sync::Arc<dyn NavigationProvider>>,
}
impl DocumentSetup {
    fn config(self) -> DocumentConfig {
        DocumentConfig {
            font_ctx: self
                .fonts
                .as_ref()
                .map(|fonts| blitz_dom::build_broker_font_ctx(fonts)),
            base_url: self.base_url,
            abort_signal: self.abort_signal,
            net_provider: self.net_provider,
            navigation_provider: self.navigation_provider,
            ..Default::default()
        }
    }
}
impl BlitzDocument {
    /// A document parsed from `html` in one pass (the engine's initial blank page).
    pub fn from_html(html: &str, setup: DocumentSetup) -> Self {
        Self(HtmlDocument::from_html(html, setup.config()).into_inner())
    }
    /// A document parsed from XML in one pass.
    pub fn from_xml(xml: &str, setup: DocumentSetup) -> Self {
        Self(HtmlDocument::from_xml(xml, setup.config()).into_inner())
    }
    /// An empty document that the incremental HTML parser then fills.
    pub fn empty(setup: DocumentSetup) -> Self {
        Self(BaseDocument::new(setup.config()))
    }
}

/// A borrowed node of a [`BlitzDocument`].
#[derive(Clone, Copy)]
pub struct BlitzNode<'a>(&'a Node);
impl<'a> BlitzNode<'a> {
    /// The underlying Blitz node, for the properties that are not behind the contract yet.
    pub fn blitz(self) -> &'a Node {
        self.0
    }
}

impl DocumentView for BlitzDocument {
    type Node<'a> = BlitzNode<'a>;
    #[inline]
    fn node(&self, key: NodeKey) -> Option<BlitzNode<'_>> {
        self.0.get_node(key.node_id()).map(BlitzNode)
    }
    fn element_by_id(&self, id: &str) -> Option<NodeKey> {
        self.0.get_element_by_id(id).map(ToKey::key)
    }
    fn root(&self) -> NodeKey {
        self.0.root_node().id.key()
    }
    fn base_url(&self) -> &Url {
        self.0.url()
    }
}

impl<'a> NodeView<'a> for BlitzNode<'a> {
    #[inline]
    fn kind(self) -> NodeKind {
        match &self.0.data {
            NodeData::Document(_) => NodeKind::Document,
            NodeData::Element(_) => NodeKind::Element,
            NodeData::AnonymousBlock(_) => NodeKind::AnonymousBlock,
            NodeData::Text(_) => NodeKind::Text,
            NodeData::Comment { .. } => NodeKind::Comment,
        }
    }
    #[inline]
    fn qual_name(self) -> Option<&'a QualName> {
        match &self.0.data {
            NodeData::Element(e) | NodeData::AnonymousBlock(e) => Some(&e.name),
            _ => None,
        }
    }
    #[inline]
    fn attr(self, name: &str) -> Option<&'a str> {
        let element = self.0.element_data()?;
        element
            .attrs
            .iter()
            .find(|attr| &*attr.name.local == name)
            .map(|attr| &*attr.value)
    }
    #[inline(always)]
    fn parent(self) -> Option<NodeKey> {
        self.0.parent.map(ToKey::key)
    }
    #[inline]
    fn children(self) -> impl ExactSizeIterator<Item = NodeKey> + DoubleEndedIterator + 'a {
        self.0.children.iter().copied().map(ToKey::key)
    }
    #[inline]
    fn child_at(self, index: usize) -> Option<NodeKey> {
        self.0.children.get(index).copied().map(ToKey::key)
    }
    fn attributes(self) -> impl Iterator<Item = (&'a QualName, &'a str)> + 'a {
        self.0
            .element_data()
            .into_iter()
            .flat_map(|element| element.attrs.iter())
            .map(|attr| (&attr.name, &*attr.value))
    }
}

impl DocumentMutation for BlitzDocument {
    type Mutator<'a> = BlitzMutator<'a>;
    fn mutate(&mut self) -> BlitzMutator<'_> {
        BlitzMutator(BaseDocument::mutate(&mut self.0))
    }
    fn refresh_style_element(&mut self, key: NodeKey) {
        let id = key.node_id();
        if self.0.get_node(id).is_some() {
            self.0.process_style_element(id);
        }
    }
}

impl FocusState for BlitzDocument {
    fn focused(&self) -> Option<NodeKey> {
        // Blitz reports the root element when nothing is focused; that fallback is the
        // keyboard target, not focus.
        let focused = self.0.get_focussed_node_id()?;
        let root = self.0.try_root_element().map(|element| element.id);
        (Some(focused) != root).then(|| focused.key())
    }
    fn keyboard_target(&self) -> Option<NodeKey> {
        self.0.get_focussed_node_id().map(ToKey::key)
    }
    fn hovered(&self) -> Option<NodeKey> {
        self.0
            .get_hover_node_id()
            .filter(|id| self.0.get_node(*id).is_some())
            .map(ToKey::key)
    }
}
impl InputControl for BlitzDocument {
    fn set_focus(&mut self, key: NodeKey) -> Result<bool, MutationError> {
        if self.0.get_node(key.node_id()).is_none() {
            return Err(MutationError::Stale(key));
        }
        Ok(self.0.set_focus_to(key.node_id()))
    }
    fn clear_focus(&mut self) {
        self.0.clear_focus();
    }
    fn pointer_moved(&mut self, x: f32, y: f32) -> HoverChange {
        let previous = self.hovered();
        self.0.set_hover_to(x, y);
        HoverChange {
            previous,
            current: self.hovered(),
        }
    }
    fn pointer_left(&mut self) -> HoverChange {
        let previous = self.hovered();
        self.0.clear_hover();
        HoverChange {
            previous,
            current: self.hovered(),
        }
    }
}

/// Blitz's `DocumentMutator` behind the Wega mutation contract. It has no `Drop` of its
/// own: dropping it drops the inner mutator, which runs Blitz's deferred work.
pub struct BlitzMutator<'a>(DocumentMutator<'a>);

fn attributes(attrs: impl IntoIterator<Item = NewAttribute>) -> Vec<Attribute> {
    attrs
        .into_iter()
        .map(|attr| Attribute {
            name: attr.name,
            value: attr.value,
        })
        .collect()
}

impl BlitzMutator<'_> {
    fn node(&self, key: NodeKey) -> Result<&Node, MutationError> {
        self.0
            .doc
            .get_node(key.node_id())
            .ok_or(MutationError::Stale(key))
    }
    fn live(&self, key: NodeKey) -> Result<NodeId, MutationError> {
        self.node(key).map(|_| key.node_id())
    }
    fn element(&self, key: NodeKey) -> Result<NodeId, MutationError> {
        match BlitzNode(self.node(key)?).kind() {
            NodeKind::Element | NodeKind::AnonymousBlock => Ok(key.node_id()),
            _ => Err(MutationError::NotElement(key)),
        }
    }
    fn container(&self, key: NodeKey) -> Result<NodeId, MutationError> {
        match BlitzNode(self.node(key)?).kind() {
            NodeKind::Text | NodeKind::Comment => Err(MutationError::InvalidParent(key)),
            _ => Ok(key.node_id()),
        }
    }
    /// Rejects stale children and any child that is `parent` or one of its ancestors.
    /// A childless node can only be an ancestor of `parent` by being `parent` itself, so the
    /// ancestor walk runs only for children that have children.
    fn check_children(&self, parent: NodeKey, children: &[NodeKey]) -> Result<(), MutationError> {
        for &child in children {
            let node = self.node(child)?;
            let cycle =
                child == parent || (!node.children.is_empty() && self.is_ancestor(child, parent));
            if cycle {
                return Err(MutationError::HierarchyCycle { parent, child });
            }
        }
        Ok(())
    }
    /// Whether `ancestor` is a proper ancestor of `node`.
    fn is_ancestor(&self, ancestor: NodeKey, node: NodeKey) -> bool {
        let mut current = self.0.doc.get_node(node.node_id()).and_then(|n| n.parent);
        while let Some(id) = current {
            if id.key() == ancestor {
                return true;
            }
            current = self.0.doc.get_node(id).and_then(|n| n.parent);
        }
        false
    }
}

/// Converts keys for Blitz; the common single-child case does not allocate.
fn with_ids<R>(children: &[NodeKey], f: impl FnOnce(&[NodeId]) -> R) -> R {
    match children {
        [] => f(&[]),
        [one] => f(&[one.node_id()]),
        many => {
            let ids: Vec<NodeId> = many.iter().copied().map(ToNodeId::node_id).collect();
            f(&ids)
        }
    }
}

impl Mutator for BlitzMutator<'_> {
    fn parent(&self, key: NodeKey) -> Option<NodeKey> {
        self.node(key).ok()?.parent.map(ToKey::key)
    }
    fn last_child(&self, key: NodeKey) -> Option<NodeKey> {
        self.node(key)
            .ok()?
            .children
            .last()
            .copied()
            .map(ToKey::key)
    }
    fn previous_sibling(&self, key: NodeKey) -> Option<NodeKey> {
        self.node(key).ok()?;
        self.0.previous_sibling_id(key.node_id()).map(ToKey::key)
    }

    fn create_element(
        &mut self,
        name: QualName,
        attrs: impl IntoIterator<Item = NewAttribute>,
    ) -> NodeKey {
        (self.0.create_element(name, attributes(attrs))).key()
    }
    fn create_text(&mut self, text: &str) -> NodeKey {
        (self.0.create_text_node(text)).key()
    }
    fn create_comment(&mut self, text: &str) -> NodeKey {
        (self.0.create_comment_node(text)).key()
    }
    fn template_contents(&mut self, template: NodeKey) -> Result<NodeKey, MutationError> {
        let id = self.element(template)?;
        Ok((self.0.template_contents(id)).key())
    }

    fn append_children(
        &mut self,
        parent: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError> {
        let parent_id = self.container(parent)?;
        self.check_children(parent, children)?;
        with_ids(children, |ids| self.0.append_children(parent_id, ids));
        Ok(())
    }
    fn insert_before(
        &mut self,
        anchor: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError> {
        let anchor_id = self.live(anchor)?;
        let parent = self.parent(anchor).ok_or(MutationError::Detached(anchor))?;
        self.check_children(parent, children)?;
        with_ids(children, |ids| self.0.insert_nodes_before(anchor_id, ids));
        Ok(())
    }
    fn replace_children(
        &mut self,
        parent: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError> {
        let parent_id = self.container(parent)?;
        self.check_children(parent, children)?;
        with_ids(children, |ids| self.0.replace_children(parent_id, ids));
        Ok(())
    }
    fn clone_node(&mut self, key: NodeKey, deep: bool) -> Result<NodeKey, MutationError> {
        use blitz_dom::node::NodeData;
        let id = self.live(key)?;
        if deep {
            return Ok((self.0.deep_clone_node(id)).key());
        }
        let cloned = match &self.node(key)?.data {
            NodeData::Element(e) | NodeData::AnonymousBlock(e) => {
                let (name, attrs) = (e.name.clone(), e.attrs.to_vec());
                self.0.create_element(name, attrs)
            }
            NodeData::Text(t) => {
                let text = t.content.to_string();
                self.0.create_text_node(&text)
            }
            NodeData::Comment { contents } => {
                let text = contents.clone();
                self.0.create_comment_node(&text)
            }
            _ => return Err(MutationError::NotCloneable(key)),
        };
        Ok(cloned.key())
    }
    fn remove(&mut self, key: NodeKey) -> Result<(), MutationError> {
        let id = self.live(key)?;
        self.0.remove_node(id);
        Ok(())
    }
    fn remove_and_drop(&mut self, key: NodeKey) -> Result<(), MutationError> {
        let id = self.live(key)?;
        self.0.remove_and_drop_node(id);
        Ok(())
    }
    fn reparent_children(&mut self, from: NodeKey, to: NodeKey) -> Result<(), MutationError> {
        let from_id = self.container(from)?;
        let to_id = self.container(to)?;
        if from == to || self.is_ancestor(from, to) {
            return Err(MutationError::HierarchyCycle {
                parent: to,
                child: from,
            });
        }
        self.0.reparent_children(from_id, to_id);
        Ok(())
    }

    fn set_text(&mut self, node: NodeKey, text: &str) -> Result<(), MutationError> {
        let id = self.live(node)?;
        if BlitzNode(self.node(node)?).kind() != NodeKind::Text {
            return Err(MutationError::NotText(node));
        }
        self.0.set_node_text(id, text);
        Ok(())
    }
    fn append_text(&mut self, node: NodeKey, text: &str) -> Result<(), MutationError> {
        let id = self.live(node)?;
        if BlitzNode(self.node(node)?).kind() != NodeKind::Text {
            return Err(MutationError::NotText(node));
        }
        self.0
            .append_text_to_node(id, text)
            .map_err(|_| MutationError::NotText(node))
    }
    fn set_attribute(
        &mut self,
        node: NodeKey,
        name: QualName,
        value: &str,
    ) -> Result<(), MutationError> {
        let id = self.element(node)?;
        self.0.set_attribute(id, name, value);
        Ok(())
    }
    fn clear_attribute(&mut self, node: NodeKey, name: QualName) -> Result<(), MutationError> {
        let id = self.element(node)?;
        self.0.clear_attribute(id, name);
        Ok(())
    }
    fn remove_style_property(&mut self, node: NodeKey, name: &str) -> Result<(), MutationError> {
        let id = self.element(node)?;
        self.0.remove_style_property(id, name);
        Ok(())
    }
    fn add_attributes_if_missing(
        &mut self,
        node: NodeKey,
        attrs: impl IntoIterator<Item = NewAttribute>,
    ) -> Result<(), MutationError> {
        let id = self.element(node)?;
        self.0.add_attrs_if_missing(id, attributes(attrs));
        Ok(())
    }
}
