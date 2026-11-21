//! Wega-owned layout engine.
//!
//! Converts DOM nodes and `ComputedStyle` into a box tree and lays it out: block and
//! inline formatting, floats, flexbox, grid, tables, min/max constraints, positioning
//! and overflow. Formatting contexts recurse through `engine::layout_box`, so they
//! share one crate until a dispatch trait lets them split.
#![forbid(unsafe_code)]
use wega_css_parser::{MediaContext, StyleSheet};
use wega_css_values::*;
pub use wega_dom::layout::{BoxModel, ClientBox, ContentBox, Insets, Point, Rect, Size};
use wega_image::SvgGraphic;
use wega_style::*;

mod block;
mod box_model;
mod box_tree;
mod common;
pub mod controls;
mod engine;
mod flex;
mod grid;
mod inline;
mod intrinsic;
pub mod resources;
mod table;
pub mod text;

use block::*;
pub use box_model::*;
pub use box_tree::*;
use common::*;
pub use engine::*;
use flex::*;
use grid::*;
use inline::*;
use intrinsic::*;
use table::*;
