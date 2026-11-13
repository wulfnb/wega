//! The focus/hover/pointer contract (`wega_dom::input`) against a real Blitz document.
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use wega_adapter_blitz::{BlitzDocument, ToNodeId};
use wega_dom::input::{FocusState, InputControl, element_chain};
use wega_dom::{DocumentView, MutationError, Mutator, NodeKey, mutate};

const HTML: &str = "<!doctype html><style>body{margin:0}input{display:block;width:100px;height:30px;margin:0}\
    #box{width:100px;height:30px}</style><input id=a><input id=b><div id=box><span id=inner>x</span></div>";

fn doc() -> BlitzDocument {
    let mut doc =
        BlitzDocument::new(HtmlDocument::from_html(HTML, DocumentConfig::default()).into_inner());
    doc.resolve(0.0);
    doc
}
fn id(doc: &BlitzDocument, id: &str) -> NodeKey {
    doc.element_by_id(id).unwrap()
}

#[test]
fn nothing_is_focused_in_a_new_document_but_keyboard_input_has_a_target() {
    let doc = doc();
    assert_eq!(doc.focused(), None);
    let target = doc.keyboard_target().expect("fallback keyboard target");
    assert_ne!(
        target,
        doc.root(),
        "the fallback is the root element, not the document"
    );
    assert_eq!(doc.hovered(), None);
}

#[test]
fn focus_moves_reports_changes_and_clears() {
    let mut doc = doc();
    let (a, b) = (id(&doc, "a"), id(&doc, "b"));
    assert_eq!(doc.set_focus(a), Ok(true));
    assert_eq!(doc.focused(), Some(a));
    assert_eq!(doc.keyboard_target(), Some(a));
    assert_eq!(
        doc.set_focus(a),
        Ok(false),
        "focusing the focused node changes nothing"
    );
    assert_eq!(doc.set_focus(b), Ok(true));
    assert_eq!(doc.focused(), Some(b));
    doc.clear_focus();
    assert_eq!(doc.focused(), None);
    assert!(doc.keyboard_target().is_some());
}

#[test]
fn a_removed_node_loses_focus_and_a_stale_key_cannot_be_focused() {
    let mut doc = doc();
    let a = id(&doc, "a");
    doc.set_focus(a).unwrap();
    {
        let mut m = mutate(&mut doc);
        m.remove_and_drop(a).unwrap();
    }
    assert_eq!(
        doc.focused(),
        None,
        "focus does not point at a dropped node"
    );
    assert_eq!(doc.set_focus(a), Err(MutationError::Stale(a)));
}

#[test]
fn pointer_updates_report_the_nodes_before_and_after_and_clear_on_leave() {
    let mut doc = doc();
    let (a, b) = (id(&doc, "a"), id(&doc, "b"));
    let first = doc.pointer_moved(10.0, 10.0);
    assert_eq!((first.previous, first.current), (None, Some(a)));
    assert!(first.changed());
    let same = doc.pointer_moved(12.0, 12.0);
    assert!(
        !same.changed(),
        "moving within a node is not a hover change"
    );
    let second = doc.pointer_moved(10.0, 40.0);
    assert_eq!((second.previous, second.current), (Some(a), Some(b)));
    assert_eq!(doc.hovered(), Some(b));
    let left = doc.pointer_left();
    assert_eq!((left.previous, left.current), (Some(b), None));
    assert_eq!(doc.hovered(), None);
}

#[test]
fn the_element_chain_runs_from_the_target_up_through_element_ancestors() {
    let doc = doc();
    let (inner, boxed) = (id(&doc, "inner"), id(&doc, "box"));
    let chain = element_chain(&doc, inner);
    assert_eq!(chain[0], inner);
    assert_eq!(chain[1], boxed);
    assert!(chain.len() >= 4, "span, div, body, html: {chain:?}");
    assert!(
        !chain.contains(&doc.root()),
        "the document node is not an element"
    );
    // Same chain as Blitz's own.
    let blitz: Vec<NodeKey> = doc
        .node_chain(
            wega_dom::NodeView::attr(doc.node(inner).unwrap(), "id")
                .map(|_| inner.node_id())
                .unwrap(),
        )
        .into_iter()
        .map(wega_adapter_blitz::ToKey::key)
        .collect();
    assert_eq!(chain, blitz);
    assert!(element_chain(&doc, NodeKey::from_u64(u64::MAX)).is_empty());
}
