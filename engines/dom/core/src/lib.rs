//! Wega's read-only view of a document: the operations existing consumers need to look
//! at the tree without naming the document engine's types.
//!
//! This crate **must not mention Blitz** or any other document engine; the adapter crates
//! (`wega-adapter-blitz`) are the only place a concrete document is mapped onto it. A test
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
#![forbid(unsafe_code)]
pub mod input;
pub mod style;

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
