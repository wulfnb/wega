//! Inline formatting context, and intrinsic (min-/max-content) sizing measured from inline
//! content.
#![forbid(unsafe_code)]
use wega_css_values::*;
use wega_layout_core::*;
use wega_style::*;

mod inline;
mod intrinsic;

pub use inline::*;
pub use intrinsic::*;
