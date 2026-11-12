//! Incremental HTML parsing into a Wega document (`wega-dom`). Unlike `blitz_html`'s one-shot
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
pub struct HtmlDiagnostic<'a> {
    pub message: Cow<'a, str>,
    /// The source line being parsed, when known.
    pub line: Option<u64>,
    /// The document being parsed.
    pub url: &'a url::Url,
}
/// Receives parse warnings as they happen (so their order relative to other events is
/// preserved). Static dispatch: the consumer is a type parameter of the parser.
pub trait ParseReport {
    fn warning(&self, diagnostic: HtmlDiagnostic<'_>);
}
impl<T: ParseReport + ?Sized> ParseReport for &T {
    fn warning(&self, diagnostic: HtmlDiagnostic<'_>) {
        (**self).warning(diagnostic);
    }
}

/// The tree sink, generic over the document it builds into (`wega_dom::DocumentMutation`).
/// Each tree-construction operation uses its own mutation scope, as before.
struct Sink<D: DocumentMutation, W: ParseReport> {
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
pub enum Step {
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

pub struct Parser<D: DocumentMutation, W: ParseReport> {
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
pub struct FragmentOverflow;

/// Parses `html` as the children of `context` (innerHTML, `insertAdjacentHTML`, contextual
/// fragments) straight into the document through the same tree sink as full documents, then
/// moves the result under `context`. The parse stops at `limit` nodes. Scripts do not run and
/// are not paused for; the caller marks them already started.
pub fn parse_fragment<D: DocumentMutation, W: ParseReport>(
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
