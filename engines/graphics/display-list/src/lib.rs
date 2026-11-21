//! Wega display commands carry the geometry used by both rasterization and
//! picking. Stacking contexts are collected across non-context ancestors.
//!
//! Reference (concepts, not code): `blitz-paint`.
#![forbid(unsafe_code)]
use anyrender::PaintScene;
use kurbo::{Affine, Shape};
use wega_css_values::{Color, Length, ObjectFit, Overflow, Position};
use wega_dom::{NodeKey, layout::Rect};
use wega_image::SvgGraphic;
use wega_layout::{BoxType, FormControlKind, LayoutBox, Point};

mod boxes;
mod list;
mod stacking;

use boxes::*;
pub use list::*;
use stacking::*;
