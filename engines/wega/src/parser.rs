//! Incremental HTML parsing into Blitz's DOM. Unlike `blitz_html`'s one-shot
//! parser, this one pauses after every `</script>` so that the script runs
//! against the partially parsed document, and accepts `document.write` input at
//! the insertion point.
//!
//! The tree sink is adapted from blitz-html 0.3.0-beta.4 `html_sink.rs`
//! (MIT OR Apache-2.0, DioxusLabs); modified by Wega contributors, 2026, to
//! borrow the shared document per operation instead of holding one mutator for
//! the whole parse.
use crate::dom_model::{DocumentMutation, MutationError, Mutator, NewAttribute, NodeKey, NodeView};
use blitz_dom::{BaseDocument, NodeId};
use html5ever::{
    QualName,
    tendril::StrTendril,
    tokenizer::{BufferQueue, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts},
    tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeBuilder, TreeBuilderOpts, TreeSink},
};
use std::{
    borrow::Cow,
    cell::{Cell, Ref, RefCell},
    rc::Rc,
};

/// The tree sink, generic over the document it builds into (`dom_model::DocumentMutation`).
/// Each tree-construction operation uses its own mutation scope, as before.
pub(crate) struct Sink<D: DocumentMutation = BaseDocument> {
    document: Rc<RefCell<D>>,
    /// Nodes created so far, and the node budget. Every node the tree builder is given is a
    /// real, distinct node (html5ever identifies open elements by handle equality, so
    /// placeholders would corrupt its stacks). Crossing the budget only sets `overflow`;
    /// `BudgetedBuilder` then stops the tokenizer after the current token, so the overshoot
    /// is bounded by one token's tree-construction work.
    created: Cell<usize>,
    limit: usize,
    line: Cell<u64>,
    overflow: Cell<bool>,
}
fn attribute(attr: html5ever::Attribute) -> NewAttribute {
    NewAttribute {
        name: attr.name,
        value: attr.value.to_string(),
    }
}
impl<D: DocumentMutation> Sink<D> {
    fn mutate<R>(&self, f: impl FnOnce(&mut D::Mutator<'_>) -> R) -> R {
        let mut document = self.document.borrow_mut();
        let mut mutator = document.mutate();
        f(&mut mutator)
    }
    /// The parser only passes keys it created, so a rejected operation means a tree
    /// construction bug; report it as an HTML warning and keep parsing.
    fn applied(&self, result: Result<(), MutationError>) {
        if let Err(error) = result {
            let mut event = crate::diagnostics::Diagnostic::error(
                crate::diagnostics::Category::Html,
                format!("HTML tree construction was rejected: {error}"),
                Some(self.document.borrow().base_url().to_string()),
            );
            event.severity = "warning".into();
            event.line = Some(self.line.get());
            event.emit();
        }
    }
    fn overflowed(&self) -> bool {
        self.overflow.get()
    }
    /// Count a node about to be created; crossing the budget requests a stop.
    fn count_node(&self) {
        let created = self.created.get().saturating_add(1);
        self.created.set(created);
        if created > self.limit {
            self.overflow.set(true);
        }
    }
}
impl<D: DocumentMutation> TreeSink for Sink<D> {
    type Output = ();
    type Handle = NodeKey;
    type ElemName<'a>
        = Ref<'a, QualName>
    where
        Self: 'a;
    fn finish(self) {}
    fn parse_error(&self, msg: Cow<'static, str>) {
        let mut event = crate::diagnostics::Diagnostic::error(
            crate::diagnostics::Category::Html,
            msg.into_owned(),
            Some(self.document.borrow().base_url().to_string()),
        );
        event.severity = "warning".into();
        event.line = Some(self.line.get());
        event.emit();
    }
    fn set_current_line(&self, line_number: u64) {
        self.line.set(line_number);
    }
    fn get_document(&self) -> NodeKey {
        self.document.borrow().root()
    }
    fn elem_name<'a>(&'a self, target: &'a NodeKey) -> Ref<'a, QualName> {
        Ref::map(self.document.borrow(), |doc| {
            doc.node(*target)
                .and_then(NodeView::qual_name)
                .expect("TreeSink::elem_name called on a node which is not an element")
        })
    }
    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<html5ever::Attribute>,
        _flags: ElementFlags,
    ) -> NodeKey {
        self.count_node();
        self.mutate(|m| m.create_element(name, attrs.into_iter().map(attribute)))
    }
    fn create_comment(&self, text: StrTendril) -> NodeKey {
        self.count_node();
        self.mutate(|m| m.create_comment(&text))
    }
    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> NodeKey {
        self.count_node();
        self.mutate(|m| m.create_comment(""))
    }
    fn append(&self, parent: &NodeKey, child: NodeOrText<NodeKey>) {
        let result = self.mutate(|m| match child {
            NodeOrText::AppendNode(id) => m.append_children(*parent, &[id]),
            NodeOrText::AppendText(text) => {
                let appended = m
                    .last_child(*parent)
                    .is_some_and(|id| m.append_text(id, &text).is_ok());
                if appended {
                    Ok(())
                } else {
                    self.count_node();
                    let id = m.create_text(&text);
                    m.append_children(*parent, &[id])
                }
            }
        });
        self.applied(result);
    }
    fn append_before_sibling(&self, sibling: &NodeKey, new_node: NodeOrText<NodeKey>) {
        let result = self.mutate(|m| match new_node {
            NodeOrText::AppendNode(id) => m.insert_before(*sibling, &[id]),
            NodeOrText::AppendText(text) => {
                let appended = m
                    .previous_sibling(*sibling)
                    .is_some_and(|id| m.append_text(id, &text).is_ok());
                if appended {
                    Ok(())
                } else {
                    self.count_node();
                    let id = m.create_text(&text);
                    m.insert_before(*sibling, &[id])
                }
            }
        });
        self.applied(result);
    }
    fn append_based_on_parent_node(
        &self,
        element: &NodeKey,
        prev_element: &NodeKey,
        child: NodeOrText<NodeKey>,
    ) {
        let has_parent = self.mutate(|m| m.parent(*element).is_some());
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }
    fn append_doctype_to_document(
        &self,
        _name: StrTendril,
        _public_id: StrTendril,
        _system_id: StrTendril,
    ) {
    }
    fn get_template_contents(&self, target: &NodeKey) -> NodeKey {
        match self.mutate(|m| m.template_contents(*target)) {
            Ok(contents) => contents,
            Err(error) => {
                self.applied(Err(error));
                *target
            }
        }
    }
    fn same_node(&self, x: &NodeKey, y: &NodeKey) -> bool {
        x == y
    }
    fn set_quirks_mode(&self, _mode: QuirksMode) {}
    fn add_attrs_if_missing(&self, target: &NodeKey, attrs: Vec<html5ever::Attribute>) {
        let result =
            self.mutate(|m| m.add_attributes_if_missing(*target, attrs.into_iter().map(attribute)));
        self.applied(result);
    }
    fn remove_from_parent(&self, target: &NodeKey) {
        let result = self.mutate(|m| m.remove(*target));
        self.applied(result);
    }
    fn reparent_children(&self, old_parent: &NodeKey, new_parent: &NodeKey) {
        let result = self.mutate(|m| m.reparent_children(*old_parent, *new_parent));
        self.applied(result);
    }
    /// A `<style>` element's text arrives after the element is inserted, and each
    /// operation uses its own mutation scope, so reprocess the sheet once it is closed.
    fn pop(&self, node: &NodeKey) {
        let is_style = self
            .document
            .borrow()
            .node(*node)
            .and_then(NodeView::qual_name)
            .is_some_and(|name| &*name.local == "style");
        if is_style {
            self.document.borrow_mut().refresh_style_element(*node);
        }
    }
}

/// What the parser did on one step.
pub(crate) enum Step {
    /// A parser-inserted `<script>` element was closed; run or fetch it next.
    Script(NodeKey),
    /// The document has more nodes than the budget allows.
    Overflow,
    /// All input was consumed and the document is complete.
    Finished,
}

/// The tree builder, plus the node budget: once the sink reports overflow, the unread input is
/// discarded so the tokenizer runs dry after the current token and is never resumed. (A
/// script pause would only be legal for tag tokens; draining the queue works for every token
/// kind, including runs of comments.)
struct BudgetedBuilder<D: DocumentMutation> {
    builder: TreeBuilder<NodeKey, Sink<D>>,
    input: Rc<BufferQueue>,
}
impl<D: DocumentMutation> TokenSink for BudgetedBuilder<D> {
    type Handle = NodeKey;
    fn process_token(&self, token: Token, line_number: u64) -> TokenSinkResult<NodeKey> {
        let result = self.builder.process_token(token, line_number);
        if self.builder.sink.overflowed() {
            // `Parser::step` reports overflow before it looks at any script result.
            self.input.replace_with(BufferQueue::default());
        }
        result
    }
    fn end(&self) {
        self.builder.end();
    }
    fn adjusted_current_node_present_but_not_in_html_namespace(&self) -> bool {
        self.builder
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}

pub(crate) struct Parser<D: DocumentMutation = BaseDocument> {
    tokenizer: Tokenizer<BudgetedBuilder<D>>,
    input: Rc<BufferQueue>,
    finished: bool,
}
impl<D: DocumentMutation> Parser<D> {
    pub fn new(document: Rc<RefCell<D>>, html: &str, node_limit: usize) -> Self {
        let builder = TreeBuilder::new(
            Sink {
                document,
                created: Cell::new(0),
                line: Cell::new(1),
                limit: node_limit,
                overflow: Cell::new(false),
            },
            TreeBuilderOpts {
                // Scripts run, so <noscript> content is raw text.
                scripting_enabled: true,
                drop_doctype: true,
                ..Default::default()
            },
        );
        let input = Rc::new(BufferQueue::default());
        let tokenizer = Tokenizer::new(
            BudgetedBuilder {
                builder,
                input: input.clone(),
            },
            TokenizerOpts::default(),
        );
        input.push_back(StrTendril::from_slice(html));
        Self {
            tokenizer,
            input,
            finished: false,
        }
    }
    /// Nodes the parser has asked the document to create.
    #[cfg(test)]
    pub fn created(&self) -> usize {
        self.tokenizer.sink.builder.sink.created.get()
    }
    pub fn is_finished(&self) -> bool {
        self.finished
    }
    /// Parse until the next script or the end of input.
    pub fn step(&mut self) -> Step {
        if self.finished {
            return Step::Finished;
        }
        loop {
            let result = self.tokenizer.feed(&self.input);
            if self.tokenizer.sink.builder.sink.overflowed() {
                self.finished = true;
                return Step::Overflow;
            }
            match result {
                html5ever::TokenizerResult::Script(node) => return Step::Script(node),
                html5ever::TokenizerResult::Done => break,
                _ => continue,
            }
        }
        self.tokenizer.end();
        self.finished = true;
        Step::Finished
    }
    /// `document.write` while parsing: the text goes at the insertion point,
    /// right after the script that wrote it.
    pub fn write(&mut self, text: &str) {
        if !self.finished && !text.is_empty() {
            self.input.push_front(StrTendril::from_slice(text));
        }
    }
}

/// Use Blitz's fragment sink while retaining its otherwise-discarded HTML errors.
/// Tree construction and fragment-root transfer match blitz-html exactly.
pub(crate) fn parse_fragment(
    mutation: &mut blitz_dom::DocumentMutator<'_>,
    element: NodeId,
    html: &str,
) {
    use html5ever::{ParseOpts, tendril::TendrilSink};
    let url = mutation.doc.url().to_string();
    let sink = blitz_html::DocumentHtmlParser::new(mutation);
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
    for message in parser.tokenizer.sink.sink.errors.borrow().iter() {
        let mut event = crate::diagnostics::Diagnostic::error(
            crate::diagnostics::Category::Html,
            message.to_string(),
            Some(url.clone()),
        );
        event.severity = "warning".into();
        event.emit();
    }
    drop(parser);
    let document = mutation.doc.root_node().id;
    if let Some(fragment) = mutation.last_child_id(document) {
        let children = mutation.child_ids(fragment);
        mutation.append_children(element, &children);
        mutation.remove_and_drop_node(fragment);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom_model::DocumentView;
    use blitz_dom::DocumentConfig;
    use blitz_html::HtmlDocument;

    fn wega_parse(html: &str, limit: usize) -> (Rc<RefCell<BaseDocument>>, usize) {
        let document = Rc::new(RefCell::new(BaseDocument::new(DocumentConfig::default())));
        let mut parser = Parser::new(document.clone(), html, limit);
        let mut scripts = 0;
        while let Step::Script(_) = parser.step() {
            scripts += 1;
        }
        (document, scripts)
    }
    fn serialized(doc: &BaseDocument) -> String {
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
            let blitz = HtmlDocument::from_html(html, DocumentConfig::default()).into_inner();
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

    // ---- node budget -------------------------------------------------------------------
    // Policy: crossing `Limits::dom_nodes` aborts the parse (`Step::Overflow`); the tokenizer
    // stops after the token that crossed it, no script runs afterwards, and the document keeps
    // the valid partial tree built so far. The engine then replaces it with the limit error page.

    /// Everything a budget test needs to know about one parse.
    struct Outcome {
        document: Rc<RefCell<BaseDocument>>,
        parser: Parser,
        scripts: usize,
        overflowed: bool,
    }
    fn budgeted(html: &str, limit: usize) -> Outcome {
        let document = Rc::new(RefCell::new(BaseDocument::new(DocumentConfig::default())));
        let mut parser = Parser::new(document.clone(), html, limit);
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
    fn assert_tree_is_consistent(document: &BaseDocument) -> usize {
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
    fn assert_document_is_usable(document: &Rc<RefCell<BaseDocument>>) {
        assert_tree_is_consistent(&document.borrow());
        // `root_element()` needs an <html> element, which a comment-only prefix never made.
        if document.borrow().try_root_element().is_some() {
            let _ = serialized(&document.borrow());
        }
        let _ = document.borrow().query_selector_all("p");
        let mut doc = document.borrow_mut();
        let root = DocumentView::root(&*doc);
        let mut mutator = crate::dom_model::mutate(&mut *doc);
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
        let html = "<script>1</script><script>2</script><p>a<p>b<p>c<p>d<script>3</script><script>4</script>";
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
        let document = Rc::new(RefCell::new(BaseDocument::new(DocumentConfig::default())));
        let mut parser = Parser::new(document.clone(), "<p>a</p><script>w</script>tail", 40);
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
}
