//! The mutation epoch (`wega_dom::MutationEpoch`): one step per successful state-changing
//! operation, nothing for no-ops, failures or detached creation, wrapping, and separate from
//! document identity.
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use html5ever::{LocalName, QualName, ns};
use wega_adapter_blitz::BlitzDocument;
use wega_dom::{
    DocumentMutation, DocumentView, MutationClock, MutationError, Mutator, NewAttribute, NodeKey,
    style::{StyleAccess, StyleControl},
};

const HTML: &str =
    "<!doctype html><body><div id=a class=x>one</div><div id=b></div><p id=p>text</p></body>";

fn doc() -> BlitzDocument {
    BlitzDocument::new(HtmlDocument::from_html(HTML, DocumentConfig::default()).into_inner())
}
fn epoch(doc: &BlitzDocument) -> u64 {
    doc.mutation_epoch().as_u64()
}
fn key(doc: &BlitzDocument, id: &str) -> NodeKey {
    doc.element_by_id(id).unwrap()
}
fn attr(name: &str) -> QualName {
    QualName::new(None, ns!(), LocalName::from(name))
}
fn text_of(doc: &BlitzDocument, id: &str) -> NodeKey {
    let node = doc.node(key(doc, id)).unwrap();
    wega_dom::NodeView::child_at(node, 0).unwrap()
}
/// Run `op` and report by how much the epoch moved.
fn moved(doc: &mut BlitzDocument, op: impl FnOnce(&mut BlitzDocument)) -> u64 {
    let before = epoch(doc);
    op(doc);
    epoch(doc).wrapping_sub(before)
}

#[test]
fn a_fresh_document_starts_at_zero_and_reads_never_move_it() {
    let doc = doc();
    assert_eq!(epoch(&doc), 0);
    let _ = (doc.root(), doc.base_url(), doc.element_by_id("a"));
    let a = key(&doc, "a");
    let _ = wega_dom::node_info(&doc, a);
    let _ = wega_dom::subtree(&doc, a, |_| true);
    let mut css = String::new();
    doc.inline_style(a, &mut css).unwrap();
    doc.resolved_value(a, "color", &mut css).unwrap();
    assert_eq!(epoch(&doc), 0);
}

#[test]
fn structural_operations_step_once_when_they_change_something() {
    let mut doc = doc();
    let (a, b) = (key(&doc, "a"), key(&doc, "b"));
    let child = wega_dom::mutate(&mut doc).create_element(attr("span"), []);
    // Detached creation is not an observable change.
    assert_eq!(
        moved(&mut doc, |d| {
            wega_dom::mutate(d).create_element(attr("i"), []);
        }),
        0
    );
    assert_eq!(
        moved(&mut doc, |d| {
            wega_dom::mutate(d).create_text("t");
        }),
        0
    );
    assert_eq!(
        moved(&mut doc, |d| {
            wega_dom::mutate(d).create_comment("c");
        }),
        0
    );
    assert_eq!(
        moved(&mut doc, |d| {
            wega_dom::mutate(d).clone_node(a, true).unwrap();
        }),
        0
    );
    // append / insert / replace / remove / reparent.
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .append_children(b, &[child])
            .unwrap()),
        1
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .append_children(b, &[])
            .unwrap()),
        0,
        "appending nothing"
    );
    let sibling = wega_dom::mutate(&mut doc).create_element(attr("em"), []);
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .insert_before(child, &[sibling])
            .unwrap()),
        1
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .insert_before(child, &[])
            .unwrap()),
        0
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d).remove(sibling).unwrap()),
        1
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .replace_children(b, &[sibling])
            .unwrap()),
        1
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .replace_children(b, &[])
            .unwrap()),
        1,
        "clearing children"
    );
    let empty = key(&doc, "p");
    let target = wega_dom::mutate(&mut doc).create_element(attr("section"), []);
    let b_key = key(&doc, "b");
    wega_dom::mutate(&mut doc)
        .append_children(b_key, &[target])
        .unwrap();
    wega_dom::mutate(&mut doc)
        .replace_children(target, &[])
        .unwrap();
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .replace_children(target, &[])
            .unwrap()),
        0,
        "nothing to nothing"
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .reparent_children(target, empty)
            .unwrap()),
        0,
        "no children to move"
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .reparent_children(empty, target)
            .unwrap()),
        1
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .remove_and_drop(target)
            .unwrap()),
        1
    );
}

#[test]
fn text_and_attribute_operations_step_only_on_a_real_change() {
    let mut doc = doc();
    let (a, text) = (key(&doc, "a"), text_of(&doc, "a"));
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .set_text(text, "one")
            .unwrap()),
        0,
        "same text"
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .set_text(text, "two")
            .unwrap()),
        1
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .append_text(text, "")
            .unwrap()),
        0
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .append_text(text, "!")
            .unwrap()),
        1
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .set_attribute(a, attr("class"), "x")
            .unwrap()),
        0,
        "same value"
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .set_attribute(a, attr("class"), "y")
            .unwrap()),
        1
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .set_attribute(a, attr("title"), "t")
            .unwrap()),
        1,
        "new attribute"
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .clear_attribute(a, attr("missing"))
            .unwrap()),
        0,
        "absent"
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .clear_attribute(a, attr("title"))
            .unwrap()),
        1
    );
    let new = |name: &str, value: &str| NewAttribute {
        name: attr(name),
        value: value.into(),
    };
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .add_attributes_if_missing(a, [new("class", "other")])
            .unwrap()),
        0,
        "already present"
    );
    assert_eq!(
        moved(&mut doc, |d| wega_dom::mutate(d)
            .add_attributes_if_missing(a, [new("class", "z"), new("data-n", "1")])
            .unwrap()),
        1,
        "one step for the operation, however many attributes it added"
    );
}

#[test]
fn failed_operations_never_move_the_epoch() {
    let mut doc = doc();
    let (a, b) = (key(&doc, "a"), key(&doc, "b"));
    let text = text_of(&doc, "a");
    let stale = {
        let n = wega_dom::mutate(&mut doc).create_element(attr("u"), []);
        wega_dom::mutate(&mut doc).remove_and_drop(n).unwrap();
        n
    };
    let before = epoch(&doc);
    let mut m = wega_dom::mutate(&mut doc);
    assert!(matches!(
        m.append_children(stale, &[a]),
        Err(MutationError::Stale(_))
    ));
    assert!(m.remove(stale).is_err() && m.remove_and_drop(stale).is_err());
    assert!(
        m.set_attribute(text, attr("x"), "y").is_err(),
        "not an element"
    );
    assert!(m.set_text(a, "x").is_err(), "not a text node");
    assert!(m.append_text(a, "x").is_err());
    assert!(m.append_children(a, &[a]).is_err(), "cycle");
    assert!(
        m.append_children(text, &[b]).is_err(),
        "text cannot be a parent"
    );
    assert!(m.reparent_children(a, a).is_err());
    assert!(m.insert_before(stale, &[b]).is_err());
    assert!(m.replace_children(stale, &[]).is_err());
    assert!(m.clear_attribute(stale, attr("id")).is_err());
    assert!(m.add_attributes_if_missing(stale, []).is_err());
    m.finish();
    assert_eq!(epoch(&doc), before);
}

#[test]
fn a_mutation_scope_steps_per_operation_and_finishing_adds_nothing() {
    let mut doc = doc();
    let (a, b) = (key(&doc, "a"), key(&doc, "b"));
    let before = epoch(&doc);
    {
        let mut scope = wega_dom::mutate(&mut doc);
        scope.set_attribute(a, attr("data-1"), "1").unwrap();
        scope.set_attribute(b, attr("data-2"), "2").unwrap();
        scope.set_attribute(b, attr("data-2"), "2").unwrap(); // no-op
        scope.finish();
    }
    assert_eq!(epoch(&doc) - before, 2);
    {
        let mut scope = wega_dom::mutate(&mut doc);
        scope.set_attribute(a, attr("data-3"), "3").unwrap();
        // dropped without finish
    }
    assert_eq!(epoch(&doc) - before, 3);
}

#[test]
fn inline_style_edits_step_only_when_the_declaration_changed() {
    let mut doc = doc();
    let a = key(&doc, "a");
    assert_eq!(
        moved(&mut doc, |d| assert!(
            d.set_inline_property(a, "color", "red", false).unwrap()
        )),
        1
    );
    assert_eq!(
        moved(&mut doc, |d| assert!(
            !d.set_inline_property(a, "color", "red", false).unwrap()
        )),
        0,
        "same declaration"
    );
    assert_eq!(
        moved(&mut doc, |d| assert!(
            d.set_inline_property(a, "color", "blue", false).unwrap()
        )),
        1
    );
    assert_eq!(
        moved(&mut doc, |d| assert!(
            !d.set_inline_property(a, "not-a-property", "x", false)
                .unwrap()
        )),
        0,
        "unknown property"
    );
    assert_eq!(
        moved(&mut doc, |d| assert!(
            !d.set_inline_property(a, "color", "not a color", false)
                .unwrap()
        )),
        0,
        "invalid value"
    );
}

#[test]
fn stylesheet_set_changes_are_part_of_the_epoch() {
    let mut doc = doc();
    let head_parent = key(&doc, "b");
    let style = wega_dom::mutate(&mut doc).create_element(attr("style"), []);
    let text = wega_dom::mutate(&mut doc).create_text("#a{color:red}");
    wega_dom::mutate(&mut doc)
        .append_children(style, &[text])
        .unwrap();
    let before = epoch(&doc);
    wega_dom::mutate(&mut doc)
        .append_children(head_parent, &[style])
        .unwrap();
    doc.refresh_style_element(style);
    assert!(epoch(&doc) != before);
}

#[test]
fn rule_edits_through_the_vendored_cssom_step_the_epoch() {
    // CSSOM now participates in the same mutation epoch.
    let mut doc = doc();
    let parent = key(&doc, "b");
    let style = wega_dom::mutate(&mut doc).create_element(attr("style"), []);
    let text = wega_dom::mutate(&mut doc).create_text("#a{color:red}");
    wega_dom::mutate(&mut doc)
        .append_children(style, &[text])
        .unwrap();
    wega_dom::mutate(&mut doc)
        .append_children(parent, &[style])
        .unwrap();
    doc.refresh_style_element(style);
    let owner = doc
        .stylesheet_owner_nodes()
        .next()
        .expect("an owned stylesheet");
    let (before, generation) = (epoch(&doc), doc.stylesheet_generation());
    let inserted = doc.stylesheet_insert_rule(owner, &[], "p{margin:0}", 0);
    assert!(
        inserted.is_ok(),
        "the CSSOM insert itself must succeed: {inserted:?}"
    );
    assert_ne!(doc.stylesheet_generation(), generation);
    assert_ne!(epoch(&doc), before);
}

#[test]
fn the_epoch_wraps_and_consumers_only_compare_for_inequality() {
    let mut doc = doc().with_mutation_epoch(u64::MAX);
    assert_eq!(epoch(&doc), u64::MAX);
    let a = key(&doc, "a");
    wega_dom::mutate(&mut doc)
        .set_attribute(a, attr("data-w"), "1")
        .unwrap();
    assert_eq!(epoch(&doc), 0, "wrapped");
    wega_dom::mutate(&mut doc)
        .set_attribute(a, attr("data-w"), "2")
        .unwrap();
    assert_eq!(epoch(&doc), 1);
    // `MutationEpoch` has equality but no ordering, so a wrap can never look like a decrease.
    let (x, y) = (
        wega_dom::MutationEpoch::from_u64(u64::MAX),
        wega_dom::MutationEpoch::from_u64(0),
    );
    assert_ne!(x, y);
}

#[test]
fn a_replacement_document_has_its_own_epoch_and_does_not_touch_the_old_one() {
    let mut old = doc();
    let a = key(&old, "a");
    wega_dom::mutate(&mut old)
        .set_attribute(a, attr("data-n"), "1")
        .unwrap();
    let before = epoch(&old);
    let fresh = doc();
    assert_eq!(
        epoch(&fresh),
        0,
        "document identity and mutation identity are separate"
    );
    assert_eq!(epoch(&old), before);
    // The same operations on a different document move only that document.
    let mut other = doc();
    let a2 = key(&other, "a");
    wega_dom::mutate(&mut other)
        .set_attribute(a2, attr("data-n"), "1")
        .unwrap();
    assert_eq!(epoch(&other), before);
    assert_eq!(epoch(&old), before);
}
