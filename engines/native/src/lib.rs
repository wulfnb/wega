//! Independent Wega algorithms: document contract -> CSS -> layout -> scene.
//! Fetching is supplied by the embedding browser. Image decoding consumes only
//! authorized bytes, with shared admission/retention budgets and no file resolver.
#![forbid(unsafe_code)]
pub mod css;
pub mod display_list;
pub mod graphics;
pub mod images;
pub mod layout;
mod syntax;
pub mod text;
