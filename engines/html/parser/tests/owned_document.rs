//! The real html5ever tree sink constructing only the Wega-owned document.
use std::{cell::RefCell, rc::Rc};
use wega_dom::{Document, DocumentMutation, DocumentView, Mutator, NodeView};
use wega_html::{HtmlDiagnostic, ParseReport, Parser, Step};
struct Quiet;
impl ParseReport for Quiet {
    fn warning(&self, _: HtmlDiagnostic<'_>) {}
}
fn parse(html: &str) -> Rc<RefCell<Document>> {
    let doc = Rc::new(RefCell::new(Document::new(
        url::Url::parse("https://wega.test/").unwrap(),
    )));
    let mut parser = Parser::new(doc.clone(), html, 30_000, Quiet);
    loop {
        match parser.step() {
            Step::Finished => break,
            Step::Script(_) => {}
            Step::Overflow => panic!("unexpected overflow"),
        }
    }
    doc
}
fn text(d: &Document, key: wega_dom::NodeKey) -> String {
    let mut out = String::new();
    let mut stack = vec![key];
    while let Some(k) = stack.pop() {
        let n = d.node(k).unwrap();
        if n.kind() == wega_dom::NodeKind::Text {
            out.push_str(n.text().unwrap());
        }
        stack.extend(n.children().rev());
    }
    out
}
#[test]
fn malformed_html_entities_and_implied_structure_use_html5ever() {
    let d = parse(
        "<!doctype html><title>A &amp; B</title><p id=p>one<b>two</p><p id=q>three&nbsp;four",
    );
    let d = d.borrow();
    let root = d.node(d.root()).unwrap().children().next().unwrap();
    assert_eq!(d.node(root).unwrap().element_name().unwrap().local, "html");
    assert_eq!(text(&d, d.element_by_id("p").unwrap()), "onetwo");
    assert_eq!(text(&d, d.element_by_id("q").unwrap()), "three\u{a0}four");
}
#[test]
fn table_foster_parenting_and_implicit_tbody_are_preserved() {
    let d = parse("<div id=host><table id=t>before<tr><td id=cell>cell</table>after</div>");
    let d = d.borrow();
    let cell = d.element_by_id("cell").unwrap();
    let tr = d.node(cell).unwrap().parent().unwrap();
    let tbody = d.node(tr).unwrap().parent().unwrap();
    assert_eq!(
        d.node(tbody).unwrap().element_name().unwrap().local,
        "tbody"
    );
    let host = d.element_by_id("host").unwrap();
    let first = d.node(host).unwrap().children().next().unwrap();
    assert_eq!(d.node(first).unwrap().text(), Some("before"));
}
#[test]
fn svg_namespaces_and_template_contents_are_real_nodes() {
    let d = parse(
        "<svg><a id=svg-link xlink:href='#target'>SVG</a></svg><template id=template><p id=hidden>inside</p></template>",
    );
    let mut d = d.borrow_mut();
    let svg = d.element_by_id("svg-link").unwrap();
    assert_eq!(
        d.node(svg).unwrap().element_name().unwrap().namespace,
        "http://www.w3.org/2000/svg"
    );
    assert_eq!(
        wega_dom::attribute_ns(&*d, svg, "http://www.w3.org/1999/xlink", "href"),
        Some("#target".into())
    );
    assert!(d.element_by_id("hidden").is_none());
    let template = d.element_by_id("template").unwrap();
    let fragment = d.mutate().template_contents(template).unwrap();
    assert_eq!(text(&d, fragment), "inside");
}
#[test]
fn script_pause_and_document_write_use_the_same_tree() {
    let d = Rc::new(RefCell::new(Document::new(
        url::Url::parse("https://wega.test/").unwrap(),
    )));
    let mut parser = Parser::new(
        d.clone(),
        "<p id=before>before</p><script id=script>pause</script><p id=after>after</p>",
        30_000,
        Quiet,
    );
    let Step::Script(script) = parser.step() else {
        panic!("script not paused")
    };
    assert_eq!(d.borrow().element_by_id("script"), Some(script));
    assert!(d.borrow().element_by_id("after").is_none());
    parser.write("<p id=written>insertion point</p>");
    assert!(matches!(parser.step(), Step::Finished));
    assert!(
        d.borrow().element_by_id("written").is_some()
            && d.borrow().element_by_id("after").is_some()
    );
}
#[test]
fn fragment_context_and_overflow_cleanup_preserve_existing_document() {
    let d = parse("<table id=t></table><p id=keep>keep</p>");
    let mut d = d.borrow_mut();
    let t = d.element_by_id("t").unwrap();
    wega_html::parse_fragment(&mut *d, t, 100, "<tr><td>cell&amp;entity</td></tr>", &Quiet)
        .unwrap();
    assert_eq!(text(&d, t), "cell&entity");
    let before = d.node_count();
    let root_children = d.node(d.root()).unwrap().children().collect::<Vec<_>>();
    assert!(
        wega_html::parse_fragment(&mut *d, t, 2, "<div><b><i>too many</i></b></div>", &Quiet)
            .is_err()
    );
    assert_eq!(d.node_count(), before);
    assert_eq!(
        d.node(d.root()).unwrap().children().collect::<Vec<_>>(),
        root_children
    );
    assert!(d.element_by_id("keep").is_some());
}
#[test]
fn parser_budget_stops_real_node_creation() {
    let d = Rc::new(RefCell::new(Document::new(
        url::Url::parse("https://wega.test/").unwrap(),
    )));
    let mut parser = Parser::new(d.clone(), &"<p>text</p>".repeat(1000), 20, Quiet);
    assert!(matches!(parser.step(), Step::Overflow));
    assert!(parser.created() <= 23);
    assert!(d.borrow().node_count() <= 24);
}
