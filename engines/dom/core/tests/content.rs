use html5ever::{QualName, ns};
use wega_dom::{
    Document, DocumentMutation, DocumentView, MutationClock, MutationError, Mutator, NewAttribute,
    NodeKind, NodeView, content::*,
};
fn name(tag: &str) -> QualName {
    QualName::new(None, ns!(html), tag.into())
}
fn doc() -> Document {
    Document::new(url::Url::parse("https://wega.test/").unwrap())
}
fn attr(key: &str, value: &str) -> NewAttribute {
    NewAttribute {
        name: QualName::new(None, ns!(), key.into()),
        value: value.into(),
    }
}

#[test]
fn text_and_markup_use_the_owned_tree_in_document_order() {
    let mut d = doc();
    let root = d.root();
    let mut m = d.mutate();
    let e = m.create_element(name("div"), [attr("data-v", "a&\"b")]);
    let t = m.create_text("a<&>");
    let c = m.create_comment("comment");
    let script = m.create_element(name("script"), []);
    let raw = m.create_text("if(a < b && c > d) {}");
    let input = m.create_element(name("input"), []);
    m.append_children(script, &[raw]).unwrap();
    m.append_children(e, &[t, c, script, input]).unwrap();
    m.append_children(root, &[e]).unwrap();
    m.finish();
    assert_eq!(text_content(&d, root), None);
    assert_eq!(
        text_content(&d, e).as_deref(),
        Some("a<&>if(a < b && c > d) {}")
    );
    assert_eq!(text_content(&d, c).as_deref(), Some("comment"));
    let mut markup = String::new();
    serialize(&d, e, &mut markup);
    assert_eq!(
        markup,
        "<div data-v=\"a&amp;&quot;b\">a&lt;&amp;&gt;<!--comment--><script>if(a < b && c > d) {}</script><input></div>"
    );
}

#[test]
fn comment_mutations_are_atomic_and_counted_and_document_nodes_are_distinct() {
    let mut d = doc();
    let root = d.root();
    let c = d.mutate().create_comment("before");
    let t = d.mutate().create_text("text");
    let before = d.mutation_epoch();
    assert_eq!(
        d.mutate().set_comment(t, "bad"),
        Err(MutationError::NotText(t))
    );
    assert_eq!(d.mutation_epoch(), before);
    d.mutate().set_comment(c, "after").unwrap();
    assert_ne!(d.mutation_epoch(), before);
    let after = d.mutation_epoch();
    d.mutate().set_comment(c, "after").unwrap();
    assert_eq!(d.mutation_epoch(), after);
    assert_eq!(text_content(&d, c).as_deref(), Some("after"));
    d.mutate().remove_and_drop(c).unwrap();
    let after = d.mutation_epoch();
    assert_eq!(
        d.mutate().set_comment(c, "bad"),
        Err(MutationError::Stale(c))
    );
    assert_eq!(d.mutation_epoch(), after);
    let other = d.mutate().create_document();
    assert_ne!(other, root);
    assert_eq!(d.node(other).unwrap().kind(), NodeKind::Document);
    assert_eq!(d.root(), root);
}

#[test]
fn form_association_follows_current_tree_and_explicit_invalid_owner_stays_unowned() {
    let mut d = doc();
    let root = d.root();
    let mut m = d.mutate();
    let a = m.create_element(name("form"), [attr("id", "a")]);
    let b = m.create_element(name("form"), [attr("id", "b")]);
    let field = m.create_element(name("input"), []);
    m.append_children(root, &[a, b]).unwrap();
    m.append_children(a, &[field]).unwrap();
    m.finish();
    assert_eq!(form_owner(&d, field), Some(a));
    d.mutate().append_children(b, &[field]).unwrap();
    assert_eq!(form_owner(&d, field), Some(b));
    d.mutate()
        .set_attribute(field, attr("form", "a").name, "a")
        .unwrap();
    assert_eq!(form_owner(&d, field), Some(a));
    d.mutate()
        .set_attribute(field, attr("form", "missing").name, "missing")
        .unwrap();
    assert_eq!(form_owner(&d, field), None);
    d.mutate().remove(a).unwrap();
    assert_eq!(form_owner(&d, a), None);
}

#[test]
fn markup_and_text_traversals_do_not_recurse_through_a_deep_document() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let mut d = doc();
            let root = d.root();
            let mut m = d.mutate();
            let mut parent = root;
            for _ in 0..10_000 {
                let e = m.create_element(name("i"), []);
                m.append_children(parent, &[e]).unwrap();
                parent = e;
            }
            let text = m.create_text("leaf");
            m.append_children(parent, &[text]).unwrap();
            m.finish();
            let child = d.node(root).unwrap().children().next().unwrap();
            assert_eq!(text_content(&d, child).as_deref(), Some("leaf"));
            let mut out = String::new();
            serialize(&d, root, &mut out);
            assert_eq!(out.len(), 70_004);
        })
        .unwrap()
        .join()
        .unwrap();
}
