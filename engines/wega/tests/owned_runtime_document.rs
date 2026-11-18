//! Exercise the runtime services with the sole Wega tree, without a reference owner.
use blitz_traits::net::{AbortController, Bytes, NetHandler, NetProvider, Request};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};
use wega_dom::{
    DocumentMutation, DocumentView, MutationClock, Mutator, NodeView, input::FocusState,
};
use wega_engine::{
    NativeDocument,
    document::{DocumentSetup, WegaDocument},
};
struct Report;
impl wega_html::ParseReport for Report {
    fn warning(&self, _: wega_html::HtmlDiagnostic<'_>) {}
}
fn parse(html: &str, setup: DocumentSetup) -> Rc<RefCell<WegaDocument>> {
    let document = Rc::new(RefCell::new(WegaDocument::empty(setup)));
    let mut parser = wega_html::Parser::new(document.clone(), html, 30_000, Report);
    while !matches!(
        parser.step(),
        wega_html::Step::Finished | wega_html::Step::Overflow
    ) {}
    document
}
fn setup() -> DocumentSetup {
    DocumentSetup {
        base_url: Some("https://wega.test/base/page".into()),
        fonts: Some(Arc::new(vec![
            include_bytes!("../../vendor/blitz/blitz-dom/assets/moz-bullet-font.otf").to_vec(),
        ])),
        ..Default::default()
    }
}
#[test]
fn owned_cssom_edits_and_mutations_drive_the_existing_native_pipeline() {
    let document = parse(
        "<style id=s>#target{width:80px;height:30px}</style><div id=target></div>",
        setup(),
    );
    let target = document.borrow().element_by_id("target").unwrap();
    let sheet = document.borrow().element_by_id("s").unwrap();
    let mut native = NativeDocument::new(document.clone());
    native.ensure_layout();
    assert_eq!(native.bounding_rect(target).unwrap().width, 80.0);
    let epoch = document.borrow().mutation_epoch();
    document
        .borrow_mut()
        .stylesheet_insert_rule(sheet, &[], "#target{width:150px}", 1)
        .unwrap();
    assert_ne!(document.borrow().mutation_epoch(), epoch);
    native.ensure_layout();
    assert_eq!(native.bounding_rect(target).unwrap().width, 150.0);
    let text = document.borrow().node(sheet).unwrap().child_at(0).unwrap();
    document
        .borrow_mut()
        .mutate()
        .set_text(text, "#target{width:80px;height:30px}")
        .unwrap();
    native.ensure_layout();
    assert_eq!(native.bounding_rect(target).unwrap().width, 80.0);
    assert_eq!(document.borrow().stylesheet_rule_count(sheet, &[]), Some(1));
    document.borrow_mut().set_stylesheet_disabled(sheet, true);
    native.ensure_layout();
    assert_ne!(native.bounding_rect(target).unwrap().width, 80.0);
    let mut other = WegaDocument::empty(setup());
    assert!(other.node(target).is_none());
    assert!(other.mutate().remove(target).is_err());
}
#[test]
fn owned_queries_scope_relations_namespaces_and_live_control_state() {
    let document = parse(
        "<div id=scope><p class=x></p><!--ignored--><p class=x><b/></p><input id=i type=checkbox checked><textarea id=t>A😀B</textarea></div>",
        setup(),
    );
    let mut d = document.borrow_mut();
    let scope = d.element_by_id("scope").unwrap();
    assert_eq!(
        d.query_selector_all_in(scope, ":scope > p.x:nth-of-type(2):is(:not(:empty))")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(d.query_selector_all("p:empty").unwrap().len(), 1);
    assert_eq!(d.query_selector_all_in(scope, ":scope").unwrap().len(), 0);
    assert!(d.query_selector("p[").is_err());
    let reference =
        wega_adapter_blitz::BlitzDocument::from_html("<p><b></b></p>", Default::default());
    assert!(reference.query_selector("p:has(b)").is_err());
    assert!(d.query_selector("p:has(b)").is_err());
    let input = d.element_by_id("i").unwrap();
    assert_eq!(d.query_selector(":checked").unwrap(), Some(input));
    d.set_checked(input, false);
    assert!(d.query_selector(":checked").unwrap().is_none());
    let textarea = d.element_by_id("t").unwrap();
    assert_eq!(d.input_value(textarea).as_deref(), Some("A😀B"));
    d.set_focus(textarea).unwrap();
    assert_eq!(d.query_selector(":focus").unwrap(), Some(textarea));
    assert_eq!(d.focused(), Some(textarea));
    d.mutate().remove(textarea).unwrap();
    assert_eq!(d.focused(), None);
    assert_eq!(d.input_value(textarea).as_deref(), Some("A😀B"));
}
#[test]
fn owned_inline_declarations_keep_cssom_order_validation_and_priority() {
    let document = parse("<div id=t style='color:red;width:30px'></div>", setup());
    let mut d = document.borrow_mut();
    let target = d.element_by_id("t").unwrap();
    assert!(
        d.set_inline_property(target, "color", "blue", true)
            .unwrap()
    );
    let mut css = String::new();
    d.inline_style(target, &mut css).unwrap();
    assert_eq!(css, "color: blue !important; width: 30px;");
    let epoch = d.mutation_epoch();
    assert!(
        !d.set_inline_property(target, "width", "banana", false)
            .unwrap()
    );
    assert!(
        !d.set_inline_property(target, "unknown-property", "3px", false)
            .unwrap()
    );
    assert_eq!(d.mutation_epoch(), epoch);
    assert!(d.supports_condition("(display: grid) and (width: 2px)"));
    assert!(!d.supports_declaration("width", "banana"));
}
#[derive(Default)]
struct Pending(Mutex<Vec<(Request, Box<dyn NetHandler>)>>);
impl NetProvider for Pending {
    fn fetch(&self, _: usize, request: Request, handler: Box<dyn NetHandler>) {
        self.0.lock().unwrap().push((request, handler));
    }
}
impl Pending {
    fn complete(&self, path: &str, final_url: &str, source: &str) {
        let (request, handler) = {
            let mut pending = self.0.lock().unwrap();
            let index = pending
                .iter()
                .position(|(r, _)| r.url.path() == path)
                .unwrap();
            pending.remove(index)
        };
        assert_eq!(request.headers.get("sec-fetch-dest").unwrap(), "style");
        assert!(request.signal.is_some());
        handler.bytes(
            final_url.to_owned(),
            Bytes::copy_from_slice(source.as_bytes()),
        );
    }
}
#[test]
fn owned_resources_keep_requested_and_final_urls_and_ignore_stale_completions() {
    let pending = Arc::new(Pending::default());
    let abort = AbortController::default();
    let document = parse(
        "<link id=s rel=stylesheet href='/first'><div id=t></div>",
        DocumentSetup {
            net_provider: Some(pending.clone()),
            abort_signal: Some(abort.signal.clone()),
            ..setup()
        },
    );
    let sheet = document.borrow().element_by_id("s").unwrap();
    document
        .borrow_mut()
        .mutate()
        .set_attribute(
            sheet,
            html5ever::QualName::new(None, html5ever::ns!(), "href".into()),
            "/second",
        )
        .unwrap();
    pending.complete("/first", "https://foreign.test/old.css", "#t{width:999px}");
    document.borrow_mut().handle_messages();
    assert!(!document.borrow().node_has_stylesheet(sheet));
    pending.complete(
        "/second",
        "https://cdn.test/styles/final.css",
        "@import 'nested.css';#t{height:30px}",
    );
    document.borrow_mut().handle_messages();
    assert_eq!(
        document.borrow().stylesheet_url(sheet).unwrap().as_str(),
        "https://cdn.test/styles/final.css"
    );
    assert_eq!(
        pending.0.lock().unwrap()[0].0.url.as_str(),
        "https://cdn.test/styles/nested.css"
    );
    pending.complete(
        "/styles/nested.css",
        "https://cdn.test/styles/nested.css",
        "#t{width:75px}",
    );
    document.borrow_mut().handle_messages();
    let mut native = NativeDocument::new(document.clone());
    native.ensure_layout();
    let target = document.borrow().element_by_id("t").unwrap();
    assert_eq!(native.bounding_rect(target).unwrap().width, 75.0);
    document
        .borrow_mut()
        .mutate()
        .set_attribute(
            sheet,
            html5ever::QualName::new(None, html5ever::ns!(), "href".into()),
            "/late",
        )
        .unwrap();
    abort.abort();
    pending.complete("/late", "https://wega.test/late", "#t{width:900px}");
    document.borrow_mut().handle_messages();
    assert!(!document.borrow().node_has_stylesheet(sheet));
}
#[test]
fn owned_stylesheet_loader_keeps_the_eight_level_import_limit() {
    let pending = Arc::new(Pending::default());
    let abort = AbortController::default();
    let document = parse(
        "<link id=s rel=stylesheet href='/0'>",
        DocumentSetup {
            net_provider: Some(pending.clone()),
            abort_signal: Some(abort.signal.clone()),
            ..setup()
        },
    );
    for depth in 0..=8 {
        pending.complete(
            &format!("/{depth}"),
            &format!("https://wega.test/{depth}"),
            &format!("@import '/{}';", depth + 1),
        );
        document.borrow_mut().handle_messages();
    }
    assert!(
        pending.0.lock().unwrap().is_empty(),
        "a ninth nested import must be refused"
    );
}
