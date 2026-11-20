//! Wega Native Rendering Engine: progressive replacement for Blitz, Stylo and Taffy.
pub mod backend;
pub use wega_native::{css, display_list, graphics, layout, text};

pub use backend::{NativeDocument, RenderingBackend};
pub use css::{
    Color, ComputedStyle, Display, FontFaceRule, ImportRule, Length, MediaContext, MediaQuery,
    ObjectFit, Position, StyleSheet, VerticalAlign, default_user_agent_stylesheet,
    resolve_computed_style,
};
pub use display_list::{DisplayItem, DisplayList};

pub mod accessibility;
