//! CSS selectors: parsing, specificity and matching through `wega_dom::DocumentView`.
//!
//! Reference (concepts, not code): the `selectors` crate.
#![forbid(unsafe_code)]
mod matching;
mod parser;
mod types;

pub use parser::*;
pub use types::*;
