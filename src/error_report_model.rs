use crate::diagnostic_event::Diagnostic;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub id: String,
    pub occurred_at: String,
    pub application: String,
    pub version: String,
    pub platform: String,
    pub source: String,
    pub diagnostic: Diagnostic,
}
