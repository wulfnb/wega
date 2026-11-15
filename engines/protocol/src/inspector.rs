//! Wega-owned inspector protocol: typed messages between the trusted browser UI and one
//! content process. Names no engine, DOM, script or embedder type, so any backend can
//! implement it and the UI never sees engine internals.
//!
//! The command set is closed: there is no evaluation, no free-form method name and no
//! generic payload that a backend could interpret. Only the browser process writes
//! [`crate::Command`]s; a page can neither construct nor submit one.
use crate::{
    DocumentContext,
    identity::{FrameId, ProcessSessionId, SessionIdentity, TabId},
};
use serde::{Deserialize, Serialize};

pub mod session;

/// Message-set version, negotiated inside `Capability::Inspector`, independent of the
/// engine protocol `VERSION`.
pub const INSPECTOR_VERSION: u32 = 1;
/// Browser-side cap on in-flight inspector requests, a sub-cap of `requests::MAX_PENDING`
/// so a panel can never starve evaluation or harness requests.
pub const MAX_INSPECTOR_PENDING: usize = 4;
/// Per-session browser-side event ring; overflow drops the oldest and is counted.
pub const EVENT_RING: usize = 64;
/// Completed-request ring read by the UI thread.
pub const RESULT_RING: usize = 8;
pub const INSPECTOR_TIMEOUT_MS: u64 = 2_000;
/// Upper bound for any string field in a snapshot.
pub const MAX_FIELD: usize = 2048;
pub const MAX_ERROR_MESSAGE: usize = 512;
pub const MAX_SESSIONS_PER_TARGET: usize = 1;

pub fn supports_inspector_version(version: u32) -> bool {
    (1..=INSPECTOR_VERSION).contains(&version)
}

/// A session is scoped to a tab's frame inside one content process. A restarted or
/// replacement process has a new `ProcessSessionId`, so a `TargetId` never outlives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TargetId {
    pub tab: TabId,
    pub frame: FrameId,
    pub process: ProcessSessionId,
}

impl TargetId {
    pub fn of(identity: &SessionIdentity) -> Self {
        Self {
            tab: identity.tab,
            frame: identity.frame,
            process: identity.process,
        }
    }
}

/// Issued only by the browser process; strictly increasing, never reused, never zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InspectorSessionId(pub u64);

/// The complete command set. Every command is parameterless: the session, target and
/// document stamp travel in the envelope, so there is nothing for a backend to interpret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InspectorCommand {
    Attach,
    Detach,
    GetTargetInfo,
    SubscribeStatus,
    UnsubscribeStatus,
    GetStatusSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectorRequest {
    pub version: u32,
    pub session: InspectorSessionId,
    pub target: TargetId,
    pub command: InspectorCommand,
}

/// A request as it crosses the pipe. It is decoded by the receiver so a malformed
/// payload is answered with `InvalidPayload` instead of failing the whole packet (which
/// would tear the content process down).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InspectorPayload(pub serde_json::Value);

impl InspectorPayload {
    pub fn new(request: &InspectorRequest) -> Self {
        // A struct of integers, byte arrays and unit enums always serialises.
        Self(serde_json::to_value(request).unwrap_or(serde_json::Value::Null))
    }
    pub fn decode(&self) -> Result<InspectorRequest, InspectorError> {
        InspectorRequest::deserialize(&self.0).map_err(|error| {
            InspectorError::new(
                InspectorErrorCode::InvalidPayload,
                format!("Malformed inspector request: {error}"),
            )
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    /// CSS pixels.
    pub width: f64,
    pub height: f64,
    pub device_pixel_ratio: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetInfo {
    pub target: TargetId,
    pub document: DocumentContext,
    pub protocol_version: u32,
    pub inspector_version: u32,
    pub engine: String,
}

/// Facts the view already maintains; nothing here forces layout, style or script.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub target: TargetId,
    pub document: DocumentContext,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub viewport: Option<Viewport>,
    pub nodes: u64,
    pub js_heap_bytes: u64,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub protocol_version: u32,
    pub inspector_version: u32,
    pub engine: String,
}

impl StatusSnapshot {
    /// Truncate every unbounded field. Called by the producer; the consumer re-checks.
    pub fn bounded(mut self) -> Self {
        truncate(&mut self.url, MAX_FIELD);
        truncate(&mut self.title, MAX_FIELD / 4);
        truncate(&mut self.engine, 128);
        self
    }
    pub fn is_bounded(&self) -> bool {
        self.url.len() <= MAX_FIELD && self.title.len() <= MAX_FIELD / 4 && self.engine.len() <= 128
    }
}

pub(crate) fn truncate(text: &mut String, max: usize) {
    if text.len() > max {
        let mut end = max;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum InspectorResponse {
    Attached,
    Detached,
    TargetInfo(TargetInfo),
    Subscribed,
    Unsubscribed,
    Status(Box<StatusSnapshot>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InspectorEvent {
    /// The document or its global was replaced; every document-bound cache is stale.
    DocumentChanged {
        document: DocumentContext,
    },
    LoadingChanged {
        loading: bool,
    },
    /// Synthesised browser-side when the event ring overflowed: `count` events were lost.
    EventsDropped {
        count: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InspectorErrorCode {
    /// No such session, or the session does not own that target.
    Unauthorized,
    UnknownTarget,
    /// The session existed and was revoked or superseded.
    StaleSession,
    StaleDocument,
    /// The process is gone, closing or restarting, or the inspector is not offered.
    Unavailable,
    UnsupportedVersion,
    Timeout,
    Cancelled,
    InvalidPayload,
    Busy,
    SessionLimit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectorError {
    pub code: InspectorErrorCode,
    pub message: String,
}

impl InspectorError {
    pub fn new(code: InspectorErrorCode, message: impl AsRef<str>) -> Self {
        Self {
            code,
            message: message.as_ref().chars().take(MAX_ERROR_MESSAGE).collect(),
        }
    }
}

impl std::fmt::Display for InspectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl From<&crate::ProtocolError> for InspectorError {
    fn from(error: &crate::ProtocolError) -> Self {
        use crate::ErrorCode as E;
        let code = match error.code {
            E::StaleDocument => InspectorErrorCode::StaleDocument,
            E::Cancelled => InspectorErrorCode::Cancelled,
            E::Timeout => InspectorErrorCode::Timeout,
            E::ResourceLimit => InspectorErrorCode::Busy,
            E::InvalidRequest => InspectorErrorCode::InvalidPayload,
            E::Unsupported => InspectorErrorCode::UnsupportedVersion,
            E::ScriptException => InspectorErrorCode::Unavailable,
        };
        Self::new(code, &error.message)
    }
}

#[cfg(test)]
#[path = "inspector_tests.rs"]
mod tests;
