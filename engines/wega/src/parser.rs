//! Incremental HTML parsing into Blitz's DOM. Unlike `blitz_html`'s one-shot
//! parser, this one pauses after every `</script>` so that the script runs
//! against the partially parsed document, and accepts `document.write` input at
//! the insertion point.
//!
//! The tree sink is adapted from blitz-html 0.3.0-beta.4 `html_sink.rs`
//! (MIT OR Apache-2.0, DioxusLabs); modified by Wega contributors, 2026, to
//! borrow the shared document per operation instead of holding one mutator for
//! the whole parse.
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
use wega_dom::{DocumentMutation, MutationError, Mutator, NewAttribute, NodeKey, NodeView};

/// One HTML parse warning (tokenizer or tree-construction error, or a parser limit). The
/// parser owns this value and knows nothing about who consumes it.
pub(crate) struct HtmlDiagnostic<'a> {
    pub message: Cow<'a, str>,
    /// The source line being parsed, when known.
    pub line: Option<u64>,
    /// The document being parsed.
    pub url: &'a url::Url,
}
/// Receives parse warnings as they happen (so their order relative to other events is
/// preserved). Static dispatch: the consumer is a type parameter of the parser.
pub(crate) trait ParseReport {
    fn warning(&self, diagnostic: HtmlDiagnostic<'_>);
}
impl<T: ParseReport + ?Sized> ParseReport for &T {
    fn warning(&self, diagnostic: HtmlDiagnostic<'_>) {
        (**self).warning(diagnostic);
    }
}

/// The tree sink, generic over the document it builds into (`wega_dom::DocumentMutation`).
/// Each tree-construction operation uses its own mutation scope, as before.
pub(crate) struct Sink<D: DocumentMutation, W: ParseReport> {
    document: Rc<RefCell<D>>,
    report: W,
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
impl<D: DocumentMutation, W: ParseReport> Sink<D, W> {
    fn new(document: Rc<RefCell<D>>, limit: usize, report: W) -> Self {
        Self {
            document,
            report,
            created: Cell::new(0),
            limit,
            line: Cell::new(1),
            overflow: Cell::new(false),
        }
    }
    fn mutate<T>(&self, f: impl FnOnce(&mut D::Mutator<'_>) -> T) -> T {
        let mut document = self.document.borrow_mut();
        let mut mutator = document.mutate();
        f(&mut mutator)
    }
    /// The parser only passes keys it created, so a rejected operation means a tree
    /// construction bug; report it as an HTML warning and keep parsing.
    fn applied(&self, result: Result<(), MutationError>) {
        if let Err(error) = result {
            self.warn(Cow::Owned(format!(
                "HTML tree construction was rejected: {error}"
            )));
        }
    }
    fn warn(&self, message: Cow<'_, str>) {
        self.report.warning(HtmlDiagnostic {
            message,
            line: Some(self.line.get()),
            url: self.document.borrow().base_url(),
        });
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
impl<D: DocumentMutation, W: ParseReport> TreeSink for Sink<D, W> {
    type Output = ();
    type Handle = NodeKey;
    type ElemName<'a>
        = Ref<'a, QualName>
    where
        Self: 'a;
    fn finish(self) {}
    fn parse_error(&self, msg: Cow<'static, str>) {
        self.warn(msg);
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
struct BudgetedBuilder<D: DocumentMutation, W: ParseReport> {
    builder: TreeBuilder<NodeKey, Sink<D, W>>,
    input: Rc<BufferQueue>,
}
impl<D: DocumentMutation, W: ParseReport> TokenSink for BudgetedBuilder<D, W> {
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

pub(crate) struct Parser<D: DocumentMutation, W: ParseReport> {
    tokenizer: Tokenizer<BudgetedBuilder<D, W>>,
    input: Rc<BufferQueue>,
    finished: bool,
}
impl<D: DocumentMutation, W: ParseReport> Parser<D, W> {
    pub fn new(document: Rc<RefCell<D>>, html: &str, node_limit: usize, report: W) -> Self {
        let builder = TreeBuilder::new(
            Sink::new(document, node_limit, report),
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

/// The fragment did not fit the node budget; nothing of it was kept.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FragmentOverflow;

/// Parses `html` as the children of `context` (innerHTML, `insertAdjacentHTML`, contextual
/// fragments) straight into the document through the same tree sink as full documents, then
/// moves the result under `context`. The parse stops at `limit` nodes. Scripts do not run and
/// are not paused for; the caller marks them already started.
pub(crate) fn parse_fragment<D: DocumentMutation, W: ParseReport>(
    document: &mut D,
    context: NodeKey,
    limit: usize,
    html: &str,
    report: &W,
) -> Result<(), FragmentOverflow> {
    let url = document.base_url().clone();
    let root = document.root();
    let document = Rc::new(RefCell::new(&mut *document));
    let input = Rc::new(BufferQueue::default());
    input.push_back(StrTendril::from_slice(html));
    let builder = TreeBuilder::new_for_fragment(
        Sink::new(document.clone(), limit, report),
        context,
        None,
        TreeBuilderOpts {
            exact_errors: false,
            scripting_enabled: false,
            iframe_srcdoc: false,
            drop_doctype: true,
            quirks_mode: QuirksMode::NoQuirks,
        },
    );
    let state = builder.tokenizer_state_for_context_elem(false);
    let tokenizer = Tokenizer::new(
        BudgetedBuilder {
            builder,
            input: input.clone(),
        },
        TokenizerOpts {
            initial_state: Some(state),
            ..Default::default()
        },
    );
    let overflowed = |tokenizer: &Tokenizer<BudgetedBuilder<&mut D, &W>>| {
        tokenizer.sink.builder.sink.overflowed()
    };
    loop {
        let result = tokenizer.feed(&input);
        if overflowed(&tokenizer) {
            break;
        }
        // A script end tag only pauses the tokenizer; fragments do not run scripts.
        if matches!(result, html5ever::TokenizerResult::Done) {
            break;
        }
    }
    if !overflowed(&tokenizer) {
        tokenizer.end();
    }
    let overflow = overflowed(&tokenizer);
    drop(tokenizer);
    let mut document = document.borrow_mut();
    // The tree builder parsed into a temporary `<html>` root appended to the document node.
    let fragment = document.node(root).and_then(|node| node.children().last());
    let Some(fragment) = fragment else {
        return if overflow {
            Err(FragmentOverflow)
        } else {
            Ok(())
        };
    };
    let children: Vec<NodeKey> = document
        .node(fragment)
        .map(|node| node.children().collect())
        .unwrap_or_default();
    let mut mutator = document.mutate();
    if overflow {
        // Nothing is kept: the temporary root and whatever hangs below it are dropped here.
        let _ = mutator.remove_and_drop(fragment);
        drop(mutator);
        report.warning(HtmlDiagnostic {
            message: Cow::Borrowed("HTML fragment exceeded the node budget"),
            line: None,
            url: &url,
        });
        return Err(FragmentOverflow);
    }
    let _ = mutator.append_children(context, &children);
    let _ = mutator.remove_and_drop(fragment);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use wega_dom::DocumentView;

    struct Silent;
    impl ParseReport for Silent {
        fn warning(&self, _: HtmlDiagnostic<'_>) {}
    }
    use blitz_dom::{BaseDocument, DocumentConfig};
    use blitz_html::HtmlDocument;
    use wega_adapter_blitz::{BlitzDocument, ToNodeId};

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
                let context =
                    add_context(&mut actual, html5ever::Namespace::from(*namespace), name);
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
}
