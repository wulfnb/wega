//! The wega-html parser against a real Blitz document: tree-construction parity with
//! blitz-html, fragment parsing, script pausing and node-budget termination.
use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::HtmlDocument;
use html5ever::{
    QualName,
    tendril::StrTendril,
    tree_builder::{QuirksMode, TreeBuilderOpts},
};
use std::{cell::RefCell, rc::Rc};
use wega_adapter_blitz::{BlitzDocument, ToNodeId};
use wega_dom::{DocumentView, Mutator, NodeKey, NodeView};
use wega_html::{FragmentOverflow, HtmlDiagnostic, ParseReport, Parser, Step, parse_fragment};

struct Silent;
impl ParseReport for Silent {
    fn warning(&self, _: HtmlDiagnostic<'_>) {}
}

fn wega_parse(html: &str, limit: usize) -> (Rc<RefCell<BlitzDocument>>, usize) {
    let document = Rc::new(RefCell::new(BlitzDocument::new(BaseDocument::new(
        DocumentConfig::default(),
    ))));
    let mut parser = Parser::new(document.clone(), html, limit, Silent);
    let mut scripts = 0;
    while let Step::Script(_) = parser.step() {
        scripts += 1;
    }
    (document, scripts)
}
fn serialized(doc: &BlitzDocument) -> String {
    doc.root_element().outer_html()
}

const CASES: &[&str] = &[
    "<!doctype html><title>t</title><body><p>one<b>two</b>three<!--c--></p>",
    // Foster parenting and misnested formatting exercise append_before_sibling and
    // append_based_on_parent_node.
    "<table><tr><td>a<td>b</table>text<table>x<tr><td>y</table>",
    "<p><b>bold<p>para</b>tail",
    // html/body attribute merging goes through add_attrs_if_missing.
    "<html a=1><body b=2><html a=9 c=3><body b=9 d=4><p>x",
    "<template id=t><i>in</i><template><b>nested</b></template></template><p>after",
    "<select><option>1<option>2<optgroup label=g><option>3</select>",
    "<ul><li>1<li>2<ul><li>n</ul></ul><svg><circle r=1 /><foreignObject><p>f</p></foreignObject></svg>",
    "<style>p{color:red}</style><p>styled</p><textarea>raw <b>text</b></textarea>",
    "<a href=x>1<a href=y>2</a></a><form><form><input></form></form>",
];

#[test]
fn contract_based_parser_builds_the_same_trees_as_blitz_html() {
    for html in CASES {
        let (document, _) = wega_parse(html, 100_000);
        let blitz = BlitzDocument::new(
            HtmlDocument::from_html(html, DocumentConfig::default()).into_inner(),
        );
        assert_eq!(
            serialized(&document.borrow()),
            serialized(&blitz),
            "tree differs for {html}"
        );
    }
}

#[test]
fn scripts_pause_the_parser_and_adjacent_text_merges() {
    let (document, scripts) = wega_parse("<p>a<script>1</script>b<script>2</script>c</p>", 100);
    assert_eq!(scripts, 2);
    let doc = document.borrow();
    let p = doc.query_selector("p").unwrap().unwrap();
    let kinds: Vec<_> = doc
        .get_node(p)
        .unwrap()
        .children
        .iter()
        .map(|id| doc.get_node(*id).unwrap().is_text_node())
        .collect();
    assert_eq!(kinds, vec![true, false, true, false, true]);
}

// ---- fragment parsing ----------------------------------------------------------------

/// The previous implementation (blitz-html's fragment sink), kept as the oracle.
fn blitz_fragment(document: &mut BlitzDocument, element: blitz_dom::NodeId, html: &str) {
    use html5ever::{ParseOpts, tendril::TendrilSink};
    let mut mutation = BaseDocument::mutate(&mut *document);
    let sink = blitz_html::DocumentHtmlParser::new(&mut mutation);
    let options = ParseOpts {
        tree_builder: TreeBuilderOpts {
            exact_errors: false,
            scripting_enabled: false,
            iframe_srcdoc: false,
            drop_doctype: true,
            quirks_mode: QuirksMode::NoQuirks,
        },
        ..Default::default()
    };
    let mut parser =
        html5ever::driver::parse_fragment_for_element(sink, options, element, false, None);
    parser.process(StrTendril::from_slice(html));
    parser.tokenizer.end();
    drop(parser);
    let root = mutation.doc.root_node().id;
    if let Some(fragment) = mutation.last_child_id(root) {
        let children = mutation.child_ids(fragment);
        mutation.append_children(element, &children);
        mutation.remove_and_drop_node(fragment);
    }
}
fn new_document() -> BlitzDocument {
    BlitzDocument::new(BaseDocument::new(DocumentConfig {
        base_url: Some("http://example.test/".into()),
        ..Default::default()
    }))
}
/// Adds a detached-from-nothing context element under the document node.
fn add_context(
    document: &mut BlitzDocument,
    namespace: html5ever::Namespace,
    name: &str,
) -> NodeKey {
    let root = DocumentView::root(document);
    let mut mutator = wega_dom::mutate(document);
    let context = mutator.create_element(
        QualName::new(None, namespace, html5ever::LocalName::from(name)),
        [],
    );
    mutator.append_children(root, &[context]).unwrap();
    mutator.finish();
    context
}
fn context_document(namespace: html5ever::Namespace, name: &str) -> (BlitzDocument, NodeKey) {
    let mut document = new_document();
    let context = add_context(&mut document, namespace, name);
    (document, context)
}
fn inner(document: &BlitzDocument, context: NodeKey) -> String {
    document
        .get_node(blitz_dom::NodeId::from_u64(context.as_u64()))
        .unwrap()
        .outer_html()
}

const FRAGMENTS: &[&str] = &[
    "plain <b>bold</b> text",
    "<p>one<p>two<div>three</div>",
    "<tr><td>cell<td>cell2<tr><td>row2",
    "<td>cell</td><th>head</th>",
    "<option>a<option>b<optgroup label=g><option>c",
    "<li>a<li>b<ul><li>n</ul>",
    "<template id=t><tr><td>in template</td></tr></template>after",
    "<template><template><b>nested</b></template></template>",
    "<svg viewBox='0 0 1 1'><circle r=1 /><foreignObject><p>html <b>in</b> svg</p></foreignObject><desc><i>d</i></desc></svg>",
    "<math><mi>x</mi><annotation-xml encoding='text/html'><p>h</p></annotation-xml></math>",
    "<b><i>misnested</b></i> text <a href=1>1<a href=2>2</a></a>",
    "<table><tr><td>a</td></tr>foster<b>x</b></table>",
    "<form><input name=a><form><input name=b></form></form>",
    "<script>if (a < b && c) {}</script><script src=x.js></script><noscript><p>ns</p></noscript>",
    "<style>p > a { color: red }</style><textarea><b>raw</b></textarea><title>t & u</title>",
    "<!-- comment --><?pi data?><!doctype html>text &amp; &lt; &#x41; &nbsp;",
    "<html a=1><body b=2><head><meta charset=x></head>x",
    "<frameset><frame></frameset><plaintext><b>not a tag",
    "<img src=x alt='a'><br><hr><input type=checkbox checked>",
    "<ruby>a<rt>b<rp>(</rp></ruby><details><summary>s</summary>d</details>",
    "",
    "   ",
    "&amp;",
];
const CONTEXTS: &[(&str, &str)] = &[
    ("http://www.w3.org/1999/xhtml", "div"),
    ("http://www.w3.org/1999/xhtml", "body"),
    ("http://www.w3.org/1999/xhtml", "table"),
    ("http://www.w3.org/1999/xhtml", "tbody"),
    ("http://www.w3.org/1999/xhtml", "tr"),
    ("http://www.w3.org/1999/xhtml", "td"),
    ("http://www.w3.org/1999/xhtml", "select"),
    ("http://www.w3.org/1999/xhtml", "colgroup"),
    ("http://www.w3.org/1999/xhtml", "textarea"),
    ("http://www.w3.org/1999/xhtml", "script"),
    ("http://www.w3.org/1999/xhtml", "style"),
    ("http://www.w3.org/1999/xhtml", "title"),
    ("http://www.w3.org/1999/xhtml", "template"),
    ("http://www.w3.org/1999/xhtml", "p"),
    ("http://www.w3.org/2000/svg", "svg"),
    ("http://www.w3.org/2000/svg", "foreignObject"),
    ("http://www.w3.org/1998/Math/MathML", "math"),
];

#[test]
fn generic_fragment_parsing_matches_the_blitz_html_fragment_sink() {
    let (mut expected, mut actual) = (new_document(), new_document());
    for (namespace, name) in CONTEXTS {
        for html in FRAGMENTS {
            let expected_context =
                add_context(&mut expected, html5ever::Namespace::from(*namespace), name);
            blitz_fragment(&mut expected, expected_context.node_id(), html);
            let context = add_context(&mut actual, html5ever::Namespace::from(*namespace), name);
            parse_fragment(&mut actual, context, usize::MAX, html, &Silent).unwrap();
            assert_eq!(
                inner(&actual, context),
                inner(&expected, expected_context),
                "fragment {html:?} in <{name}>"
            );
            // The temporary parser root is gone: only context elements hang below the
            // document node, and both implementations left the same number of them.
            assert_eq!(
                actual.root_node().children.len(),
                expected.root_node().children.len(),
                "{html:?} in <{name}>"
            );
        }
    }
    assert_tree_is_consistent(&actual);
}

#[test]
fn fragment_parsing_moves_nodes_in_place_without_copying_them() {
    let (mut document, context) = context_document(html5ever::ns!(html), "div");
    parse_fragment(
        &mut document,
        context,
        usize::MAX,
        "<p id=a>x</p><p id=b>y</p>",
        &Silent,
    )
    .unwrap();
    let a = DocumentView::element_by_id(&document, "a").unwrap();
    let before: Vec<NodeKey> = document.node(context).unwrap().children().collect();
    assert_eq!(before.len(), 2);
    assert_eq!(before[0], a, "the parsed node is the node in the tree");
    assert_eq!(document.node(a).unwrap().parent(), Some(context));
    // Parsing again replaces nothing by itself: the caller decides (it appends here).
    parse_fragment(&mut document, context, usize::MAX, "<i>z</i>", &Silent).unwrap();
    assert_eq!(document.node(context).unwrap().children().len(), 3);
    // Nodes parsed earlier keep their identity.
    assert_eq!(DocumentView::element_by_id(&document, "a"), Some(a));
}

#[test]
fn fragment_scripts_stay_inert_and_do_not_pause() {
    let (mut document, context) = context_document(html5ever::ns!(html), "div");
    parse_fragment(
        &mut document,
        context,
        usize::MAX,
        "<script>1</script>a<script>2</script>b<p>c</p>",
        &Silent,
    )
    .unwrap();
    let kinds: Vec<_> = document
        .node(context)
        .unwrap()
        .children()
        .map(|k| {
            document
                .node(k)
                .unwrap()
                .qual_name()
                .map(|n| n.local.to_string())
        })
        .collect();
    assert_eq!(
        kinds.len(),
        5,
        "both scripts and all following input parsed: {kinds:?}"
    );
}

#[test]
fn fragment_parsing_respects_the_node_budget_and_leaves_the_document_valid() {
    let html = "<p>x</p>".repeat(10_000);
    for limit in [0, 1, 10, 100] {
        let (mut document, context) = context_document(html5ever::ns!(html), "div");
        let before = assert_tree_is_consistent(&document);
        assert_eq!(
            parse_fragment(&mut document, context, limit, &html, &Silent),
            Err(FragmentOverflow)
        );
        // The temporary root and everything under it are gone; the context is untouched.
        assert_eq!(
            document.node(context).unwrap().children().len(),
            0,
            "limit {limit}"
        );
        assert!(
            assert_tree_is_consistent(&document) <= before + OVERSHOOT,
            "limit {limit}"
        );
    }
    // Within the budget it succeeds (the temporary root counts too).
    let (mut document, context) = context_document(html5ever::ns!(html), "div");
    assert!(parse_fragment(&mut document, context, 100, "<p>a</p><p>b</p>", &Silent).is_ok());
    // Every construction case survives a spread of budgets in a table context.
    let mut document = new_document();
    for fragment in FRAGMENTS {
        for limit in [0, 1, 2, 3, 5, 8, 13, 21, usize::MAX] {
            let context = add_context(&mut document, html5ever::ns!(html), "table");
            let _ = parse_fragment(&mut document, context, limit, fragment, &Silent);
        }
        assert_tree_is_consistent(&document);
    }
}

// ---- node budget -------------------------------------------------------------------
// Policy: crossing `Limits::dom_nodes` aborts the parse (`Step::Overflow`); the tokenizer
// stops after the token that crossed it, no script runs afterwards, and the document keeps
// the valid partial tree built so far. The engine then replaces it with the limit error page.

/// Everything a budget test needs to know about one parse.
struct Outcome {
    document: Rc<RefCell<BlitzDocument>>,
    parser: Parser<BlitzDocument, Silent>,
    scripts: usize,
    overflowed: bool,
}
fn budgeted(html: &str, limit: usize) -> Outcome {
    let document = Rc::new(RefCell::new(BlitzDocument::new(BaseDocument::new(
        DocumentConfig::default(),
    ))));
    let mut parser = Parser::new(document.clone(), html, limit, Silent);
    let (mut scripts, mut overflowed) = (0, false);
    loop {
        match parser.step() {
            Step::Script(_) => scripts += 1,
            Step::Overflow => {
                overflowed = true;
                break;
            }
            Step::Finished => break,
        }
    }
    Outcome {
        document,
        parser,
        scripts,
        overflowed,
    }
}
fn created(html: &str) -> usize {
    budgeted(html, usize::MAX).parser.created()
}
/// Every reachable node is linked consistently: parent/child agree, no node appears twice.
fn assert_tree_is_consistent(document: &BlitzDocument) -> usize {
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![document.root_node().id];
    while let Some(id) = stack.pop() {
        assert!(seen.insert(id), "node {id} is reachable twice");
        for child in &document.get_node(id).unwrap().children {
            assert_eq!(
                document.get_node(*child).unwrap().parent,
                Some(id),
                "child {child} does not point back at {id}"
            );
            stack.push(*child);
        }
    }
    seen.len()
}
/// The partial document stays usable: it serialises, queries and takes further mutations.
fn assert_document_is_usable(document: &Rc<RefCell<BlitzDocument>>) {
    assert_tree_is_consistent(&document.borrow());
    // `root_element()` needs an <html> element, which a comment-only prefix never made.
    if document.borrow().try_root_element().is_some() {
        let _ = serialized(&document.borrow());
    }
    let _ = document.borrow().query_selector_all("p");
    let mut doc = document.borrow_mut();
    let root = DocumentView::root(&*doc);
    let mut mutator = wega_dom::mutate(&mut *doc);
    let name = QualName::new(None, html5ever::ns!(html), html5ever::LocalName::from("i"));
    let element = mutator.create_element(name, []);
    mutator.append_children(root, &[element]).unwrap();
    mutator.finish();
    assert!(DocumentView::node(&*doc, element).is_some());
    drop(doc);
    assert_tree_is_consistent(&document.borrow());
}
/// One token may need several nodes (implied elements, re-opened formatting), so the
/// overshoot is small and bounded; it never scales with the input.
const OVERSHOOT: usize = 64;

#[test]
fn a_document_exactly_at_the_limit_parses_completely() {
    let html = format!("<body>{}", "<p>x</p>".repeat(10));
    let total = created(&html);
    let outcome = budgeted(&html, total);
    assert!(!outcome.overflowed);
    assert!(outcome.parser.is_finished());
    assert_eq!(
        outcome
            .document
            .borrow()
            .query_selector_all("p")
            .unwrap()
            .len(),
        10
    );
    assert_document_is_usable(&outcome.document);
}

#[test]
fn a_document_one_node_over_the_limit_overflows() {
    let html = format!("<body>{}", "<p>x</p>".repeat(10));
    let total = created(&html);
    let outcome = budgeted(&html, total - 1);
    assert!(outcome.overflowed);
    assert!(outcome.parser.is_finished(), "the parser is not resumable");
    assert!(outcome.parser.created() <= total);
    assert_document_is_usable(&outcome.document);
}

#[test]
fn the_original_400_paragraphs_with_a_budget_of_100_no_longer_panic() {
    let html = format!("<body>{}", "<p>x</p>".repeat(400));
    let outcome = budgeted(&html, 100);
    assert!(outcome.overflowed);
    let count = outcome
        .document
        .borrow()
        .query_selector_all("p")
        .unwrap()
        .len();
    assert!(count <= 100, "{count} paragraphs built");
    assert!(outcome.parser.created() <= 100 + OVERSHOOT);
    assert_document_is_usable(&outcome.document);
}

#[test]
fn a_huge_overflow_stops_early_and_allocates_only_the_budget() {
    let html = format!("<body>{}", "<p>x</p>".repeat(200_000));
    let outcome = budgeted(&html, 1_000);
    assert!(outcome.overflowed);
    assert!(outcome.parser.created() <= 1_000 + OVERSHOOT);
    assert!(assert_tree_is_consistent(&outcome.document.borrow()) <= 1_000 + OVERSHOOT + 8);
}

#[test]
fn nested_elements_and_reopened_formatting_stay_bounded() {
    let nested = "<div>".repeat(5_000);
    let outcome = budgeted(&nested, 50);
    assert!(outcome.overflowed);
    assert!(outcome.parser.created() <= 50 + OVERSHOOT);
    assert_document_is_usable(&outcome.document);

    // Misnested formatting makes the tree builder re-open every active formatting element
    // when text follows; those nodes are counted and the stop still comes next token.
    let formatting = format!("<p>{}</p>tail<p>tail", "<b>".repeat(40));
    for limit in [5, 20, 40, 45, 60, 85] {
        let outcome = budgeted(&formatting, limit);
        assert!(
            outcome.parser.created() <= limit + OVERSHOOT,
            "limit {limit}: {} nodes",
            outcome.parser.created()
        );
        assert_document_is_usable(&outcome.document);
    }
}

#[test]
fn floods_of_non_tag_tokens_stop_at_the_budget_too() {
    // Comments and text are not tag tokens, so they cannot use a tokenizer script pause.
    for html in [
        "<!--c-->".repeat(100_000),
        "a<!---->".repeat(100_000),
        "<?pi?>".repeat(100_000),
    ] {
        let outcome = budgeted(&html, 100);
        assert!(outcome.overflowed);
        assert!(outcome.parser.created() <= 100 + OVERSHOOT);
        assert_document_is_usable(&outcome.document);
    }
}

#[test]
fn tables_and_templates_overflow_without_corrupting_the_tree() {
    let table = format!("<table>{}</table>", "<tr><td>a<td>b".repeat(200));
    let template = format!(
        "<template>{}</template><p>after",
        "<div><span>x</span></div>".repeat(200)
    );
    let foster = format!("<table>{}</table>", "text<b>x</b><tr><td>c".repeat(100));
    for html in [table, template, foster] {
        for limit in [1, 2, 7, 30, 101] {
            let outcome = budgeted(&html, limit);
            assert!(outcome.overflowed, "limit {limit}");
            assert!(outcome.parser.created() <= limit + OVERSHOOT);
            assert_document_is_usable(&outcome.document);
        }
    }
}

#[test]
fn scripts_stop_running_once_the_budget_is_crossed() {
    // Two scripts fit; the third closes after the budget is already exceeded.
    let html =
        "<script>1</script><script>2</script><p>a<p>b<p>c<p>d<script>3</script><script>4</script>";
    let fits = created("<script>1</script><script>2</script>");
    let outcome = budgeted(html, fits + 2);
    assert!(outcome.overflowed);
    assert_eq!(
        outcome.scripts, 2,
        "no script closes after the budget is crossed"
    );
    // A script whose own end tag crosses the budget does not run either.
    let outcome = budgeted("<script>1</script>", 1);
    assert!(outcome.overflowed);
    assert_eq!(outcome.scripts, 0);
    assert_document_is_usable(&outcome.document);
}

#[test]
fn chunked_input_and_document_write_respect_the_budget() {
    let document = Rc::new(RefCell::new(BlitzDocument::new(BaseDocument::new(
        DocumentConfig::default(),
    ))));
    let mut parser = Parser::new(
        document.clone(),
        "<p>a</p><script>w</script>tail",
        40,
        Silent,
    );
    assert!(matches!(parser.step(), Step::Script(_)));
    // document.write puts a large chunk at the insertion point.
    parser.write(&"<p>x</p>".repeat(500));
    assert!(matches!(parser.step(), Step::Overflow));
    assert!(parser.is_finished());
    assert!(parser.created() <= 40 + OVERSHOOT);
    // Writes after termination are ignored, and stepping again stays terminal.
    let before = parser.created();
    parser.write("<p>late</p>");
    assert!(matches!(parser.step(), Step::Finished));
    assert_eq!(parser.created(), before);
    assert_document_is_usable(&document);
}

#[test]
fn every_budget_boundary_of_every_construction_case_is_safe() {
    for html in CASES {
        let total = created(html);
        for limit in 0..=total + 1 {
            let outcome = budgeted(html, limit);
            assert_eq!(outcome.overflowed, limit < total, "{html} @ {limit}");
            assert!(outcome.parser.created() <= limit.max(total) + OVERSHOOT);
            assert_tree_is_consistent(&outcome.document.borrow());
        }
    }
}

#[test]
fn the_parser_recovers_for_the_next_document() {
    let first = budgeted(&"<p>x</p>".repeat(100), 10);
    assert!(first.overflowed);
    drop(first);
    let second = budgeted("<!doctype html><title>t</title><body><p>fine</p>", 100);
    assert!(!second.overflowed);
    assert_eq!(
        second
            .document
            .borrow()
            .query_selector_all("p")
            .unwrap()
            .len(),
        1
    );
}
