//! Structured engine diagnostics, shared with the independent page worker.
use serde::{Deserialize, Serialize};
pub const PREFIX: &str = "WEGA_DIAGNOSTIC\t";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Javascript,
    Css,
    Html,
    Browser,
    Network,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub category: Category,
    pub severity: String,
    pub message: String,
    pub url: Option<String>,
    pub line: Option<u64>,
    pub column: Option<u32>,
    #[serde(default)]
    pub truncated: bool,
}
impl Diagnostic {
    pub fn error(category: Category, message: impl Into<String>, url: Option<String>) -> Self {
        Self {
            category,
            severity: "error".into(),
            message: message.into(),
            url,
            line: None,
            column: None,
            truncated: false,
        }
    }
    pub fn bounded(mut self) -> Self {
        if !matches!(self.severity.as_str(), "warning" | "error") {
            self.severity = "error".into();
        }
        if self.message.chars().count() > 16384 {
            self.message = self.message.chars().take(16384).collect();
            self.truncated = true;
        }
        if let Some(url) = &mut self.url {
            *url = url.chars().take(8192).collect();
        }
        self
    }
    pub fn emit(self) {
        if let Ok(json) = serde_json::to_string(&self.bounded()) {
            eprintln!("{PREFIX}{json}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_limits_preserve_unicode_and_mark_truncated_messages() {
        let mut event = Diagnostic::error(Category::Css, "λ".repeat(17000), Some("λ".repeat(9000)));
        event.severity = "unrecognized".into();
        let event = event.bounded();
        assert_eq!(event.message.chars().count(), 16384);
        assert_eq!(event.url.as_ref().unwrap().chars().count(), 8192);
        assert!(event.truncated);
        assert_eq!(event.severity, "error");
        let mut warning = Diagnostic::error(Category::Html, "recoverable", None);
        warning.severity = "warning".into();
        let warning = warning.bounded();
        assert_eq!(warning.severity, "warning");
        assert!(!warning.truncated);
        warning.emit();
    }
}
