use html5ever::{QualName, ns};
use wega_dom::{
    Document, DocumentMutation, DocumentView, MutationClock, MutationError, Mutator, NewAttribute,
    NodeView,
};
fn doc() -> Document {
    Document::new(url::Url::parse("https://wega.test/").unwrap())
}
fn name(s: &str) -> QualName {
    QualName::new(None, ns!(html), s.into())
}
fn attr(s: &str) -> QualName {
    QualName::new(None, ns!(), s.into())
}
fn element(d: &mut Document, tag: &str) -> wega_dom::NodeKey {
    d.mutate().create_element(name(tag), [])
}
#[test]
fn keys_are_stable_and_foreign_or_deleted_keys_never_alias() {
    let mut a = doc();
    let mut b = doc();
    let k = element(&mut a, "p");
    assert!(b.node(k).is_none());
    assert!(a.node(b.root()).is_none());
    a.mutate().remove(k).unwrap();
    assert!(a.node(k).is_some());
    a.mutate().remove_and_drop(k).unwrap();
    for _ in 0..1000 {
        assert_ne!(element(&mut a, "div"), k);
    }
    assert!(a.node(k).is_none());
    assert_ne!(element(&mut b, "p"), k);
    assert_eq!(a.mutate().remove(k), Err(MutationError::Stale(k)));
}
#[test]
fn reparent_insert_replace_and_detach_preserve_tree_links() {
    let mut d = doc();
    let root = d.root();
    let p = element(&mut d, "p");
    let q = element(&mut d, "div");
    let a = d.mutate().create_text("a");
    let b = d.mutate().create_text("b");
    d.mutate().append_children(root, &[p, q]).unwrap();
    d.mutate().append_children(p, &[a, b]).unwrap();
    d.mutate().insert_before(a, &[b]).unwrap();
    assert_eq!(d.node(p).unwrap().children().collect::<Vec<_>>(), [b, a]);
    d.mutate().reparent_children(p, q).unwrap();
    assert_eq!(d.node(p).unwrap().children().len(), 0);
    assert_eq!(d.node(a).unwrap().parent(), Some(q));
    d.mutate().replace_children(q, &[a]).unwrap();
    assert_eq!(d.node(b).unwrap().parent(), None);
    d.mutate().remove(a).unwrap();
    assert_eq!(d.node(a).unwrap().text(), Some("a"));
    assert_eq!(d.node(q).unwrap().children().len(), 0);
}
#[test]
fn rejected_mutations_are_atomic() {
    let mut d = doc();
    let root = d.root();
    let p = element(&mut d, "p");
    let c = element(&mut d, "b");
    d.mutate().append_children(root, &[p]).unwrap();
    d.mutate().append_children(p, &[c]).unwrap();
    let before = d.mutation_epoch();
    let foreign = doc().root();
    assert_eq!(
        d.mutate().append_children(c, &[p]),
        Err(MutationError::HierarchyCycle {
            parent: c,
            child: p
        })
    );
    assert_eq!(
        d.mutate().replace_children(p, &[c, foreign]),
        Err(MutationError::Stale(foreign))
    );
    assert_eq!(d.mutation_epoch(), before);
    assert_eq!(d.node(p).unwrap().children().collect::<Vec<_>>(), [c]);
    assert_eq!(d.node(p).unwrap().parent(), Some(root));
}
#[test]
fn duplicate_ids_follow_connected_tree_order_after_moves() {
    let mut d = doc();
    let root = d.root();
    let a = element(&mut d, "p");
    let b = element(&mut d, "p");
    for k in [a, b] {
        d.mutate()
            .set_attribute(k, attr("id"), "duplicate")
            .unwrap();
    }
    assert_eq!(d.element_by_id("duplicate"), None);
    d.mutate().append_children(root, &[a, b]).unwrap();
    assert_eq!(d.element_by_id("duplicate"), Some(a));
    d.mutate().append_children(root, &[a]).unwrap();
    assert_eq!(d.element_by_id("duplicate"), Some(b));
    d.mutate().remove(b).unwrap();
    assert_eq!(d.element_by_id("duplicate"), Some(a));
}
#[test]
fn namespaces_and_attribute_order_survive_mutation_and_clone() {
    let mut d = doc();
    let a = element(&mut d, "a");
    let x = QualName::new(Some("xlink".into()), ns!(xlink), "href".into());
    d.mutate().set_attribute(a, attr("href"), "local").unwrap();
    d.mutate().set_attribute(a, x.clone(), "foreign").unwrap();
    assert_eq!(
        wega_dom::attribute_ns(&d, a, ns!(xlink).as_ref(), "href"),
        Some("foreign".into())
    );
    let b = d.mutate().clone_node(a, false).unwrap();
    let attrs = d
        .node(b)
        .unwrap()
        .attributes()
        .map(|(n, v)| (n.clone(), v.to_owned()))
        .collect::<Vec<_>>();
    assert_eq!(
        attrs,
        [
            (attr("href"), "local".into()),
            (x.clone(), "foreign".into())
        ]
    );
    d.mutate().clear_attribute(a, x).unwrap();
    assert_eq!(d.node(a).unwrap().attr("href"), Some("local"));
}
#[test]
fn epoch_and_invalidations_change_only_for_effective_operations() {
    let mut d = doc();
    let a = element(&mut d, "p");
    let initial = d.mutation_epoch();
    d.mutate().remove(a).unwrap();
    d.mutate().append_children(a, &[]).unwrap();
    assert_eq!(d.mutation_epoch(), initial);
    d.mutate().set_attribute(a, attr("class"), "x").unwrap();
    let after = d.mutation_epoch();
    d.mutate().set_attribute(a, attr("class"), "x").unwrap();
    d.mutate().clear_attribute(a, attr("missing")).unwrap();
    assert_eq!(d.mutation_epoch(), after);
    let dirty = d.take_invalidations();
    assert!(dirty.style && dirty.layout && !dirty.structure);
    assert!(dirty.nodes.contains(&a));
    assert!(!d.take_invalidations().layout);
    let root = d.root();
    d.mutate().append_children(root, &[a]).unwrap();
    assert!(d.take_invalidations().structure);
}
#[test]
fn template_clone_has_separate_owned_fragment_and_iterative_cleanup() {
    let mut d = doc();
    let t = element(&mut d, "template");
    let f = d.mutate().template_contents(t).unwrap();
    assert!(d.is_fragment(f));
    assert_eq!(d.node(f).unwrap().parent(), None);
    let text = d.mutate().create_text("inside");
    d.mutate().append_children(f, &[text]).unwrap();
    let copy = d.mutate().clone_node(t, true).unwrap();
    let copied = d.mutate().template_contents(copy).unwrap();
    assert_ne!(f, copied);
    let ct = d.node(copied).unwrap().children().next().unwrap();
    assert_ne!(text, ct);
    assert_eq!(d.node(ct).unwrap().text(), Some("inside"));
    d.mutate().remove_and_drop(t).unwrap();
    assert!(d.node(f).is_none() && d.node(text).is_none());
    assert_eq!(d.node(ct).unwrap().text(), Some("inside"));
}
#[test]
fn deep_storage_clone_drop_and_document_drop_do_not_recurse() {
    // Storage robustness, not a change to the browser's accepted depth budget.
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let mut d = doc();
            let mut tree = element(&mut d, "leaf");
            // Build bottom-up to keep cycle validation linear for this stress case.
            for _ in 0..10_000 {
                let parent = element(&mut d, "div");
                d.mutate().append_children(parent, &[tree]).unwrap();
                tree = parent;
            }
            let count = d.node_count();
            let copy = d.mutate().clone_node(tree, true).unwrap();
            assert_eq!(d.node_count(), count * 2 - 1);
            d.mutate().remove_and_drop(copy).unwrap();
            assert_eq!(d.node_count(), count);
            d.mutate().remove_and_drop(tree).unwrap();
            assert_eq!(d.node_count(), 1);
            let mut tree = element(&mut d, "leaf");
            for _ in 0..10_000 {
                let parent = element(&mut d, "div");
                d.mutate().append_children(parent, &[tree]).unwrap();
                tree = parent;
            }
            drop(d);
        })
        .unwrap()
        .join()
        .unwrap();
}
#[test]
fn attribute_creation_and_missing_updates_deduplicate_by_namespace() {
    let mut d = doc();
    let a = d.mutate().create_element(
        name("p"),
        [
            NewAttribute {
                name: attr("id"),
                value: "first".into(),
            },
            NewAttribute {
                name: attr("id"),
                value: "second".into(),
            },
        ],
    );
    assert_eq!(d.node(a).unwrap().attr("id"), Some("first"));
    let before = d.mutation_epoch();
    d.mutate()
        .add_attributes_if_missing(
            a,
            [NewAttribute {
                name: attr("id"),
                value: "third".into(),
            }],
        )
        .unwrap();
    assert_eq!(d.mutation_epoch(), before);
}

#[test]
fn fragments_insert_contents_and_template_ownership_cannot_cycle() {
    let mut d = doc();
    let p = element(&mut d, "p");
    let f = d.create_fragment();
    let text = d.mutate().create_text("fragment");
    d.mutate().append_children(f, &[text]).unwrap();
    d.mutate().append_children(p, &[f]).unwrap();
    assert_eq!(d.node(p).unwrap().children().collect::<Vec<_>>(), [text]);
    assert_eq!(d.node(f).unwrap().children().len(), 0);
    assert_eq!(d.node(text).unwrap().parent(), Some(p));
    assert_eq!(d.node(f).unwrap().parent(), None);
    let t = element(&mut d, "template");
    let contents = d.mutate().template_contents(t).unwrap();
    let before = d.mutation_epoch();
    assert!(matches!(
        d.mutate().append_children(contents, &[t]),
        Err(MutationError::HierarchyCycle { .. })
    ));
    assert_eq!(d.mutation_epoch(), before);
    d.mutate().remove_and_drop(contents).unwrap();
    let replacement = d.mutate().template_contents(t).unwrap();
    assert_ne!(replacement, contents);
    assert!(d.is_fragment(replacement));
}
#[test]
fn repeated_collection_does_not_retain_deleted_keys_in_invalidation_set() {
    let mut d = doc();
    for _ in 0..5000 {
        let key = element(&mut d, "p");
        d.mutate().remove_and_drop(key).unwrap();
    }
    assert_eq!(d.node_count(), 1);
    let dirty = d.take_invalidations();
    assert!(dirty.structure);
    assert!(dirty.nodes.is_empty());
}
