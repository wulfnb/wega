//! Style resolution: cascade, user-agent sheet, presentational hints and computed values.
//!
//! Reference (concepts, not code): Stylo `rule_tree` and `custom_properties.rs`;
//! Blitz `user_agent.rs` and `presentational_hints.rs`.
#![forbid(unsafe_code)]
mod apply;
mod cascade;
pub mod computed;
mod computed_style;
mod gradient;
mod hints;
mod typed;
mod user_agent;
mod values;

use apply::*;
pub use cascade::*;
pub use computed_style::*;
use gradient::*;
use hints::*;
use typed::*;
pub use user_agent::*;
use values::*;
