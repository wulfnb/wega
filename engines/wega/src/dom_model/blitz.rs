//! The only mapping from Blitz's document onto the Wega document view.
use super::{
    DocumentMutation, DocumentView, MutationError, Mutator, NewAttribute, NodeKey, NodeKind,
    NodeView,
};
use blitz_dom::{BaseDocument, DocumentMutator, Node, NodeId, node::Attribute, node::NodeData};
use html5ever::QualName;
use url::Url;

impl From<NodeId> for NodeKey {
    #[inline(always)]
    fn from(id: NodeId) -> Self {
        NodeKey::from_u64(id.as_u64())
    }
}
impl From<NodeKey> for NodeId {
    #[inline(always)]
    fn from(key: NodeKey) -> Self {
        NodeId::from_u64(key.as_u64())
    }
}

impl DocumentView for BaseDocument {
    type Node<'a> = &'a Node;
    #[inline]
    fn node(&self, key: NodeKey) -> Option<&Node> {
        self.get_node(NodeId::from(key))
    }
    fn element_by_id(&self, id: &str) -> Option<NodeKey> {
        self.get_element_by_id(id).map(NodeKey::from)
    }
    fn root(&self) -> NodeKey {
        NodeKey::from(self.root_node().id)
    }
    fn base_url(&self) -> &Url {
        self.url()
    }
}

impl<'a> NodeView<'a> for &'a Node {
    #[inline]
    fn kind(self) -> NodeKind {
        match &self.data {
            NodeData::Document(_) => NodeKind::Document,
            NodeData::Element(_) => NodeKind::Element,
            NodeData::AnonymousBlock(_) => NodeKind::AnonymousBlock,
            NodeData::Text(_) => NodeKind::Text,
            NodeData::Comment { .. } => NodeKind::Comment,
        }
    }
    #[inline]
    fn qual_name(self) -> Option<&'a QualName> {
        match &self.data {
            NodeData::Element(e) | NodeData::AnonymousBlock(e) => Some(&e.name),
            _ => None,
        }
    }
    #[inline]
    fn attr(self, name: &str) -> Option<&'a str> {
        let element = self.element_data()?;
        element
            .attrs
            .iter()
            .find(|attr| &*attr.name.local == name)
            .map(|attr| &*attr.value)
    }
    #[inline(always)]
    fn parent(self) -> Option<NodeKey> {
        self.parent.map(NodeKey::from)
    }
    #[inline]
    fn children(self) -> impl ExactSizeIterator<Item = NodeKey> + DoubleEndedIterator + 'a {
        self.children.iter().copied().map(NodeKey::from)
    }
    #[inline]
    fn child_at(self, index: usize) -> Option<NodeKey> {
        self.children.get(index).copied().map(NodeKey::from)
    }
    fn attributes(self) -> impl Iterator<Item = (&'a QualName, &'a str)> + 'a {
        self.element_data()
            .into_iter()
            .flat_map(|element| element.attrs.iter())
            .map(|attr| (&attr.name, &*attr.value))
    }
}

impl DocumentMutation for BaseDocument {
    type Mutator<'a> = BlitzMutator<'a>;
    fn mutate(&mut self) -> BlitzMutator<'_> {
        BlitzMutator(BaseDocument::mutate(self))
    }
    fn refresh_style_element(&mut self, key: NodeKey) {
        let id = NodeId::from(key);
        if self.get_node(id).is_some() {
            self.process_style_element(id);
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
            .get_node(NodeId::from(key))
            .ok_or(MutationError::Stale(key))
    }
    fn live(&self, key: NodeKey) -> Result<NodeId, MutationError> {
        self.node(key).map(|_| NodeId::from(key))
    }
    fn element(&self, key: NodeKey) -> Result<NodeId, MutationError> {
        match self.node(key)?.kind() {
            NodeKind::Element | NodeKind::AnonymousBlock => Ok(NodeId::from(key)),
            _ => Err(MutationError::NotElement(key)),
        }
    }
    fn container(&self, key: NodeKey) -> Result<NodeId, MutationError> {
        match self.node(key)?.kind() {
            NodeKind::Text | NodeKind::Comment => Err(MutationError::InvalidParent(key)),
            _ => Ok(NodeId::from(key)),
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
        let mut current = self
            .0
            .doc
            .get_node(NodeId::from(node))
            .and_then(|n| n.parent);
        while let Some(id) = current {
            if NodeKey::from(id) == ancestor {
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
        [one] => f(&[NodeId::from(*one)]),
        many => {
            let ids: Vec<NodeId> = many.iter().copied().map(NodeId::from).collect();
            f(&ids)
        }
    }
}

impl Mutator for BlitzMutator<'_> {
    fn parent(&self, key: NodeKey) -> Option<NodeKey> {
        self.node(key).ok()?.parent.map(NodeKey::from)
    }
    fn last_child(&self, key: NodeKey) -> Option<NodeKey> {
        self.node(key)
            .ok()?
            .children
            .last()
            .copied()
            .map(NodeKey::from)
    }
    fn previous_sibling(&self, key: NodeKey) -> Option<NodeKey> {
        self.node(key).ok()?;
        self.0
            .previous_sibling_id(NodeId::from(key))
            .map(NodeKey::from)
    }

    fn create_element(
        &mut self,
        name: QualName,
        attrs: impl IntoIterator<Item = NewAttribute>,
    ) -> NodeKey {
        NodeKey::from(self.0.create_element(name, attributes(attrs)))
    }
    fn create_text(&mut self, text: &str) -> NodeKey {
        NodeKey::from(self.0.create_text_node(text))
    }
    fn create_comment(&mut self, text: &str) -> NodeKey {
        NodeKey::from(self.0.create_comment_node(text))
    }
    fn template_contents(&mut self, template: NodeKey) -> Result<NodeKey, MutationError> {
        let id = self.element(template)?;
        Ok(NodeKey::from(self.0.template_contents(id)))
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
            return Ok(NodeKey::from(self.0.deep_clone_node(id)));
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
        Ok(NodeKey::from(cloned))
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
        if self.node(node)?.kind() != NodeKind::Text {
            return Err(MutationError::NotText(node));
        }
        self.0.set_node_text(id, text);
        Ok(())
    }
    fn append_text(&mut self, node: NodeKey, text: &str) -> Result<(), MutationError> {
        let id = self.live(node)?;
        if self.node(node)?.kind() != NodeKind::Text {
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
