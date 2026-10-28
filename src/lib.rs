//! Wega core and application services. The default-free build has no Servo or GPU dependency.
#![forbid(unsafe_code)]
pub mod diagnostic_event;
pub mod diagnostics;
pub mod downloads;
pub mod error_auth;
pub mod error_report_model;
pub mod error_reporting;
pub mod model;
pub mod options;
pub mod profile;
pub mod session;
pub mod storage;
#[cfg(feature = "desktop")]
pub mod theme {
    pub use crate::model::{Palette, install, palette};
}
pub mod application;
pub mod base64_text;
pub mod container_launch;
pub mod containers;
pub mod deadline;
pub mod engine_choice;
pub mod engine_protocol;
pub mod lifecycle;
pub mod navigation;
pub mod permissions;
