//! Wega's read-only view of a document: the operations existing consumers need to look
//! at the tree without naming the document engine's types.
//!
//! This module (and everything under it except `blitz.rs`) **must not mention Blitz**;
//! `blitz.rs` is the single place that maps Blitz's `BaseDocument`/`Node` onto it. A test
//! enforces the rule. The contract is deliberately small: it covers lookup, kind/name,
//! attributes and tree links, which is what the JavaScript DOM bindings use most.
//!
//! Design choices, from measured call sites (see `docs/architecture/dom-migration-report.md`):
//! - **Identity**: [`NodeKey`] is the `u64` JavaScript wrappers already hold (decimal strings
//!   on the wire). The document must guarantee that a key referring to a dropped node never
//!   resolves to another node (Blitz's ids carry a slot version for this).
//! - **Static dispatch**: consumers are generic over [`DocumentView`]; nodes are borrowed
//!   handles (`Copy`) obtained through an associated type, so there is no wrapper allocation,
//!   no copy of document state and no virtual call.
//! - **Mutation** ([`DocumentMutation`]/[`Mutator`]): the operations the HTML parser and the DOM
//!   creation/removal ops share. Semantics are documented on [`Mutator`]: operations apply
//!   immediately, each one validates *before* touching the document (all-or-nothing per
//!   operation, never a panic on a stale key), and deferred side effects run when the mutator is
//!   finished or dropped. There is no rollback and no staging.
mod blitz;
pub use blitz::{BlitzDocument, ToKey, ToNodeId};

use html5ever::QualName;
use url::Url;

/// Stable node identity. Round-trips through `u64` and its decimal string form.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeKey(u64);

impl NodeKey {
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}
impl std::fmt::Display for NodeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Document,
    Element,
    /// Layout-created anonymous box; behaves like an element for lookups.
    AnonymousBlock,
    Text,
    Comment,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ElementName<'a> {
    pub local: &'a str,
    pub namespace: &'a str,
    pub prefix: Option<&'a str>,
}

pub const HTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";

/// A borrowed, `Copy` handle to one node of a document.
pub trait NodeView<'a>: Copy {
    fn kind(self) -> NodeKind;
    /// The qualified name of an element (or anonymous block).
    fn qual_name(self) -> Option<&'a QualName>;
    /// `Some` for elements (and anonymous blocks).
    fn element_name(self) -> Option<ElementName<'a>> {
        self.qual_name().map(|name| ElementName {
            local: &name.local,
            namespace: &name.ns,
            prefix: name.prefix.as_deref(),
        })
    }
    /// The value of the attribute whose local name is `name`.
    fn attr(self, name: &str) -> Option<&'a str>;
    fn parent(self) -> Option<NodeKey>;
    fn children(self) -> impl ExactSizeIterator<Item = NodeKey> + DoubleEndedIterator + 'a;
    /// The child at `index`; engines override this with direct indexing (`NodeList.item`).
    fn child_at(self, index: usize) -> Option<NodeKey> {
        self.children().nth(index)
    }
    /// All attributes with their qualified names, in document order; empty for non-elements.
    fn attributes(self) -> impl Iterator<Item = (&'a QualName, &'a str)> + 'a;
}

/// A document that can be inspected. Implemented once per document engine.
pub trait DocumentView {
    type Node<'a>: NodeView<'a>
    where
        Self: 'a;
    /// `None` when the key does not (or no longer) refer to a node of this document.
    fn node(&self, key: NodeKey) -> Option<Self::Node<'_>>;
    fn element_by_id(&self, id: &str) -> Option<NodeKey>;
    /// The document node.
    fn root(&self) -> NodeKey;
    fn base_url(&self) -> &Url;
}

/// A borrowed document is a document: lets a parser build into a `&mut` the caller already
/// holds, without wrapping it in a second owner.
impl<D: DocumentView> DocumentView for &mut D {
    type Node<'a>
        = D::Node<'a>
    where
        Self: 'a;
    fn node(&self, key: NodeKey) -> Option<Self::Node<'_>> {
        (**self).node(key)
    }
    fn element_by_id(&self, id: &str) -> Option<NodeKey> {
        (**self).element_by_id(id)
    }
    fn root(&self) -> NodeKey {
        (**self).root()
    }
    fn base_url(&self) -> &Url {
        (**self).base_url()
    }
}

/// The value of the attribute with this namespace and local name.
pub fn attribute_ns<D: DocumentView>(
    doc: &D,
    key: NodeKey,
    namespace: &str,
    local: &str,
) -> Option<String> {
    doc.node(key)?
        .attributes()
        .find(|(name, _)| &*name.ns == namespace && &*name.local == local)
        .map(|(_, value)| value.to_owned())
}

/// The next (`forward`) or previous sibling of `key` among its parent's children.
pub fn sibling<D: DocumentView>(doc: &D, key: NodeKey, forward: bool) -> Option<NodeKey> {
    let parent = doc.node(key)?.parent()?;
    let mut children = doc.node(parent)?.children();
    if forward {
        children.by_ref().find(|child| *child == key)?;
        children.next()
    } else {
        let mut previous = None;
        for child in children {
            if child == key {
                return previous;
            }
            previous = Some(child);
        }
        None
    }
}

/// The root of the tree `key` belongs to (the topmost ancestor; `key` itself if it has no
/// parent or does not resolve).
pub fn tree_root<D: DocumentView>(doc: &D, key: NodeKey) -> NodeKey {
    let mut root = key;
    while let Some(parent) = doc.node(root).and_then(NodeView::parent) {
        root = parent;
    }
    root
}

/// The attribute value as an owned string.
pub fn attribute<D: DocumentView>(doc: &D, key: NodeKey, name: &str) -> Option<String> {
    doc.node(key)?.attr(name).map(str::to_owned)
}

/// `root` and all its descendants, depth-first with the same (stack) order the DOM ops
/// have always used. Keys that no longer resolve are skipped.
pub fn subtree<D: DocumentView, K>(
    doc: &D,
    root: NodeKey,
    convert: impl Fn(NodeKey) -> K,
) -> Vec<K> {
    let mut nodes = Vec::new();
    let mut pending = vec![root];
    while let Some(key) = pending.pop() {
        if let Some(node) = doc.node(key) {
            nodes.push(convert(key));
            pending.extend(node.children());
        }
    }
    nodes
}

/// The DOM `Node` interface facts for `nodeType`, `nodeName`, `localName`, `namespaceURI`
/// and `prefix`.
#[derive(Debug, PartialEq, Eq)]
pub struct NodeInfo {
    pub node_type: u8,
    pub node_name: String,
    pub local_name: Option<String>,
    pub namespace: Option<String>,
    pub prefix: Option<String>,
}

pub fn node_info<D: DocumentView>(doc: &D, key: NodeKey) -> Option<NodeInfo> {
    let node = doc.node(key)?;
    Some(match (node.kind(), node.element_name()) {
        (NodeKind::Document, _) => NodeInfo {
            node_type: 9,
            node_name: "#document".into(),
            local_name: None,
            namespace: None,
            prefix: None,
        },
        (NodeKind::Text, _) => NodeInfo {
            node_type: 3,
            node_name: "#text".into(),
            local_name: None,
            namespace: None,
            prefix: None,
        },
        (NodeKind::Comment, _) => NodeInfo {
            node_type: 8,
            node_name: "#comment".into(),
            local_name: None,
            namespace: None,
            prefix: None,
        },
        (_, Some(name)) => {
            let fragment = name.local == "#document-fragment";
            let node_name = if name.namespace == HTML_NAMESPACE && !fragment {
                name.local.to_uppercase()
            } else {
                name.local.to_owned()
            };
            NodeInfo {
                node_type: if fragment { 11 } else { 1 },
                node_name,
                local_name: Some(name.local.to_owned()),
                namespace: Some(name.namespace.to_owned()),
                prefix: name.prefix.map(str::to_owned),
            }
        }
        // An element kind always has a name; treat the impossible case as "not a node".
        (_, None) => return None,
    })
}

/// Starts a mutation scope without going through an inherent `mutate` method.
pub fn mutate<D: DocumentMutation>(doc: &mut D) -> D::Mutator<'_> {
    doc.mutate()
}

/// Why a mutation was rejected. A rejected operation leaves the document untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationError {
    /// The key does not (or no longer) refer to a node of this document.
    Stale(NodeKey),
    /// The anchor of an insertion has no parent.
    Detached(NodeKey),
    /// The node cannot have children (text, comment).
    InvalidParent(NodeKey),
    /// The operation needs an element.
    NotElement(NodeKey),
    /// The operation needs a text node.
    NotText(NodeKey),
    /// The insertion would put a node below itself.
    HierarchyCycle { parent: NodeKey, child: NodeKey },
    /// A shallow copy of this kind of node (document, doctype, ...) is not defined.
    NotCloneable(NodeKey),
}
impl std::fmt::Display for MutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stale(key) => write!(f, "node {key} no longer exists"),
            Self::Detached(key) => write!(f, "node {key} has no parent"),
            Self::InvalidParent(key) => write!(f, "node {key} cannot have children"),
            Self::NotElement(key) => write!(f, "node {key} is not an element"),
            Self::NotText(key) => write!(f, "node {key} is not a text node"),
            Self::NotCloneable(key) => write!(f, "node {key} cannot be cloned"),
            Self::HierarchyCycle { parent, child } => {
                write!(
                    f,
                    "cannot insert node {child} below its descendant {parent}"
                )
            }
        }
    }
}
impl std::error::Error for MutationError {}

/// An attribute for a new element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAttribute {
    pub name: QualName,
    pub value: String,
}

/// A document that can be mutated.
pub trait DocumentMutation: DocumentView {
    type Mutator<'a>: Mutator
    where
        Self: 'a;
    /// Starts a mutation scope. Exclusive borrow: scopes cannot nest on one document.
    fn mutate(&mut self) -> Self::Mutator<'_>;
    /// Re-reads an inline `<style>` element's text into its stylesheet now. The parser calls
    /// this when the element is closed, because its text arrives after the element is
    /// inserted. Applies immediately; not part of a mutation scope.
    fn refresh_style_element(&mut self, key: NodeKey);
}

impl<D: DocumentMutation> DocumentMutation for &mut D {
    type Mutator<'a>
        = D::Mutator<'a>
    where
        Self: 'a;
    fn mutate(&mut self) -> Self::Mutator<'_> {
        (**self).mutate()
    }
    fn refresh_style_element(&mut self, key: NodeKey) {
        (**self).refresh_style_element(key);
    }
}

/// One mutation scope. **Semantics (fixed by tests):**
///
/// - *Immediate application.* Every operation changes the document before it returns; nothing
///   is staged, so the document is always readable between operations of one scope.
/// - *Validate, then apply.* Every fallible operation checks all keys, node kinds and the tree
///   shape first and returns a [`MutationError`] without touching the document; a rejected
///   operation changes nothing. A sequence of operations is **not** atomic and has no rollback.
/// - *Deferred side effects* (title, `<style>` text, form owners, autofocus, redraw request)
///   run when the scope ends: [`Mutator::finish`] or dropping it. Style/layout invalidation of
///   the mutated nodes happens inside each operation, not at the end.
/// - *Finalization runs no script.* It can only call the shell (title, redraw, focus), so a
///   caller may hold its `RefCell` borrow across it.
/// - *Unwinding.* Wega adds no `Drop`; the document engine's drop still runs its deferred work
///   during a panic. That is best-effort on a document the panic may have left inconsistent:
///   treat the document as poisoned after a panic.
/// - *Stale keys* never panic and never alias: a key whose node was dropped fails with
///   `Stale` even after its slot is reused.
pub trait Mutator {
    // Queries; stale keys give `None`.
    fn parent(&self, key: NodeKey) -> Option<NodeKey>;
    fn last_child(&self, key: NodeKey) -> Option<NodeKey>;
    fn previous_sibling(&self, key: NodeKey) -> Option<NodeKey>;

    // Creation: new nodes are detached.
    fn create_element(
        &mut self,
        name: QualName,
        attrs: impl IntoIterator<Item = NewAttribute>,
    ) -> NodeKey;
    fn create_text(&mut self, text: &str) -> NodeKey;
    fn create_comment(&mut self, text: &str) -> NodeKey;
    /// The `<template>` contents fragment, created on first use.
    fn template_contents(&mut self, template: NodeKey) -> Result<NodeKey, MutationError>;
    /// A detached copy of `key`: element (name and attributes), text or comment when
    /// `deep` is false; the whole subtree when it is true. Backs `cloneNode`/`importNode`.
    fn clone_node(&mut self, key: NodeKey, deep: bool) -> Result<NodeKey, MutationError>;

    // Structure. Children already in the tree are moved (detached from their old parent first).
    fn append_children(
        &mut self,
        parent: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError>;
    fn insert_before(&mut self, anchor: NodeKey, children: &[NodeKey])
    -> Result<(), MutationError>;
    fn replace_children(
        &mut self,
        parent: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError>;
    /// Detaches `key` from its parent; the node stays alive.
    fn remove(&mut self, key: NodeKey) -> Result<(), MutationError>;
    /// Detaches `key` and drops it and its descendants; their keys become stale.
    fn remove_and_drop(&mut self, key: NodeKey) -> Result<(), MutationError>;
    fn reparent_children(&mut self, from: NodeKey, to: NodeKey) -> Result<(), MutationError>;

    // Text and attributes.
    fn set_text(&mut self, node: NodeKey, text: &str) -> Result<(), MutationError>;
    fn append_text(&mut self, node: NodeKey, text: &str) -> Result<(), MutationError>;
    fn set_attribute(
        &mut self,
        node: NodeKey,
        name: QualName,
        value: &str,
    ) -> Result<(), MutationError>;
    fn clear_attribute(&mut self, node: NodeKey, name: QualName) -> Result<(), MutationError>;
    /// Removes one declaration from the element's inline `style` (`element.style`).
    fn remove_style_property(&mut self, node: NodeKey, name: &str) -> Result<(), MutationError>;
    fn add_attributes_if_missing(
        &mut self,
        node: NodeKey,
        attrs: impl IntoIterator<Item = NewAttribute>,
    ) -> Result<(), MutationError>;

    /// Ends the scope now (runs the deferred side effects). Equivalent to dropping it.
    fn finish(self)
    where
        Self: Sized,
    {
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{BlitzDocument, ToKey, ToNodeId};
    use blitz_dom::{BaseDocument, DocumentConfig, NodeId, node::NodeData};
    use blitz_html::HtmlDocument;
    use html5ever::{LocalName, QualName, ns};
    use std::time::Instant;

    const HTML: &str = "<!doctype html><title>t</title><body><div id=root class=a>\
        <p id=p1 data-x=1>text<b>bold</b><!--c--></p><svg id=s><circle r=1 /></svg>\
        <template id=t><i>in</i></template><span id=sp></span></div></body>";

    fn doc() -> BlitzDocument {
        BlitzDocument::new(HtmlDocument::from_html(HTML, DocumentConfig::default()).into_inner())
    }
    fn root_key(doc: &BlitzDocument) -> NodeKey {
        (doc.root_node().id).key()
    }

    #[test]
    fn the_contract_names_no_blitz_types() {
        let source = include_str!("mod.rs");
        let contract = source.split("#[cfg(test)]").next().unwrap();
        for line in contract
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
        {
            assert!(
                !line.contains("blitz_") && !line.contains("BaseDocument"),
                "contract mentions the document engine: {line}"
            );
        }
    }

    // The pre-model implementations, kept verbatim as the reference.
    fn legacy_subtree(doc: &BlitzDocument, root: NodeId) -> Vec<NodeId> {
        let mut nodes = vec![];
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if let Some(node) = doc.get_node(id) {
                nodes.push(id);
                pending.extend(node.children.iter().copied());
            }
        }
        nodes
    }
    fn legacy_attribute(doc: &BlitzDocument, id: NodeId, name: &str) -> Option<String> {
        doc.get_node(id)?
            .element_data()?
            .attr(LocalName::from(name))
            .map(str::to_owned)
    }
    fn legacy_info(doc: &BlitzDocument, id: NodeId) -> NodeInfo {
        let node = doc.get_node(id).unwrap();
        let (kind, name, local, namespace, prefix) = match &node.data {
            NodeData::Document(_) => (9, "#document".to_owned(), None, None, None),
            NodeData::Text(_) => (3, "#text".to_owned(), None, None, None),
            NodeData::Comment { .. } => (8, "#comment".to_owned(), None, None, None),
            NodeData::Element(e) | NodeData::AnonymousBlock(e) => {
                let local = e.name.local.to_string();
                let fragment = local == "#document-fragment";
                let name = if e.name.ns == ns!(html) && !fragment {
                    local.to_uppercase()
                } else {
                    local.clone()
                };
                (
                    if fragment { 11 } else { 1 },
                    name,
                    Some(local),
                    Some(e.name.ns.to_string()),
                    e.name.prefix.as_ref().map(|p| p.to_string()),
                )
            }
        };
        NodeInfo {
            node_type: kind,
            node_name: name,
            local_name: local,
            namespace,
            prefix,
        }
    }

    #[test]
    fn model_matches_direct_blitz_access_for_every_node() {
        let doc = doc();
        let root: NodeId = root_key(&doc).node_id();
        let legacy = legacy_subtree(&doc, root);
        let model: Vec<NodeId> = subtree(&doc, root.key(), ToNodeId::node_id);
        assert_eq!(model, legacy, "same nodes in the same order");
        assert!(legacy.len() > 10);
        for id in legacy {
            assert_eq!(node_info(&doc, id.key()), Some(legacy_info(&doc, id)));
            for name in ["id", "class", "data-x", "r", "missing"] {
                assert_eq!(
                    attribute(&doc, id.key(), name),
                    legacy_attribute(&doc, id, name),
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn element_lookup_and_tree_links_agree_with_blitz() {
        let doc = doc();
        let p = doc.element_by_id("p1").expect("p1");
        let node = doc.node(p).unwrap();
        assert_eq!(node.kind(), NodeKind::Element);
        assert_eq!(node.element_name().unwrap().local, "p");
        assert_eq!(NodeView::attr(node, "data-x"), Some("1"));
        assert_eq!(node.children().len(), 3);
        let parent = node.parent().unwrap();
        assert_eq!(
            NodeView::attr(doc.node(parent).unwrap(), "id"),
            Some("root")
        );
        // Every node's tree root is the document root.
        for id in legacy_subtree(&doc, doc.root_node().id) {
            assert_eq!(tree_root(&doc, id.key()), root_key(&doc));
        }
    }

    #[test]
    fn keys_round_trip_and_dropped_nodes_never_resolve() {
        let mut doc = doc();
        let sp = doc.element_by_id("sp").unwrap();
        assert_eq!(NodeKey::from_u64(sp.as_u64()), sp);
        assert_eq!(sp.to_string(), sp.as_u64().to_string());
        assert_eq!(sp.node_id().as_u64(), sp.as_u64());
        let id: NodeId = sp.node_id();
        BaseDocument::mutate(&mut doc).remove_and_drop_node(id);
        assert!(doc.node(sp).is_none(), "dropped node");
        // Reusing the slot must not make the stale key alias the new node.
        let fresh = doc.mutate().create_element(
            QualName::new(None, ns!(html), LocalName::from("em")),
            vec![],
        );
        assert_ne!(fresh, sp);
        assert!(doc.node(sp).is_none(), "stale key still unresolved");
        assert!(doc.node(root_key(&doc)).is_some());
    }

    // ----- mutation contract -----

    fn qn(local: &str) -> QualName {
        QualName::new(None, ns!(html), LocalName::from(local))
    }
    fn empty_doc() -> BlitzDocument {
        BlitzDocument::new(BaseDocument::new(DocumentConfig::default()))
    }
    /// A compact structural dump: `name(child,child)` / `#text:..` for the subtree.
    fn dump<D: DocumentView>(doc: &D, key: NodeKey) -> String {
        let node = doc.node(key).unwrap();
        match node.kind() {
            NodeKind::Text => "#text".into(),
            NodeKind::Comment => "#comment".into(),
            NodeKind::Document => format!("#document[{}]", children_dump(doc, node)),
            _ => format!(
                "{}{}[{}]",
                node.element_name().unwrap().local,
                node.attr("id")
                    .map(|id| format!("#{id}"))
                    .unwrap_or_default(),
                children_dump(doc, node)
            ),
        }
    }
    fn children_dump<'a, D: DocumentView>(doc: &D, node: D::Node<'a>) -> String {
        node.children()
            .map(|child| dump(doc, child))
            .collect::<Vec<_>>()
            .join(",")
    }
    /// Every child's parent link points back and no node appears twice.
    fn assert_integrity(doc: &BlitzDocument) {
        let mut seen = std::collections::HashSet::new();
        let mut pending = vec![doc.root()];
        while let Some(key) = pending.pop() {
            assert!(seen.insert(key), "node {key} appears twice in the tree");
            for child in doc.node(key).unwrap().children() {
                assert_eq!(
                    doc.node(child).unwrap().parent(),
                    Some(key),
                    "parent link of {child}"
                );
                pending.push(child);
            }
        }
    }
    fn element(doc: &mut BlitzDocument, name: &str, id: &str) -> NodeKey {
        let mut m = mutate(doc);
        m.create_element(
            qn(name),
            [NewAttribute {
                name: QualName::new(None, ns!(), LocalName::from("id")),
                value: id.into(),
            }],
        )
    }
    fn tree() -> (BlitzDocument, NodeKey, NodeKey, NodeKey, NodeKey) {
        let mut doc = empty_doc();
        let root = doc.root();
        let host = element(&mut doc, "div", "host");
        let (a, b, c) = (
            element(&mut doc, "p", "a"),
            element(&mut doc, "p", "b"),
            element(&mut doc, "p", "c"),
        );
        let mut m = mutate(&mut doc);
        m.append_children(root, &[host]).unwrap();
        m.append_children(host, &[a, b, c]).unwrap();
        m.finish();
        (doc, host, a, b, c)
    }

    #[test]
    fn mutations_apply_in_order_and_keep_parent_links_consistent() {
        let (mut doc, host, a, b, c) = tree();
        assert_eq!(dump(&doc, host), "div#host[p#a[],p#b[],p#c[]]");
        let (x, y) = (element(&mut doc, "i", "x"), element(&mut doc, "i", "y"));
        let mut m = mutate(&mut doc);
        m.insert_before(b, &[x, y]).unwrap();
        // Moving within the same parent detaches first, then inserts at the anchor.
        m.insert_before(a, &[c]).unwrap();
        m.finish();
        assert_eq!(dump(&doc, host), "div#host[p#c[],p#a[],i#x[],i#y[],p#b[]]");
        assert_integrity(&doc);
        let mut m = mutate(&mut doc);
        m.replace_children(host, &[y, a]).unwrap();
        m.finish();
        assert_eq!(dump(&doc, host), "div#host[i#y[],p#a[]]");
        // Replaced-out nodes are detached, not dropped.
        assert_eq!(doc.node(b).unwrap().parent(), None);
        assert_integrity(&doc);
    }

    #[test]
    fn traversal_and_attribute_helpers_agree_with_direct_blitz_access() {
        let doc = doc();
        let mut all = Vec::new();
        let mut stack = vec![root_key(&doc)];
        while let Some(key) = stack.pop() {
            all.push(key);
            stack.extend(doc.node(key).unwrap().children());
        }
        for key in all {
            let direct = doc.get_node(key.node_id()).unwrap();
            let view = doc.node(key).unwrap();
            let children: Vec<NodeKey> = direct.children.iter().copied().map(ToKey::key).collect();
            assert_eq!(
                view.children().rev().collect::<Vec<_>>(),
                children.iter().rev().copied().collect::<Vec<_>>()
            );
            for index in 0..=children.len() {
                assert_eq!(view.child_at(index), children.get(index).copied());
            }
            let attrs: Vec<_> = direct
                .attrs()
                .unwrap_or_default()
                .iter()
                .map(|a| (a.name.clone(), a.value.clone()))
                .collect();
            let seen: Vec<_> = view
                .attributes()
                .map(|(n, v)| (n.clone(), v.to_owned()))
                .collect();
            assert_eq!(seen, attrs);
            for (name, value) in &attrs {
                assert_eq!(
                    attribute_ns(&doc, key, &name.ns, &name.local).as_deref(),
                    Some(value.as_str())
                );
            }
            let parent_children: Vec<NodeKey> = view
                .parent()
                .map(|p| doc.node(p).unwrap().children().collect())
                .unwrap_or_default();
            let index = parent_children.iter().position(|c| *c == key);
            assert_eq!(
                sibling(&doc, key, true),
                index.and_then(|i| parent_children.get(i + 1).copied())
            );
            assert_eq!(
                sibling(&doc, key, false),
                index
                    .and_then(|i| i.checked_sub(1))
                    .map(|i| parent_children[i])
            );
        }
    }

    #[test]
    fn clone_node_copies_shallow_or_deep_and_rejects_stale_and_unclonable_nodes() {
        let (mut doc, host, a, _b, _c) = tree();
        let text = {
            let mut m = mutate(&mut doc);
            let text = m.create_text("hello");
            m.append_children(a, &[text]).unwrap();
            m.finish();
            text
        };
        let root = root_key(&doc);
        let mut m = mutate(&mut doc);
        let shallow = m.clone_node(host, false).unwrap();
        let deep = m.clone_node(host, true).unwrap();
        let text_copy = m.clone_node(text, false).unwrap();
        assert_eq!(
            m.clone_node(root, false),
            Err(MutationError::NotCloneable(root))
        );
        m.finish();
        assert_eq!(dump(&doc, shallow), "div#host[]");
        assert_eq!(dump(&doc, deep), dump(&doc, host));
        assert_ne!(deep, host, "a deep clone has its own identity");
        assert_eq!(
            doc.node(deep).unwrap().parent(),
            None,
            "clones are detached"
        );
        assert_eq!(doc.node(text_copy).unwrap().kind(), NodeKind::Text);
        assert_integrity(&doc);
        let mut m = mutate(&mut doc);
        m.remove_and_drop(shallow).unwrap();
        assert_eq!(
            m.clone_node(shallow, true),
            Err(MutationError::Stale(shallow))
        );
    }

    #[test]
    fn remove_style_property_edits_the_inline_style_and_validates_the_target() {
        let (mut doc, _host, a, _b, _c) = tree();
        let mut m = mutate(&mut doc);
        m.set_attribute(
            a,
            QualName::new(None, ns!(), LocalName::from("style")),
            "color:red;margin:1px",
        )
        .unwrap();
        m.remove_style_property(a, "color").unwrap();
        let text = m.create_text("t");
        assert_eq!(
            m.remove_style_property(text, "color"),
            Err(MutationError::NotElement(text))
        );
        m.finish();
        let block = doc
            .get_node(a.node_id())
            .unwrap()
            .element_data()
            .unwrap()
            .style_attribute
            .clone()
            .unwrap();
        let guard = doc.guard().read();
        let mut css = String::new();
        let _ = block.read_with(&guard).to_css(&mut css);
        assert!(css.contains("margin") && !css.contains("color"), "{css}");
    }

    #[test]
    fn new_and_removed_nodes_are_detached_and_can_be_reinserted() {
        let (mut doc, host, a, _b, _c) = tree();
        let fresh = element(&mut doc, "em", "fresh");
        assert_eq!(doc.node(fresh).unwrap().parent(), None);
        let mut m = mutate(&mut doc);
        assert_eq!(
            m.insert_before(fresh, &[a]),
            Err(MutationError::Detached(fresh))
        );
        m.remove(a).unwrap();
        assert_eq!(m.parent(a), None);
        m.append_children(fresh, &[a]).unwrap();
        m.append_children(host, &[fresh]).unwrap();
        m.finish();
        assert_eq!(dump(&doc, host), "div#host[p#b[],p#c[],em#fresh[p#a[]]]");
        assert_integrity(&doc);
    }

    #[test]
    fn stale_keys_are_rejected_everywhere_and_never_alias_reused_slots() {
        let (mut doc, host, a, _b, _c) = tree();
        let mut m = mutate(&mut doc);
        m.remove_and_drop(a).unwrap();
        // Reuse the freed slot.
        let reused = m.create_element(qn("u"), []);
        assert_ne!(reused, a);
        let other = m.create_text("t");
        let stale = MutationError::Stale(a);
        assert_eq!(m.append_children(host, &[a]), Err(stale));
        assert_eq!(m.append_children(a, &[other]), Err(stale));
        assert_eq!(m.insert_before(a, &[other]), Err(stale));
        assert_eq!(m.replace_children(a, &[]), Err(stale));
        assert_eq!(m.remove(a), Err(stale));
        assert_eq!(m.remove_and_drop(a), Err(stale));
        assert_eq!(m.set_text(a, "x"), Err(stale));
        assert_eq!(m.append_text(a, "x"), Err(stale));
        assert_eq!(m.set_attribute(a, qn("id"), "x"), Err(stale));
        assert_eq!(m.clear_attribute(a, qn("id")), Err(stale));
        assert_eq!(m.template_contents(a), Err(stale));
        assert_eq!(m.reparent_children(a, host), Err(stale));
        assert_eq!(
            (m.parent(a), m.last_child(a), m.previous_sibling(a)),
            (None, None, None)
        );
        m.finish();
        assert!(doc.node(a).is_none());
        assert_eq!(dump(&doc, host), "div#host[p#b[],p#c[]]");
        assert_integrity(&doc);
    }

    #[test]
    fn rejected_operations_leave_the_document_unchanged() {
        let (mut doc, host, a, b, _c) = tree();
        let text = {
            let mut m = mutate(&mut doc);
            let text = m.create_text("hi");
            m.append_children(a, &[text]).unwrap();
            m.finish();
            text
        };
        let before = dump(&doc, doc.root());
        let mut m = mutate(&mut doc);
        let deep = m.create_element(qn("span"), []);
        m.append_children(b, &[deep]).unwrap();
        let before_deep = before.replace("p#b[]", "p#b[span[]]");
        assert_eq!(
            m.append_children(deep, &[host]),
            Err(MutationError::HierarchyCycle {
                parent: deep,
                child: host
            })
        );
        assert_eq!(
            m.append_children(host, &[host]),
            Err(MutationError::HierarchyCycle {
                parent: host,
                child: host
            })
        );
        assert_eq!(
            m.insert_before(deep, &[host]),
            Err(MutationError::HierarchyCycle {
                parent: b,
                child: host
            })
        );
        assert_eq!(
            m.reparent_children(host, b),
            Err(MutationError::HierarchyCycle {
                parent: b,
                child: host
            })
        );
        assert_eq!(
            m.append_children(text, &[deep]),
            Err(MutationError::InvalidParent(text))
        );
        assert_eq!(
            m.set_attribute(text, qn("id"), "x"),
            Err(MutationError::NotElement(text))
        );
        assert_eq!(m.append_text(a, "x"), Err(MutationError::NotText(a)));
        assert_eq!(m.set_text(a, "x"), Err(MutationError::NotText(a)));
        assert_eq!(
            m.template_contents(text),
            Err(MutationError::NotElement(text))
        );
        // A batch with one stale child must not detach the valid ones.
        let gone = m.create_element(qn("s"), []);
        m.remove_and_drop(gone).unwrap();
        assert_eq!(
            m.append_children(host, &[a, gone]),
            Err(MutationError::Stale(gone))
        );
        m.finish();
        assert_eq!(dump(&doc, doc.root()), before_deep);
        assert_integrity(&doc);
    }

    #[test]
    fn scopes_are_sequential_and_a_panic_does_not_roll_back_applied_operations() {
        let (mut doc, host, a, _b, _c) = tree();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut m = mutate(&mut doc);
            m.remove(a).unwrap();
            panic!("failure inside a mutation scope");
        }));
        assert!(unwound.is_err());
        // The applied removal stays (no rollback) and the document accepts a new scope.
        assert_eq!(doc.node(a).unwrap().parent(), None);
        let mut m = mutate(&mut doc);
        m.append_children(host, &[a]).unwrap();
        m.finish();
        assert_eq!(dump(&doc, host), "div#host[p#b[],p#c[],p#a[]]");
        assert_integrity(&doc);
    }

    #[test]
    fn layout_reflects_mutations_once_the_scope_ends() {
        let mut doc = empty_doc();
        let root = doc.root();
        let outer = element(&mut doc, "div", "outer");
        let first = {
            let mut m = mutate(&mut doc);
            let first = m.create_element(qn("div"), []);
            m.set_attribute(
                first,
                QualName::new(None, ns!(), LocalName::from("style")),
                "height:10px",
            )
            .unwrap();
            m.append_children(outer, &[first]).unwrap();
            m.append_children(root, &[outer]).unwrap();
            m.finish();
            first
        };
        let height = |doc: &mut BlitzDocument| {
            doc.resolve(0.0);
            doc.node(outer).unwrap().blitz().final_layout().size.height
        };
        let before = height(&mut doc);
        assert_eq!(before, 10.0);
        let mut m = mutate(&mut doc);
        let second = m.create_element(qn("div"), []);
        m.set_attribute(
            second,
            QualName::new(None, ns!(), LocalName::from("style")),
            "height:15px",
        )
        .unwrap();
        m.insert_before(first, &[second]).unwrap();
        // Dropped at the end of this statement block; layout is read after the scope.
        m.finish();
        assert_eq!(height(&mut doc), 25.0);
    }

    /// Mutation overhead of the contract against direct Blitz calls; run with
    /// `cargo test -p wega-engine --lib mutation_overhead -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn mutation_overhead_against_direct_access() {
        const N: usize = 4_000;
        let median = |mut v: Vec<f64>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() / 2]
        };
        let time = |f: &dyn Fn()| {
            median(
                (0..9)
                    .map(|_| {
                        let start = Instant::now();
                        f();
                        start.elapsed().as_secs_f64() * 1e6
                    })
                    .collect(),
            )
        };
        let direct = || {
            let mut doc = empty_doc();
            let root = doc.root_node().id;
            let mut m = BaseDocument::mutate(&mut *doc);
            let host = m.create_element(qn("div"), vec![]);
            m.append_children(root, &[host]);
            let mut kids = Vec::with_capacity(N);
            for _ in 0..N {
                let e = m.create_element(qn("p"), vec![]);
                m.append_children(host, &[e]);
                kids.push(e);
            }
            for e in &kids {
                m.set_attribute(
                    *e,
                    QualName::new(None, ns!(), LocalName::from("data-k")),
                    "v",
                );
            }
            for e in &kids {
                m.remove_node(*e);
            }
        };
        let contract = || {
            let mut doc = empty_doc();
            let root = doc.root();
            let mut m = mutate(&mut doc);
            let host = m.create_element(qn("div"), []);
            m.append_children(root, &[host]).unwrap();
            let mut kids = Vec::with_capacity(N);
            for _ in 0..N {
                let e = m.create_element(qn("p"), []);
                m.append_children(host, &[e]).unwrap();
                kids.push(e);
            }
            for e in &kids {
                m.set_attribute(
                    *e,
                    QualName::new(None, ns!(), LocalName::from("data-k")),
                    "v",
                )
                .unwrap();
            }
            for e in &kids {
                m.remove(*e).unwrap();
            }
        };
        let (d, c) = (time(&direct), time(&contract));
        println!(
            "MUTMODEL create+append+setAttribute+remove x{N}: direct {d:.0} us, contract {c:.0} us ({:+.1}%)",
            (c / d - 1.0) * 100.0
        );
    }

    /// Timing of the model against the reference on a large tree; run with
    /// `cargo test -p wega-engine --lib dom_model -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn overhead_against_direct_access() {
        let mut html = String::from("<!doctype html><body><div id=root>");
        for i in 0..8_000 {
            html.push_str(&format!("<div id=n{i} data-i={i}><span>x</span></div>"));
        }
        html.push_str("</div></body>");
        let doc = BlitzDocument::new(
            HtmlDocument::from_html(&html, DocumentConfig::default()).into_inner(),
        );
        let root: NodeId = doc.element_by_id("root").unwrap().node_id();
        let median = |mut v: Vec<f64>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() / 2]
        };
        let time = |f: &dyn Fn() -> usize| {
            median(
                (0..15)
                    .map(|_| {
                        let start = Instant::now();
                        std::hint::black_box(f());
                        start.elapsed().as_secs_f64() * 1e6
                    })
                    .collect(),
            )
        };
        let ids = legacy_subtree(&doc, root);
        let a = time(&|| legacy_subtree(&doc, root).len());
        let b = time(&|| subtree(&doc, root.key(), ToNodeId::node_id).len());
        let c = time(&|| {
            ids.iter()
                .filter(|id| legacy_attribute(&doc, **id, "data-i").is_some())
                .count()
        });
        let d = time(&|| {
            ids.iter()
                .filter(|id| attribute(&doc, (**id).key(), "data-i").is_some())
                .count()
        });
        println!(
            "DOMMODEL subtree({} nodes): direct {a:.0} us, model {b:.0} us; attribute x{}: direct {c:.0} us, model {d:.0} us",
            ids.len(),
            ids.len()
        );
    }
}
