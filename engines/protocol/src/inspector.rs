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
pub const INSPECTOR_VERSION: u32 = 2;
/// First inspector message-set version that carries console and evaluation commands.
pub const CONSOLE_VERSION: u32 = 2;
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
/// Engine ring per document and browser/UI stores: entry and byte budgets.
pub const CONSOLE_RING_ENTRIES: usize = 1000;
pub const CONSOLE_RING_BYTES: usize = 1024 * 1024;
pub const CONSOLE_QUEUE_ENTRIES: usize = 256;
pub const CONSOLE_QUEUE_BYTES: usize = 256 * 1024;
/// Engine delivery budget per loop iteration (fairness with navigation and commands).
pub const CONSOLE_BATCH_ENTRIES: usize = 32;
pub const CONSOLE_BATCH_BYTES: usize = 64 * 1024;
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024;
pub const MAX_EXPRESSION_BYTES: usize = 16 * 1024;
pub const MAX_HISTORY_PAGE: u32 = 100;
pub const MAX_HISTORY_BYTES: usize = 64 * 1024;
pub const MAX_ARGS: usize = 16;
pub const MAX_PREVIEW_DEPTH: usize = 3;
pub const MAX_PREVIEW_NODES: usize = 64;
pub const MAX_PREVIEW_PROPS: usize = 8;
pub const MAX_PREVIEW_TEXT: usize = 256;
/// Evaluation can legitimately take the whole script deadline; the request outlives it.
pub const EVALUATE_TIMEOUT_MS: u64 = crate::EVALUATION_TIMEOUT_MS + 1_000;

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
    // Inspector version 2: console and evaluation.
    EnableConsole,
    DisableConsole,
    ClearConsole,
    GetConsoleHistory,
    GrantEvaluation,
    RevokeEvaluation,
    Evaluate,
}

impl InspectorCommand {
    /// Whether the command needs inspector message-set version 2.
    pub fn needs_console_version(self) -> bool {
        !matches!(
            self,
            Self::Attach
                | Self::Detach
                | Self::GetTargetInfo
                | Self::SubscribeStatus
                | Self::UnsubscribeStatus
                | Self::GetStatusSnapshot
        )
    }
    pub fn timeout_ms(self) -> u64 {
        if self == Self::Evaluate {
            EVALUATE_TIMEOUT_MS
        } else {
            INSPECTOR_TIMEOUT_MS
        }
    }
}

/// A 128-bit evaluation grant minted by the browser process for one session and document.
/// It is verified, never trusted: the content process compares it with the grant it stored
/// from the browser's `GrantEvaluation` command. `Debug` never prints it.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GrantToken(pub [u8; 16]);

impl std::fmt::Debug for GrantToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GrantToken(..)")
    }
}
impl PartialEq for GrantToken {
    fn eq(&self, other: &Self) -> bool {
        // Branch-free over the whole token.
        self.0
            .iter()
            .zip(other.0)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
}
impl Eq for GrantToken {}
impl GrantToken {
    pub fn is_zero(&self) -> bool {
        self.0 == [0; 16]
    }
}

/// Command parameters. A closed enum, so no command can carry a payload nobody validated.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InspectorArgs {
    #[default]
    None,
    History {
        after: u64,
        limit: u32,
    },
    Grant {
        token: GrantToken,
        /// The user explicitly allowed an opaque or internal document.
        opaque_override: bool,
    },
    Evaluate {
        expression: String,
        grant: GrantToken,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectorRequest {
    pub version: u32,
    pub session: InspectorSessionId,
    pub target: TargetId,
    pub command: InspectorCommand,
    #[serde(default)]
    pub args: InspectorArgs,
}

impl InspectorRequest {
    /// Shape and size validation that needs no state.
    pub fn validate(&self) -> Result<(), InspectorError> {
        use InspectorCommand as C;
        let bad = |message: &str| {
            Err(InspectorError::new(
                InspectorErrorCode::InvalidPayload,
                message,
            ))
        };
        if self.command.needs_console_version() && self.version < CONSOLE_VERSION {
            return Err(InspectorError::new(
                InspectorErrorCode::UnsupportedVersion,
                "Command needs inspector version 2",
            ));
        }
        match (&self.command, &self.args) {
            (C::GetConsoleHistory, InspectorArgs::History { limit, .. }) => {
                if *limit == 0 || *limit > MAX_HISTORY_PAGE {
                    return bad("History page size out of range");
                }
                Ok(())
            }
            (C::GrantEvaluation, InspectorArgs::Grant { token, .. }) => {
                if token.is_zero() {
                    return bad("Empty grant token");
                }
                Ok(())
            }
            (C::Evaluate, InspectorArgs::Evaluate { expression, .. }) => {
                if expression.len() > MAX_EXPRESSION_BYTES {
                    return bad("Expression is too large");
                }
                if expression.trim().is_empty() {
                    return bad("Empty expression");
                }
                Ok(())
            }
            (
                C::Attach
                | C::Detach
                | C::GetTargetInfo
                | C::SubscribeStatus
                | C::UnsubscribeStatus
                | C::GetStatusSnapshot
                | C::EnableConsole
                | C::DisableConsole
                | C::ClearConsole
                | C::RevokeEvaluation,
                InspectorArgs::None,
            ) => Ok(()),
            _ => bad("Arguments do not match the command"),
        }
    }
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
        let request = InspectorRequest::deserialize(&self.0).map_err(|error| {
            InspectorError::new(
                InspectorErrorCode::InvalidPayload,
                format!("Malformed inspector request: {error}"),
            )
        })?;
        request.validate()?;
        Ok(request)
    }
    /// The command named by the payload, without decoding the rest (used to pick a deadline
    /// before the full request is parsed).
    pub fn command_hint(&self) -> Option<InspectorCommand> {
        InspectorCommand::deserialize(self.0.get("command")?).ok()
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreviewKind {
    Undefined,
    Null,
    Boolean,
    Number,
    BigInt,
    String,
    Symbol,
    Function,
    Array,
    Object,
    Error,
    Date,
    RegExp,
    Map,
    Set,
    Promise,
    Proxy,
    Circular,
    /// A value the generator deliberately did not inspect (type only).
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PreviewProp {
    pub key: String,
    pub value: Preview,
}

/// A bounded, self-contained rendering of a JavaScript value. It holds no reference to the
/// value, so nothing in the page is retained by the inspector.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preview {
    pub kind: PreviewKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<PreviewProp>,
    /// Properties or elements that exist but were not included.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub more: u32,
}

fn is_false(value: &bool) -> bool {
    !*value
}
fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl Preview {
    pub fn simple(kind: PreviewKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            truncated: false,
            props: Vec::new(),
            more: 0,
        }
    }
    /// Depth, node count, per-level width and text budgets.
    pub fn is_bounded(&self) -> bool {
        fn walk(preview: &Preview, depth: usize, nodes: &mut usize) -> bool {
            *nodes += 1;
            *nodes <= MAX_PREVIEW_NODES
                && depth <= MAX_PREVIEW_DEPTH
                && preview.text.len() <= MAX_PREVIEW_TEXT * 4
                && preview.props.len() <= MAX_PREVIEW_PROPS
                && preview
                    .props
                    .iter()
                    .all(|prop| prop.key.len() <= 128 * 4 && walk(&prop.value, depth + 1, nodes))
        }
        walk(self, 0, &mut 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConsoleLevel {
    Debug,
    Log,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLocation {
    pub url: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExceptionKind {
    Uncaught,
    UnhandledRejection,
    Evaluation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExceptionDetails {
    pub kind: ExceptionKind,
    pub name: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceLocation>,
}

impl ExceptionDetails {
    pub fn is_bounded(&self) -> bool {
        self.name.len() <= 512
            && self.message.len() <= 8 * 1024
            && self.stack.as_ref().is_none_or(|s| s.len() <= 16 * 1024)
            && self.source.as_ref().is_none_or(|s| s.url.len() <= 4096)
    }
}

/// One console entry (a console call or an exception). `seq` is per document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConsoleMessage {
    pub seq: u64,
    pub timestamp_ms: u64,
    pub level: ConsoleLevel,
    pub method: String,
    #[serde(default)]
    pub args: Vec<Preview>,
    /// Single-string rendering (formatter output or exception message), bounded.
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exception: Option<ExceptionDetails>,
}

impl ConsoleMessage {
    pub fn is_bounded(&self) -> bool {
        self.method.len() <= 32
            && self.args.len() <= MAX_ARGS
            && self.args.iter().all(Preview::is_bounded)
            && self.text.len() <= 4096
            && self.source.as_ref().is_none_or(|s| s.url.len() <= 4096)
            && self
                .exception
                .as_ref()
                .is_none_or(ExceptionDetails::is_bounded)
            && self.approx_bytes() <= MAX_MESSAGE_BYTES * 2
    }
    /// Serialised size, the unit of every byte budget.
    pub fn approx_bytes(&self) -> usize {
        serde_json::to_vec(self).map_or(usize::MAX, |bytes| bytes.len())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EvalOutcome {
    Value { preview: Preview },
    Exception { details: ExceptionDetails },
}

impl EvalOutcome {
    pub fn is_bounded(&self) -> bool {
        match self {
            Self::Value { preview } => preview.is_bounded(),
            Self::Exception { details } => details.is_bounded(),
        }
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
    ConsoleEnabled {
        /// Entries currently retained for this document, and the first retained sequence.
        retained: u32,
        first_seq: u64,
    },
    ConsoleDisabled,
    ConsoleCleared,
    ConsoleHistory {
        messages: Vec<ConsoleMessage>,
        next_seq: u64,
        /// Entries evicted from the engine ring since the document began.
        dropped: u64,
    },
    EvaluationGranted,
    EvaluationRevoked,
    Evaluated(Box<EvalOutcome>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    ConsoleMessageAdded(Box<ConsoleMessage>),
    /// A console entry that carries `exception` details.
    ExceptionThrown(Box<ConsoleMessage>),
    ExecutionContextCreated {
        document: DocumentContext,
    },
    ExecutionContextDestroyed {
        document: DocumentContext,
    },
    /// Console entries lost between the engine ring and the consumer.
    ConsoleMessagesDropped {
        count: u64,
    },
}

impl InspectorEvent {
    /// Whether the event belongs to the console stream (separate, byte-bounded queue).
    pub fn is_console(&self) -> bool {
        matches!(
            self,
            Self::ConsoleMessageAdded(_)
                | Self::ExceptionThrown(_)
                | Self::ExecutionContextCreated { .. }
                | Self::ExecutionContextDestroyed { .. }
                | Self::ConsoleMessagesDropped { .. }
        )
    }
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
