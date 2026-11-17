//! Wega Native Rendering Engine: progressive replacement for Blitz, Stylo and Taffy.
pub mod backend;
pub mod css;
pub mod display_list;
pub mod layout;
mod syntax;
pub mod text;

pub use backend::{NativeDocument, RenderingBackend};
pub use css::{
    Color, ComputedStyle, Display, Length, StyleSheet, default_user_agent_stylesheet,
    resolve_computed_style,
};
pub use display_list::{DisplayItem, DisplayList};
pub use layout::{BoxType, LayoutBox, LayoutContext, perform_layout};
