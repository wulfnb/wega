//! The only mapping from Blitz's document onto the Wega document view.
use super::{DocumentView, ElementName, NodeKey, NodeKind, NodeView};
use blitz_dom::{BaseDocument, Node, NodeId, node::NodeData};

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
    fn element_name(self) -> Option<ElementName<'a>> {
        match &self.data {
            NodeData::Element(e) | NodeData::AnonymousBlock(e) => Some(ElementName {
                local: &e.name.local,
                namespace: &e.name.ns,
                prefix: e.name.prefix.as_deref(),
            }),
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
    fn children(self) -> impl ExactSizeIterator<Item = NodeKey> + 'a {
        self.children.iter().copied().map(NodeKey::from)
    }
}
