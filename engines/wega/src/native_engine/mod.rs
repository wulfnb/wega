//! Wega Native Rendering Engine: progressive replacement for Blitz, Stylo and Taffy.
pub mod backend;
/// The CSS engine crates under one path: values, selectors, stylesheets and style.
pub mod css {
    pub use wega_css_parser::*;
    pub use wega_css_selectors::*;
    pub use wega_css_values::*;
    pub use wega_style::*;
}
pub use wega_display_list as display_list;
pub use wega_layout as layout;
pub use wega_layout::text;

pub use backend::{NativeDocument, RenderingBackend};
pub use css::{
    Color, ComputedStyle, Display, FontFaceRule, ImportRule, Length, MediaContext, MediaQuery,
    ObjectFit, Position, StyleSheet, VerticalAlign, default_user_agent_stylesheet,
    resolve_computed_style,
};
pub use display_list::{DisplayItem, DisplayList};

pub mod accessibility;
