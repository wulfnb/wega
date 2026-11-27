//! Table layout. The container's own box is sized by the block formatting context
//! (`init_container`); this crate places its contents.
#![forbid(unsafe_code)]
use wega_css_values::*;
use wega_layout_block::*;
use wega_layout_core::*;
use wega_layout_inline::*;

mod table;

pub use table::*;
