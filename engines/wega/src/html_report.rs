//! The engine's consumer of HTML parser warnings: parse diagnostics become `Html`
//! warnings with the document URL and line, emitted as they occur.
use crate::dom_model::BlitzDocument;
use crate::parser::{HtmlDiagnostic, ParseReport};

pub(crate) struct HtmlReport;
impl ParseReport for HtmlReport {
    fn warning(&self, diagnostic: HtmlDiagnostic<'_>) {
        let mut event = crate::diagnostics::Diagnostic::error(
            crate::diagnostics::Category::Html,
            diagnostic.message.into_owned(),
            Some(diagnostic.url.to_string()),
        );
        event.severity = "warning".into();
        event.line = diagnostic.line;
        event.emit();
    }
}

/// The incremental parser as the engine uses it.
pub(crate) type Parser = crate::parser::Parser<BlitzDocument, HtmlReport>;
