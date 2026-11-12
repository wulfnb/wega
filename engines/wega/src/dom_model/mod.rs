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
//! - **Read-only**: mutation stays on the document engine (`mutate()`), because an equally
//!   small mutation contract has only the parser and the DOM ops as consumers (see report).
mod blitz;

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
    /// `Some` for elements (and anonymous blocks).
    fn element_name(self) -> Option<ElementName<'a>>;
    /// The value of the attribute whose local name is `name`.
    fn attr(self, name: &str) -> Option<&'a str>;
    fn parent(self) -> Option<NodeKey>;
    fn children(self) -> impl ExactSizeIterator<Item = NodeKey> + 'a;
}

/// A document that can be inspected. Implemented once per document engine.
pub trait DocumentView {
    type Node<'a>: NodeView<'a>
    where
        Self: 'a;
    /// `None` when the key does not (or no longer) refer to a node of this document.
    fn node(&self, key: NodeKey) -> Option<Self::Node<'_>>;
    fn element_by_id(&self, id: &str) -> Option<NodeKey>;
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
pub fn subtree<D: DocumentView, K: From<NodeKey>>(doc: &D, root: NodeKey) -> Vec<K> {
    let mut nodes = Vec::new();
    let mut pending = vec![root];
    while let Some(key) = pending.pop() {
        if let Some(node) = doc.node(key) {
            nodes.push(K::from(key));
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

#[cfg(test)]
mod tests {
    use super::*;
    use blitz_dom::{BaseDocument, DocumentConfig, NodeId, node::NodeData};
    use blitz_html::HtmlDocument;
    use html5ever::{LocalName, QualName, ns};
    use std::time::Instant;

    const HTML: &str = "<!doctype html><title>t</title><body><div id=root class=a>\
        <p id=p1 data-x=1>text<b>bold</b><!--c--></p><svg id=s><circle r=1 /></svg>\
        <template id=t><i>in</i></template><span id=sp></span></div></body>";

    fn doc() -> BaseDocument {
        HtmlDocument::from_html(HTML, DocumentConfig::default()).into_inner()
    }
    fn root_key(doc: &BaseDocument) -> NodeKey {
        NodeKey::from(doc.root_node().id)
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
    fn legacy_subtree(doc: &BaseDocument, root: NodeId) -> Vec<NodeId> {
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
    fn legacy_attribute(doc: &BaseDocument, id: NodeId, name: &str) -> Option<String> {
        doc.get_node(id)?
            .element_data()?
            .attr(LocalName::from(name))
            .map(str::to_owned)
    }
    fn legacy_info(doc: &BaseDocument, id: NodeId) -> NodeInfo {
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
        let root: NodeId = root_key(&doc).into();
        let legacy = legacy_subtree(&doc, root);
        let model: Vec<NodeId> = subtree(&doc, root.into());
        assert_eq!(model, legacy, "same nodes in the same order");
        assert!(legacy.len() > 10);
        for id in legacy {
            assert_eq!(node_info(&doc, id.into()), Some(legacy_info(&doc, id)));
            for name in ["id", "class", "data-x", "r", "missing"] {
                assert_eq!(
                    attribute(&doc, id.into(), name),
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
            assert_eq!(tree_root(&doc, id.into()), root_key(&doc));
        }
    }

    #[test]
    fn keys_round_trip_and_dropped_nodes_never_resolve() {
        let mut doc = doc();
        let sp = doc.element_by_id("sp").unwrap();
        assert_eq!(NodeKey::from_u64(sp.as_u64()), sp);
        assert_eq!(sp.to_string(), sp.as_u64().to_string());
        assert_eq!(NodeId::from(sp).as_u64(), sp.as_u64());
        let id: NodeId = sp.into();
        doc.mutate().remove_and_drop_node(id);
        assert!(doc.node(sp).is_none(), "dropped node");
        // Reusing the slot must not make the stale key alias the new node.
        let fresh = doc.mutate().create_element(
            QualName::new(None, ns!(html), LocalName::from("em")),
            vec![],
        );
        assert_ne!(NodeKey::from(fresh), sp);
        assert!(doc.node(sp).is_none(), "stale key still unresolved");
        assert!(doc.node(root_key(&doc)).is_some());
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
        let doc = HtmlDocument::from_html(&html, DocumentConfig::default()).into_inner();
        let root: NodeId = doc.element_by_id("root").unwrap().into();
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
        let b = time(&|| subtree::<_, NodeId>(&doc, root.into()).len());
        let c = time(&|| {
            ids.iter()
                .filter(|id| legacy_attribute(&doc, **id, "data-i").is_some())
                .count()
        });
        let d = time(&|| {
            ids.iter()
                .filter(|id| attribute(&doc, NodeKey::from(**id), "data-i").is_some())
                .count()
        });
        println!(
            "DOMMODEL subtree({} nodes): direct {a:.0} us, model {b:.0} us; attribute x{}: direct {c:.0} us, model {d:.0} us",
            ids.len(),
            ids.len()
        );
    }
}
