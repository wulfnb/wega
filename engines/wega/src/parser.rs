//! Incremental HTML parsing into Blitz's DOM. Unlike `blitz_html`'s one-shot
//! parser, this one pauses after every `</script>` so that the script runs
//! against the partially parsed document, and accepts `document.write` input at
//! the insertion point.
//!
//! The tree sink is adapted from blitz-html 0.3.0-beta.4 `html_sink.rs`
//! (MIT OR Apache-2.0, DioxusLabs); modified by Wega contributors, 2026, to
//! borrow the shared document per operation instead of holding one mutator for
//! the whole parse.
use blitz_dom::{BaseDocument, NodeId, node::Attribute};
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

pub(crate) struct Sink {
    document: Rc<RefCell<BaseDocument>>,
    /// Nodes created so far, and the node budget. Past the budget the sink
    /// stops building: new nodes become one detached placeholder.
    created: Cell<usize>,
    limit: usize,
    line: Cell<u64>,
    overflow: Cell<Option<NodeId>>,
}
fn attribute(attr: html5ever::Attribute) -> Attribute {
    Attribute {
        name: attr.name,
        value: attr.value.to_string(),
    }
}
impl Sink {
    fn mutate<R>(&self, f: impl FnOnce(&mut blitz_dom::DocumentMutator<'_>) -> R) -> R {
        let mut document = self.document.borrow_mut();
        let mut mutator = document.mutate();
        f(&mut mutator)
    }
    fn overflowed(&self) -> bool {
        self.overflow.get().is_some()
    }
    /// Count a new node; past the budget, return the placeholder instead.
    fn admit(&self) -> Option<NodeId> {
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
                vec![],
            )
        });
        self.overflow.set(Some(placeholder));
        Some(placeholder)
    }
}
impl TreeSink for Sink {
    type Output = ();
    type Handle = NodeId;
    type ElemName<'a>
        = Ref<'a, QualName>
    where
        Self: 'a;
    fn finish(self) {}
    fn parse_error(&self, msg: Cow<'static, str>) {
        let mut event = crate::diagnostics::Diagnostic::error(
            crate::diagnostics::Category::Html,
            msg.into_owned(),
            Some(self.document.borrow().url().to_string()),
        );
        event.severity = "warning".into();
        event.line = Some(self.line.get());
        event.emit();
    }
    fn set_current_line(&self, line_number: u64) {
        self.line.set(line_number);
    }
    fn get_document(&self) -> NodeId {
        self.document.borrow().root_node().id
    }
    fn elem_name<'a>(&'a self, target: &'a NodeId) -> Ref<'a, QualName> {
        Ref::map(self.document.borrow(), |doc| {
            &doc.get_node(*target)
                .and_then(|n| n.element_data())
                .expect("TreeSink::elem_name called on a node which is not an element")
                .name
        })
    }
    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<html5ever::Attribute>,
        _flags: ElementFlags,
    ) -> NodeId {
        if let Some(placeholder) = self.admit() {
            return placeholder;
        }
        let attrs = attrs.into_iter().map(attribute).collect();
        self.mutate(|m| m.create_element(name, attrs))
    }
    fn create_comment(&self, text: StrTendril) -> NodeId {
        if let Some(placeholder) = self.admit() {
            return placeholder;
        }
        self.mutate(|m| m.create_comment_node(&text))
    }
    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> NodeId {
        self.mutate(|m| m.create_comment_node(""))
    }
    fn append(&self, parent: &NodeId, child: NodeOrText<NodeId>) {
        if self.overflowed() {
            return;
        }
        self.mutate(|m| match child {
            NodeOrText::AppendNode(id) => m.append_children(*parent, &[id]),
            NodeOrText::AppendText(text) => {
                let appended = m
                    .last_child_id(*parent)
                    .is_some_and(|id| m.append_text_to_node(id, &text).is_ok());
                if !appended {
                    let id = m.create_text_node(&text);
                    m.append_children(*parent, &[id]);
                }
            }
        })
    }
    fn append_before_sibling(&self, sibling: &NodeId, new_node: NodeOrText<NodeId>) {
        if self.overflowed() {
            return;
        }
        self.mutate(|m| match new_node {
            NodeOrText::AppendNode(id) => m.insert_nodes_before(*sibling, &[id]),
            NodeOrText::AppendText(text) => {
                let appended = m
                    .previous_sibling_id(*sibling)
                    .is_some_and(|id| m.append_text_to_node(id, &text).is_ok());
                if !appended {
                    let id = m.create_text_node(&text);
                    m.insert_nodes_before(*sibling, &[id]);
                }
            }
        })
    }
    fn append_based_on_parent_node(
        &self,
        element: &NodeId,
        prev_element: &NodeId,
        child: NodeOrText<NodeId>,
    ) {
        let has_parent = self.mutate(|m| m.node_has_parent(*element));
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
    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        self.mutate(|m| m.template_contents(*target))
    }
    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }
    fn set_quirks_mode(&self, _mode: QuirksMode) {}
    fn add_attrs_if_missing(&self, target: &NodeId, attrs: Vec<html5ever::Attribute>) {
        if self.overflowed() {
            return;
        }
        let attrs = attrs.into_iter().map(attribute).collect();
        self.mutate(|m| m.add_attrs_if_missing(*target, attrs));
    }
    fn remove_from_parent(&self, target: &NodeId) {
        if self.overflowed() {
            return;
        }
        self.mutate(|m| m.remove_node(*target));
    }
    fn reparent_children(&self, old_parent: &NodeId, new_parent: &NodeId) {
        if self.overflowed() {
            return;
        }
        self.mutate(|m| m.reparent_children(*old_parent, *new_parent));
    }
    /// A `<style>` element's text arrives after the element is inserted, and each
    /// operation uses its own mutator, so reprocess the sheet once it is closed.
    fn pop(&self, node: &NodeId) {
        if self.overflowed() {
            return;
        }
        let is_style = self
            .document
            .borrow()
            .get_node(*node)
            .and_then(|n| n.element_data())
            .is_some_and(|e| &*e.name.local == "style");
        if is_style {
            self.document.borrow_mut().process_style_element(*node);
        }
    }
}

/// What the parser did on one step.
pub(crate) enum Step {
    /// A parser-inserted `<script>` element was closed; run or fetch it next.
    Script(NodeId),
    /// The document has more nodes than the budget allows.
    Overflow,
    /// All input was consumed and the document is complete.
    Finished,
}

pub(crate) struct Parser {
    tokenizer: Tokenizer<TreeBuilder<NodeId, Sink>>,
    input: BufferQueue,
    finished: bool,
}
impl Parser {
    pub fn new(document: Rc<RefCell<BaseDocument>>, html: &str, node_limit: usize) -> Self {
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
