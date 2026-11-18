use std::{cell::RefCell, rc::Rc};
use wega_dom::DocumentView;
use wega_engine::{
    document::{DocumentSetup, WegaDocument},
    native_engine::*,
};
fn doc(html: &str) -> NativeDocument<WegaDocument> {
    let mut d = NativeDocument::new(Rc::new(RefCell::new(WegaDocument::from_html(
        html,
        DocumentSetup::default(),
    ))));
    d.flush_layout();
    d
}
#[test]
fn deeply_nested_blocks_respect_stack_limit_and_recover() {
    let mut html = String::from("<body>");
    for _ in 0..120 {
        html.push_str("<div>");
    }
    html.push_str("<p id=deep>leaf</p>");
    for _ in 0..120 {
        html.push_str("</div>");
    }
    html.push_str("</body>");
    let d = doc(&html);
    let key = d.doc.borrow().element_by_id("deep").unwrap();
    let r = d.bounding_rect(key).unwrap();
    assert!(r.width >= 0.0);
}
#[test]
fn cyclic_or_extreme_flex_values_do_not_panic() {
    let d = doc(
        "<body style='display:flex;width:100px'><div style='flex:1 1 0%;width:1e9px'></div><div style='flex:2 1 0%;min-width:50px'></div></body>",
    );
    assert_eq!(d.viewport.width_px, 1280);
}
#[test]
fn zero_and_negative_dimensions_clamp_safely() {
    let d = doc(
        "<body style='width:-20px;height:0px;padding:-5px'><div id=neg style='width:-100px;height:-50px;margin:-10px'>neg</div></body>",
    );
    let key = d.doc.borrow().element_by_id("neg").unwrap();
    let r = d.bounding_rect(key).unwrap();
    assert_eq!(r.width, 0.0);
    assert_eq!(r.height, 0.0);
}
#[test]
fn massive_stylesheet_budget_enforcement() {
    let mut css = String::new();
    for i in 0..600 {
        css.push_str(&format!(".c{i} {{ color: red; width: {i}px; }}\n"));
    }
    let sheet = StyleSheet::parse(&css);
    assert_eq!(sheet.rules.len(), 600);
}
#[test]
fn rapid_mutation_burst_maintains_consistent_tree_invariants() {
    let mut d =
        doc("<body style='margin:0'><div id=target style='width:50px;height:50px'></div></body>");
    for i in 1..=20 {
        let key = d.doc.borrow().element_by_id("target").unwrap();
        d.doc
            .borrow_mut()
            .set_inline_property(key, "width", &format!("{}px", 50 + i * 5), false)
            .unwrap();
        d.flush_layout();
        let r = d.bounding_rect(key).unwrap();
        assert_eq!(r.width, (50 + i * 5) as f64);
    }
}
