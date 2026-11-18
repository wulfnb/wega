//! XML construction through the same document mutation contract as HTML.
use super::{ParseReport, Sink};
use html5ever::{buffer_queue::BufferQueue, tendril::StrTendril};
use std::{cell::RefCell, rc::Rc};
use wega_dom::{DocumentMutation, NodeKey};
use xml5ever::{
    tokenizer::{ProcessResult, Token, TokenSink, XmlTokenizer},
    tree_builder::XmlTreeBuilder,
};

/// The XML document exceeded its creation budget; discard the partial document.
#[derive(Debug, PartialEq, Eq)]
pub struct DocumentOverflow;

struct BudgetedXml<D: DocumentMutation, W: ParseReport> {
    builder: XmlTreeBuilder<NodeKey, Sink<D, W>>,
    input: Rc<BufferQueue>,
}
impl<D: DocumentMutation, W: ParseReport> TokenSink for BudgetedXml<D, W> {
    type Handle = NodeKey;
    fn process_token(&self, token: Token) -> ProcessResult<NodeKey> {
        let result = self.builder.process_token(token);
        // As with HTML, stop after the token that crosses the budget, without
        // returning placeholder handles to the tree builder or allocating the
        // rest of an attacker-controlled document.
        if self.builder.sink.overflowed() {
            self.input.replace_with(BufferQueue::default());
        }
        result
    }
    fn end(&self) {
        self.builder.end();
    }
}

/// Parse XML into `document` without executing script. XML document scripts are
/// scheduled by the engine after construction, as before. Returns the count of
/// created nodes, or `Err` when the node budget is crossed; the caller must
/// discard an overflowing document. No external entity or resource loader is
/// installed. Namespace binding, case preservation and error recovery belong to
/// the existing XML tokenizer/tree builder.
pub fn parse_xml<D: DocumentMutation, W: ParseReport>(
    document: Rc<RefCell<D>>,
    xml: &str,
    node_limit: usize,
    report: W,
) -> Result<usize, DocumentOverflow> {
    let input = Rc::new(BufferQueue::default());
    let tokenizer = XmlTokenizer::new(
        BudgetedXml {
            builder: XmlTreeBuilder::new(
                Sink::new(document, node_limit, report),
                Default::default(),
            ),
            input: input.clone(),
        },
        Default::default(),
    );
    input.push_back(StrTendril::from_slice(xml));
    while let html5ever::TokenizerResult::Script(_) = tokenizer.feed(&input) {}
    let sink = &tokenizer.sink.builder.sink;
    if !sink.overflowed() {
        tokenizer.end();
    }
    if sink.overflowed() {
        sink.warn(std::borrow::Cow::Borrowed(
            "XML document exceeded the node budget",
        ));
        Err(DocumentOverflow)
    } else {
        Ok(sink.created.get())
    }
}
