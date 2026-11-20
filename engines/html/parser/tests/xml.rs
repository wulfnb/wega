use std::{cell::RefCell, rc::Rc};
use wega_dom::{Document, DocumentView, NodeKind, NodeView};
use wega_html::{HtmlDiagnostic, ParseReport, parse_xml};

struct Report(RefCell<Vec<String>>);
impl ParseReport for Report {
    fn warning(&self, d: HtmlDiagnostic<'_>) {
        self.0.borrow_mut().push(d.message.into_owned());
    }
}
fn document() -> Rc<RefCell<Document>> {
    Rc::new(RefCell::new(Document::new(
        url::Url::parse("https://wega.test/document.xhtml").unwrap(),
    )))
}

#[test]
fn xml_binds_namespaces_and_preserves_case_without_html_implied_elements() {
    let d = document();
    let report = Report(RefCell::new(vec![]));
    parse_xml(d.clone(), r#"<?xml version="1.0"?><root xmlns="urn:root" xmlns:q="urn:attr"><Item id="i" q:Name="A&amp;B"><![CDATA[<literal>]]></Item></root>"#, 100, &report).unwrap();
    let d = d.borrow();
    let root = d
        .node(d.root())
        .unwrap()
        .children()
        .find(|key| d.node(*key).unwrap().kind() == NodeKind::Element)
        .unwrap();
    let root_name = d.node(root).unwrap().element_name().unwrap();
    assert_eq!((root_name.local, root_name.namespace), ("root", "urn:root"));
    let item = d.element_by_id("i").unwrap();
    assert_eq!(d.node(item).unwrap().element_name().unwrap().local, "Item");
    assert_eq!(
        wega_dom::attribute_ns(&*d, item, "urn:attr", "Name").as_deref(),
        Some("A&B")
    );
    let text = d.node(item).unwrap().children().next().unwrap();
    assert_eq!(d.node(text).unwrap().text(), Some("<literal>"));
    assert!(report.0.borrow().is_empty(), "{:?}", report.0.borrow());
}

#[test]
fn xml_scripts_are_constructed_without_running_and_style_pop_is_delivered() {
    let d = document();
    let report = Report(RefCell::new(vec![]));
    parse_xml(d.clone(), r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>p{color:red}</style></head><body><script id="s">1 &lt; 2</script><p id="after"/></body></html>"#, 100, &report).unwrap();
    let mut d = d.borrow_mut();
    assert!(d.element_by_id("after").is_some());
    let script = d.element_by_id("s").unwrap();
    let text = d.node(script).unwrap().children().next().unwrap();
    assert_eq!(d.node(text).unwrap().text(), Some("1 < 2"));
    assert!(d.take_invalidations().style);
}

#[test]
fn xml_budget_stops_before_allocating_the_remaining_input() {
    let d = document();
    let report = Report(RefCell::new(vec![]));
    let xml = format!("<root>{}</root>", "<item>text</item>".repeat(50_000));
    assert!(parse_xml(d.clone(), &xml, 20, &report).is_err());
    assert!(d.borrow().node_count() <= 23, "{}", d.borrow().node_count());
    let zero = document();
    assert!(parse_xml(zero.clone(), "<root/>", 0, &report).is_err());
    assert_eq!(zero.borrow().node_count(), 2);
}

#[test]
fn xml_does_not_resolve_external_entities() {
    let d = document();
    let report = Report(RefCell::new(vec![]));
    parse_xml(
        d.clone(),
        r#"<!DOCTYPE root SYSTEM "file:///etc/passwd"><root>&external;</root>"#,
        100,
        &report,
    )
    .unwrap();
    let d = d.borrow();
    let nodes = wega_dom::subtree(&*d, d.root(), |k| k);
    let text: String = nodes
        .iter()
        .filter_map(|&k| d.node(k))
        .filter(|n| n.kind() == NodeKind::Text)
        .filter_map(NodeView::text)
        .collect();
    assert!(!text.contains("root:"));
    assert!(!report.0.borrow().is_empty());
}
