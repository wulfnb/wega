// Modified by Wega contributors, 2026: opt-in CSS parse-error diagnostics.
use style::error_reporting::ParseErrorReporter;
pub(crate) fn reporter() -> Option<&'static dyn ParseErrorReporter> {
    #[cfg(feature = "error-diagnostics")]
    {
        Some(&Reporter)
    }
    #[cfg(not(feature = "error-diagnostics"))]
    {
        None
    }
}
#[cfg(feature = "error-diagnostics")]
struct Reporter;
#[cfg(feature = "error-diagnostics")]
impl ParseErrorReporter for Reporter {
    fn report_error(
        &self,
        url: &style::stylesheets::UrlExtraData,
        location: cssparser::SourceLocation,
        error: style::error_reporting::ContextualParseError,
    ) {
        let message = error.to_string();
        let event = serde_json::json!({"category":"css","severity":"warning","message":message.chars().take(16384).collect::<String>(),"url":url.as_str().chars().take(8192).collect::<String>(),"line":location.line as u64+1,"column":location.column,"truncated":message.chars().count()>16384});
        eprintln!("WEGA_DIAGNOSTIC\t{event}");
    }
}
