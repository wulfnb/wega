//! Small Actix collector with Flow's routes/controllers/validators separation.
pub mod controllers;
#[path = "../../src/diagnostic_event.rs"]
pub mod diagnostic_event;
#[path = "../../src/error_auth.rs"]
pub mod error_auth;
#[path = "../../src/error_report_model.rs"]
pub mod error_report_model;
pub mod handlers;
pub mod routes;
pub mod store;
pub mod validators;
use std::sync::Mutex;
pub struct State {
    pub store: Mutex<store::Store>,
}
pub const MAX_BODY: usize = 256 * 1024;
#[cfg(test)]
mod tests;
