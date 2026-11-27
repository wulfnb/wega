//! Block formatting context: block flow, floats, margin collapsing, and positioned
//! children. Also sizes the boxes of other containers (`init_container`).
#![forbid(unsafe_code)]
use wega_css_values::*;
use wega_layout_core::*;
use wega_layout_inline::*;

mod block;
mod container;
mod control_text;

pub use block::*;
pub use container::*;
use control_text::*;
