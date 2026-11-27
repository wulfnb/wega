//! Layout core: the box tree built from the DOM and `ComputedStyle`, the box model and
//! line/fragment output, form control state, and the layout context.
//!
//! Formatting contexts live in their own crates (`wega-layout-block`, `-inline`, `-flex`,
//! `-grid`, `-table`) and recurse into each other through [`layout_box`], which calls the
//! dispatcher `wega-layout` installs in [`LayoutContext::dispatch`].
#![forbid(unsafe_code)]
use wega_css_parser::{MediaContext, StyleSheet};
use wega_css_values::*;
pub use wega_dom::layout::{BoxModel, ClientBox, ContentBox, Insets, Point, Rect, Size};
use wega_image::SvgGraphic;
use wega_style::*;

mod box_model;
mod box_tree;
mod context;
pub mod controls;
mod geometry;
pub mod resources;

pub use box_model::*;
pub use box_tree::*;
pub use context::*;
pub use geometry::*;
