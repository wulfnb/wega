//! Authorized DOM and inline-style edits. Only the closed operations below exist: no HTML
//! replacement, no subtree insertion, no free-form dispatch.
use super::dom::InspectorNodeId;
use crate::DocumentContext;
use serde::{Deserialize, Serialize};

pub const MAX_ATTRIBUTE_NAME: usize = 128;
pub const MAX_ATTRIBUTE_VALUE: usize = 4096;
pub const MAX_EDIT_TEXT: usize = 4096;
pub const MAX_CSS_PROPERTY: usize = 128;
pub const MAX_CSS_VALUE: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationOp {
    /// Add the attribute or change its value.
    SetAttribute {
        name: String,
        value: String,
    },
    RemoveAttribute {
        name: String,
    },
    /// Replace a text or comment node's data.
    SetText {
        text: String,
    },
    /// Remove the node (not the document, `html`, `head` or `body`).
    RemoveNode,
    /// `style.setProperty`; an empty value removes the declaration.
    SetInlineProperty {
        property: String,
        value: String,
        important: bool,
    },
}

/// The HTML attribute-name grammar the editor accepts: non-empty, ASCII, no whitespace, quotes,
/// `/`, `>`, `=` or controls. A name the parser could not have produced is refused.
pub fn valid_attribute_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_ATTRIBUTE_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_graphic() && !matches!(b, b'"' | b'\'' | b'>' | b'/' | b'=' | b'<'))
}
/// Setting these runs code (an event handler, an embedded document or a script), so they need
/// the evaluation grant as well.
pub fn is_code_attribute(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("on") || lower == "srcdoc"
}
pub fn valid_css_property(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_CSS_PROPERTY
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl MutationOp {
    /// Shape and size validation that needs no state. `Err` carries the reason.
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::SetAttribute { name, value } => {
                if !valid_attribute_name(name) {
                    Err("Invalid attribute name")
                } else if value.len() > MAX_ATTRIBUTE_VALUE || value.contains('\0') {
                    Err("Attribute value is too large or invalid")
                } else {
                    Ok(())
                }
            }
            Self::RemoveAttribute { name } => {
                if valid_attribute_name(name) {
                    Ok(())
                } else {
                    Err("Invalid attribute name")
                }
            }
            Self::SetText { text } => {
                if text.len() > MAX_EDIT_TEXT || text.contains('\0') {
                    Err("Text is too large or invalid")
                } else {
                    Ok(())
                }
            }
            Self::RemoveNode => Ok(()),
            Self::SetInlineProperty {
                property, value, ..
            } => {
                if !valid_css_property(property) {
                    Err("Invalid CSS property name")
                } else if value.len() > MAX_CSS_VALUE || value.contains('\0') {
                    Err("CSS value is too large or invalid")
                } else {
                    Ok(())
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationOutcome {
    pub document: DocumentContext,
    pub node: InspectorNodeId,
    /// The mutation epoch after the edit; the browser adopts it as its scope.
    pub epoch: u64,
    /// False when the edit was valid but changed nothing (a CSS declaration the engine ignored
    /// or already in that state).
    pub changed: bool,
}
impl MutationOutcome {
    pub fn is_bounded(&self) -> bool {
        self.node.document == self.document
    }
}
