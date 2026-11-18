//! The engine's consumer of HTML parser warnings: parse diagnostics become `Html`
//! warnings with the document URL and line, emitted as they occur.
use wega_adapter_blitz::BlitzDocument;
use wega_html::{HtmlDiagnostic, ParseReport};

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
pub(crate) type Parser = wega_html::Parser<BlitzDocument, HtmlReport>;

/// Construction can discover resources before the XML budget is exceeded. The
/// view has not installed this controller yet, so cancel it here on rejection.
pub(crate) fn parse_xml_document<D: wega_dom::DocumentMutation>(
    document: std::rc::Rc<std::cell::RefCell<D>>,
    source: &str,
    limit: usize,
    abort: blitz_traits::net::AbortController,
) -> Result<blitz_traits::net::AbortController, String> {
    match wega_html::parse_xml(document, source, limit, HtmlReport) {
        Ok(_) => Ok(abort),
        Err(_) => {
            abort.abort();
            Err("XML document exceeded the DOM node limit".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blitz_traits::net::{AbortController, AbortSignal, NetHandler, NetProvider, Request};
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{Arc, Mutex},
    };
    use wega_adapter_blitz::DocumentSetup;

    #[derive(Default)]
    struct ResourceProbe(Mutex<Vec<AbortSignal>>);
    impl NetProvider for ResourceProbe {
        fn fetch(&self, _: usize, request: Request, _: Box<dyn NetHandler>) {
            self.0
                .lock()
                .unwrap()
                .push(request.signal.expect("resource cancellation signal"));
        }
    }
    #[test]
    fn xml_rejection_aborts_resources_discovered_before_the_limit() {
        for overflow in [false, true] {
            let probe = Arc::new(ResourceProbe::default());
            let abort = AbortController::default();
            let signal = abort.signal.clone();
            let document = Rc::new(RefCell::new(BlitzDocument::empty(DocumentSetup {
                base_url: Some("https://wega.test/document.xhtml".into()),
                abort_signal: Some(signal.clone()),
                net_provider: Some(probe.clone()),
                ..Default::default()
            })));
            let xml = format!(
                "<html xmlns='http://www.w3.org/1999/xhtml'><head><link rel='stylesheet' href='/slow.css'/></head><body>{}</body></html>",
                "<p/>".repeat(if overflow { 100 } else { 1 })
            );
            let result = parse_xml_document(document, &xml, 16, abort);
            assert_eq!(result.is_err(), overflow);
            assert_eq!(probe.0.lock().unwrap().len(), 1);
            assert_eq!(signal.aborted(), overflow);
            assert_eq!(probe.0.lock().unwrap()[0].aborted(), overflow);
            if let Ok(abort) = result {
                abort.abort();
                assert!(signal.aborted());
            }
        }
    }
}
