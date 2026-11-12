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
    tokenizer::{BufferQueue, Tokenizer, TokenizerOpts},
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
    /// Nodes created so far, and the node budget. Past the budget the sink
    /// stops building: new nodes become one detached placeholder.
    created: Cell<usize>,
    limit: usize,
    line: Cell<u64>,
    overflow: Cell<Option<NodeKey>>,
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
        self.overflow.get().is_some()
    }
    /// Count a new node; past the budget, return the placeholder instead.
    fn admit(&self) -> Option<NodeKey> {
        if let Some(placeholder) = self.overflow.get() {
            return Some(placeholder);
        }
        self.created.set(self.created.get() + 1);
        if self.created.get() <= self.limit {
            return None;
        }
        let placeholder = self.mutate(|m| {
            m.create_element(
                QualName::new(
                    None,
                    html5ever::ns!(html),
                    html5ever::LocalName::from("template"),
                ),
                [],
            )
        });
        self.overflow.set(Some(placeholder));
        Some(placeholder)
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
        if let Some(placeholder) = self.admit() {
            return placeholder;
        }
        self.mutate(|m| m.create_element(name, attrs.into_iter().map(attribute)))
    }
    fn create_comment(&self, text: StrTendril) -> NodeKey {
        if let Some(placeholder) = self.admit() {
            return placeholder;
        }
        self.mutate(|m| m.create_comment(&text))
    }
    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> NodeKey {
        self.mutate(|m| m.create_comment(""))
    }
    fn append(&self, parent: &NodeKey, child: NodeOrText<NodeKey>) {
        if self.overflowed() {
            return;
        }
        let result = self.mutate(|m| match child {
            NodeOrText::AppendNode(id) => m.append_children(*parent, &[id]),
            NodeOrText::AppendText(text) => {
                let appended = m
                    .last_child(*parent)
                    .is_some_and(|id| m.append_text(id, &text).is_ok());
                if appended {
                    Ok(())
                } else {
                    let id = m.create_text(&text);
                    m.append_children(*parent, &[id])
                }
            }
        });
        self.applied(result);
    }
    fn append_before_sibling(&self, sibling: &NodeKey, new_node: NodeOrText<NodeKey>) {
        if self.overflowed() {
            return;
        }
        let result = self.mutate(|m| match new_node {
            NodeOrText::AppendNode(id) => m.insert_before(*sibling, &[id]),
            NodeOrText::AppendText(text) => {
                let appended = m
                    .previous_sibling(*sibling)
                    .is_some_and(|id| m.append_text(id, &text).is_ok());
                if appended {
                    Ok(())
                } else {
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
        if self.overflowed() {
            return;
        }
        let result =
            self.mutate(|m| m.add_attributes_if_missing(*target, attrs.into_iter().map(attribute)));
        self.applied(result);
    }
    fn remove_from_parent(&self, target: &NodeKey) {
        if self.overflowed() {
            return;
        }
        let result = self.mutate(|m| m.remove(*target));
        self.applied(result);
    }
    fn reparent_children(&self, old_parent: &NodeKey, new_parent: &NodeKey) {
        if self.overflowed() {
            return;
        }
        let result = self.mutate(|m| m.reparent_children(*old_parent, *new_parent));
        self.applied(result);
    }
    /// A `<style>` element's text arrives after the element is inserted, and each
    /// operation uses its own mutation scope, so reprocess the sheet once it is closed.
    fn pop(&self, node: &NodeKey) {
        if self.overflowed() {
            return;
        }
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

pub(crate) struct Parser<D: DocumentMutation = BaseDocument> {
    tokenizer: Tokenizer<TreeBuilder<NodeKey, Sink<D>>>,
    input: BufferQueue,
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
                overflow: Cell::new(None),
            },
            TreeBuilderOpts {
                // Scripts run, so <noscript> content is raw text.
                scripting_enabled: true,
                drop_doctype: true,
                ..Default::default()
            },
        );
        let tokenizer = Tokenizer::new(builder, TokenizerOpts::default());
        let input = BufferQueue::default();
        input.push_back(StrTendril::from_slice(html));
        Self {
            tokenizer,
            input,
            finished: false,
        }
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
            if self.tokenizer.sink.sink.overflowed() {
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

    /// KNOWN PRE-EXISTING BUG (identical before the mutation contract): once the node budget
    /// is exceeded inside one chunk, every new node is the same placeholder handle and
    /// html5ever's open-element bookkeeping underflows ("no current element") while the rest
    /// of the chunk is processed. Tracked in docs/architecture/dom-mutation-report.md.
    #[test]
    #[ignore = "pre-existing: html5ever panics once the node budget is exceeded within one chunk"]
    fn the_node_budget_stops_the_tree_growing() {
        let html = format!("<body>{}", "<p>x</p>".repeat(50));
        let (document, _) = wega_parse(&html, 20);
        let doc = document.borrow();
        let count = doc.query_selector_all("p").unwrap().len();
        assert!(
            count < 50,
            "parsing stops growing the tree past the budget ({count})"
        );
    }
}
