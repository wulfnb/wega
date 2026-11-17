//! Independent Wega algorithms: document contract -> CSS -> layout -> scene.
//! Resource fetching/decoding is supplied by the embedding browser, never here.
#![forbid(unsafe_code)]
pub mod css;
pub mod display_list;
pub mod graphics;
pub mod layout;
mod syntax;
pub mod text;
