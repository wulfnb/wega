//! Style access: the inline style of an element, resolved (computed) values and the
//! CSS support and media queries script can ask.
//!
//! The document engine owns all style state (declaration blocks, stylesheets, the cascade and
//! computed values). These traits only read it or forward one CSSOM write; they keep no cache
//! and no copy, so there is no second source of truth. Reads append to a caller-provided
//! `String` so a value that can be serialized straight from borrowed style data needs no
//! intermediate allocation.
//!
//! Not covered, because nothing consumes it: stylesheet/rule objects (CSSOM is not exposed to
//! script), pseudo-element styles, and invalidation (the document engine invalidates inside
//! each mutation; stylesheets enter and leave through `<style>`/`<link>` mutations).
use crate::{DocumentMutation, DocumentView, MutationError, NodeKey};

/// Why a style read was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleError {
    /// The key does not (or no longer) refer to a node of this document.
    Stale(NodeKey),
    /// Only elements have styles.
    NotElement(NodeKey),
}
impl std::fmt::Display for StyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stale(key) => write!(f, "node {key} no longer exists"),
            Self::NotElement(key) => write!(f, "node {key} is not an element"),
        }
    }
}
impl std::error::Error for StyleError {}

/// Read access to styles.
pub trait StyleAccess: DocumentView {
    /// Appends the element's inline declaration block in its canonical serialization
    /// (`element.style.cssText`); nothing for an element without inline declarations.
    fn inline_style(&self, key: NodeKey, out: &mut String) -> Result<(), StyleError>;
    /// Appends the value of one inline property, longhand or shorthand
    /// (`element.style.getPropertyValue`); nothing when it is not set or not a property.
    fn inline_property(
        &self,
        key: NodeKey,
        property: &str,
        out: &mut String,
    ) -> Result<(), StyleError>;
    /// Appends the resolved value of `property` (`getComputedStyle`): the computed value, or
    /// the used value for layout-dependent properties. Nothing for unknown properties and for
    /// elements outside the document. **The caller flushes style and layout first**; this
    /// never triggers a restyle or relayout.
    fn resolved_value(
        &self,
        key: NodeKey,
        property: &str,
        out: &mut String,
    ) -> Result<(), StyleError>;
    /// `CSS.supports(conditionText)`; false for an unparseable condition.
    fn supports_condition(&self, condition: &str) -> bool;
    /// `CSS.supports(property, value)`.
    fn supports_declaration(&self, property: &str, value: &str) -> bool;
}

/// Style writes, and queries that need the document mutably.
pub trait StyleControl: StyleAccess + DocumentMutation {
    /// `element.style.setProperty(property, value, priority)`; an empty `value` removes the
    /// property (`removeProperty`). The element's `style` attribute becomes the canonical
    /// serialization of the new block and the element is invalidated for restyle.
    ///
    /// `Ok(false)` means nothing changed: the property is unknown, the value does not parse
    /// (CSSOM ignores such a write) or the block is already in that state. All-or-nothing.
    fn set_inline_property(
        &mut self,
        key: NodeKey,
        property: &str,
        value: &str,
        important: bool,
    ) -> Result<bool, MutationError>;
    /// Whether the media query list matches the document's device (`matchMedia`). Mutable
    /// only because pending viewport changes are applied to the device first.
    fn media_matches(&mut self, query: &str) -> bool;
}
