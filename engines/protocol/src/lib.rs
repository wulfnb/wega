//! Versioned, bounded private pipe protocol shared with the independent engine worker.
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
pub mod authority;
pub mod identity;
pub mod lifecycle;
pub mod queue;
pub mod requests;
pub const VERSION: u32 = 13;
pub const MAX_MESSAGE: usize = 1024 * 1024;
pub const MAX_PIXELS: usize = 4096 * 2160;
pub const MAX_RESULT_BYTES: usize = 128 * 1024;
pub const EVALUATION_TIMEOUT_MS: u64 = 5_000;
pub const PAGE_STATE_TIMEOUT_MS: u64 = 2_000;
pub const HARNESS_TIMEOUT_MS: u64 = 10_000;

/// A generation within one private content-process session. Zero is its initial document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentId(pub u64);

/// Actual DOM/global replacement sequence within a content process, including
/// replacements initiated by a form rather than a browser Load command.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentRevision(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DocumentContext {
    pub document: DocumentId,
    pub revision: DocumentRevision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DialogId(pub u64);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ContentEvent {
    Navigate(String),
    Notice(String),
    Compatibility(String),
    Download {
        url: String,
        cookies: String,
    },
    OpenWindow(String),
    ActivatedOpenWindow {
        url: String,
        activation: identity::ActivationId,
    },
    TraverseUnhandled(i32),
    Cursor(String),
    Dialog {
        id: DialogId,
        kind: String,
        message: String,
        default: String,
    },
    DialogClosed(DialogId),
}

pub fn content_event(version: u32, context: DocumentContext, event: ContentEvent) -> Event {
    let event = match event {
        ContentEvent::ActivatedOpenWindow { url, .. } if version < 11 => {
            ContentEvent::OpenWindow(url)
        }
        event => event,
    };
    if version >= 9 {
        return Event::Content { context, event };
    }
    match event {
        ContentEvent::Navigate(url) => Event::Navigate(url),
        ContentEvent::Notice(message) => Event::Notice(message),
        ContentEvent::Compatibility(reason) => Event::Compatibility {
            serial: context.document.0,
            reason,
        },
        ContentEvent::Download { url, cookies } => Event::Download { url, cookies },
        ContentEvent::OpenWindow(url) => Event::OpenWindow(url),
        ContentEvent::ActivatedOpenWindow { url, .. } => Event::OpenWindow(url),
        ContentEvent::TraverseUnhandled(delta) => Event::TraverseUnhandled(delta),
        ContentEvent::Cursor(cursor) => Event::Cursor(cursor),
        ContentEvent::Dialog {
            kind,
            message,
            default,
            ..
        } => Event::Dialog {
            kind,
            message,
            default,
        },
        ContentEvent::DialogClosed(_) => Event::Notice("Page dialog closed".into()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RequestContext {
    pub document: DocumentId,
    #[serde(default)]
    pub revision: DocumentRevision,
    pub request: RequestId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    ScopedEvaluation,
    ScopedPageState,
    DocumentCommitTracking,
    QueuedCancellation,
    ScopedContentEvents,
    GracefulShutdown,
    SessionIdentity,
    SharedFrames,
    HarnessObservability,
    SiteIsolationFrames,
}

pub const CAPABILITIES: &[Capability] = &[
    Capability::ScopedEvaluation,
    Capability::ScopedPageState,
    Capability::DocumentCommitTracking,
    Capability::QueuedCancellation,
    Capability::ScopedContentEvents,
    Capability::GracefulShutdown,
    Capability::SessionIdentity,
    Capability::SharedFrames,
    Capability::HarnessObservability,
    Capability::SiteIsolationFrames,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticRecord {
    pub level: DiagnosticLevel,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub col: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    StaleDocument,
    Cancelled,
    Timeout,
    ResourceLimit,
    InvalidRequest,
    Unsupported,
    ScriptException,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
}

impl ProtocolError {
    pub fn new(code: ErrorCode, message: impl AsRef<str>) -> Self {
        Self {
            code,
            message: message.as_ref().chars().take(4096).collect(),
        }
    }
}

pub fn supports_version(version: u32) -> bool {
    matches!(version, 4..=VERSION)
}

pub fn capabilities(version: u32) -> Vec<Capability> {
    CAPABILITIES
        .iter()
        .copied()
        .filter(|capability| {
            version
                >= match capability {
                    Capability::SharedFrames => 4,
                    Capability::ScopedEvaluation | Capability::ScopedPageState => 6,
                    Capability::DocumentCommitTracking => 7,
                    Capability::QueuedCancellation => 8,
                    Capability::ScopedContentEvents => 9,
                    Capability::GracefulShutdown => 10,
                    Capability::SessionIdentity => 11,
                    Capability::HarnessObservability => 12,
                    Capability::SiteIsolationFrames => 13,
                }
        })
        .collect()
}

pub fn scoped_context(command: &Command) -> Option<RequestContext> {
    match command {
        Command::EvaluateDocument { context, .. }
        | Command::CheckDocumentState(context)
        | Command::CaptureDocument { context, .. }
        | Command::NavigateDocument { context, .. } => {
            Some(*context)
        }
        _ => None,
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Command {
    Session {
        identity: identity::SessionIdentity,
        command: Box<Command>,
    },
    Activate {
        context: DocumentContext,
        activation: identity::ActivationId,
    },
    AnswerDialog {
        context: DocumentContext,
        id: DialogId,
        value: Option<String>,
    },
    Capabilities,
    EvaluateDocument {
        context: RequestContext,
        source: String,
    },
    CheckDocumentState(RequestContext),
    CaptureDocument {
        context: RequestContext,
    },
    NavigateDocument {
        context: RequestContext,
        url: String,
    },
    /// Cancels a queued operation. Already-running script side effects cannot be rolled back.
    CancelRequest(RequestContext),
    Init {
        #[serde(default)]
        identity: Option<identity::SessionIdentity>,
        version: u32,
        url: String,
        offline: bool,
        blocked_hosts: String,
        allowed_hosts: String,
        storage_dir: Option<String>,
        broker_address: Option<String>,
        broker_token: Option<String>,
        frame_path: Option<String>,
    },
    Load {
        url: String,
        serial: u64,
    },
    FrameConsumed {
        slot: u8,
        sequence: u64,
    },
    Resize {
        width: u32,
        height: u32,
        scale: f32,
        zoom: f32,
        dark: bool,
    },
    Click {
        x: f32,
        y: f32,
    },
    Key {
        key: String,
        code: String,
        text: Option<String>,
        pressed: bool,
        modifiers: u8,
        repeat: bool,
    },
    Ime {
        text: String,
        preedit: bool,
        cursor: Option<(usize, usize)>,
    },
    Scroll {
        x: f64,
        y: f64,
        pointer_x: f32,
        pointer_y: f32,
    },
    Freeze(bool),
    Stop,
    Redraw,
    Policy {
        offline: bool,
        blocked_hosts: String,
        allowed_hosts: String,
    },
    Evaluate(String),
    CheckPageState(u64),
    /// Pointer input in viewport CSS pixels. `kind` is move, down, up, leave or wheel;
    /// `button` and `buttons` follow the DOM; modifiers: 1 shift, 2 control, 4 alt, 8 meta.
    Pointer {
        kind: String,
        x: f32,
        y: f32,
        button: u8,
        buttons: u8,
        modifiers: u8,
        delta_x: f64,
        delta_y: f64,
    },
    /// Browser back (-1) or forward (+1); the worker answers `TraverseUnhandled`
    /// when the step leaves the document's own history entries.
    Traverse(i32),
    /// The answer to `Event::Dialog`: None for cancel, Some("true") for an accepted
    /// confirm, Some(text) for prompt (alert ignores the value).
    DialogReply(Option<String>),
    Shutdown,
    /// Instruct child frame browsing context to navigate.
    FrameNavigate {
        frame: identity::FrameId,
        url: String,
    },
    /// Route a postMessage from the host or another frame to the designated frame.
    FramePostMessage {
        target: identity::FrameId,
        origin: String,
        target_origin: Option<String>,
        message: String,
    },
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    Session {
        identity: identity::SessionIdentity,
        event: Box<Event>,
    },
    /// DOM/JS ownership was released; the supervisor must still observe OS process exit.
    ShutdownComplete,
    Content {
        context: DocumentContext,
        event: ContentEvent,
    },
    DocumentCommitted {
        serial: u64,
        revision: DocumentRevision,
        /// Corroborated against the host service receipt; never trusted on its own.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    Capabilities {
        version: u32,
        supported: Vec<Capability>,
    },
    DocumentEvaluated {
        context: RequestContext,
        result: Result<String, ProtocolError>,
    },
    DocumentState {
        context: RequestContext,
        dirty: bool,
    },
    DocumentCaptured {
        context: RequestContext,
        width: u32,
        height: u32,
        hash: String,
    },
    DocumentNavigated {
        context: RequestContext,
        status: Result<(), ProtocolError>,
    },
    Diagnostic {
        context: Option<DocumentContext>,
        record: DiagnosticRecord,
    },
    RequestFailed {
        context: RequestContext,
        error: ProtocolError,
    },
    PageState {
        request: u64,
        dirty: bool,
    },
    Ready {
        version: u32,
    },
    State {
        serial: u64,
        rss_bytes: Option<u64>,
        url: String,
        title: String,
        loading: bool,
        nodes: usize,
        js_heap: usize,
        /// The page's own session history (pushState, fragments) can go back / forward.
        back: bool,
        forward: bool,
    },
    Compatibility {
        serial: u64,
        reason: String,
    },
    SharedFrame {
        serial: u64,
        #[serde(default)]
        revision: DocumentRevision,
        width: u32,
        height: u32,
        slot: u8,
        sequence: u64,
    },
    Navigate(String),
    Notice(String),
    Evaluated {
        value: String,
        error: Option<String>,
    },
    /// Exactly width*height*4 raw RGBA bytes follow this bounded JSON packet.
    Frame {
        serial: u64,
        #[serde(default)]
        revision: DocumentRevision,
        width: u32,
        height: u32,
    },
    /// The navigation response is a file to download, not a page. `cookies` are
    /// Netscape cookie-file lines from the worker's jar for the download manager.
    Download {
        url: String,
        cookies: String,
    },
    /// A user-activated popup or `target=_blank` link to open in a new tab.
    OpenWindow(String),
    /// The CSS cursor name over the hovered element.
    Cursor(String),
    /// A blocking `alert`, `confirm` or `prompt`; reply with `Command::DialogReply`.
    Dialog {
        kind: String,
        message: String,
        default: String,
    },
    /// A traversal outside the page's own entries, for the browser's history.
    TraverseUnhandled(i32),
    /// A frame element was attached to the document tree.
    FrameAttached {
        parent: identity::FrameId,
        frame: identity::FrameId,
        url: String,
        name: Option<String>,
        sandbox: Option<String>,
    },
    /// A frame element was removed from the document tree.
    FrameDetached {
        frame: identity::FrameId,
    },
    /// A frame navigated to a new URL and committed an origin.
    FrameNavigated {
        frame: identity::FrameId,
        url: String,
        origin: String,
    },
    /// Cross-document message dispatched by `window.postMessage`.
    FramePostMessage {
        source: identity::FrameId,
        target: identity::FrameId,
        origin: String,
        target_origin: Option<String>,
        message: String,
    },
}
pub fn frame_bytes(width: u32, height: u32) -> io::Result<usize> {
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .filter(|n| *n > 0 && *n <= MAX_PIXELS)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid engine frame size"))?;
    Ok(pixels * 4)
}
pub fn write<T: Serialize>(stream: &mut impl Write, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Engine message is too large",
        ));
    }
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()
}
pub fn read<T: for<'a> Deserialize<'a>>(stream: &mut impl Read) -> io::Result<Option<T>> {
    let mut size = [0; 4];
    if stream.read(&mut size[..1])? == 0 {
        return Ok(None);
    }
    stream.read_exact(&mut size[1..])?;
    let len = u32::from_le_bytes(size) as usize;
    if len == 0 || len > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid engine message length",
        ));
    }
    let mut bytes = vec![0; len];
    stream.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activation_packets_are_downgraded_for_running_v9_and_v10_hosts() {
        #[derive(Deserialize)]
        enum OldContent {
            OpenWindow(String),
        }
        #[derive(Deserialize)]
        enum OldEvent {
            Content {
                context: DocumentContext,
                event: OldContent,
            },
        }
        let context = DocumentContext {
            document: DocumentId(2),
            revision: DocumentRevision(3),
        };
        for version in [9, 10] {
            let bytes = serde_json::to_vec(&content_event(
                version,
                context,
                ContentEvent::ActivatedOpenWindow {
                    url: "https://example.test/".into(),
                    activation: identity::ActivationId(1),
                },
            ))
            .unwrap();
            let OldEvent::Content {
                context: received,
                event: OldContent::OpenWindow(url),
            } = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(received, context);
            assert_eq!(url, "https://example.test/");
        }
    }
    #[test]
    fn scoped_content_is_never_sent_to_an_older_protocol() {
        #[derive(Deserialize)]
        enum LegacyEvent {
            Navigate(String),
        }
        let context = DocumentContext {
            document: DocumentId(1),
            revision: DocumentRevision(2),
        };
        let packet = serde_json::to_vec(&content_event(
            8,
            context,
            ContentEvent::Navigate("https://example.org/".into()),
        ))
        .unwrap();
        let LegacyEvent::Navigate(url) = serde_json::from_slice(&packet).unwrap();
        assert_eq!(url, "https://example.org/");
        assert!(!capabilities(8).contains(&Capability::ScopedContentEvents));
        assert!(
            matches!(content_event(VERSION, context, ContentEvent::Navigate(url)), Event::Content { context: owner, .. } if owner == context)
        );
    }
    #[test]
    fn capabilities_remain_decodable_by_running_v6_hosts() {
        #[derive(Debug, Deserialize)]
        enum LegacyCapability {
            ScopedEvaluation,
            ScopedPageState,
            SharedFrames,
        }
        #[derive(Debug, Deserialize)]
        enum LegacyEvent {
            Capabilities {
                version: u32,
                supported: Vec<LegacyCapability>,
            },
        }
        let packet = serde_json::to_vec(&Event::Capabilities {
            version: 6,
            supported: capabilities(6),
        })
        .unwrap();
        let LegacyEvent::Capabilities { version, supported } =
            serde_json::from_slice(&packet).unwrap();
        assert_eq!(version, 6);
        assert_eq!(supported.len(), 3);
        assert!(!capabilities(7).contains(&Capability::QueuedCancellation));
        assert!(capabilities(7).contains(&Capability::DocumentCommitTracking));
        assert!(capabilities(VERSION).contains(&Capability::QueuedCancellation));
    }
    #[test]
    fn packets_reject_truncation_and_oversized_messages_before_allocation() {
        assert!(read::<Event>(&mut &((MAX_MESSAGE as u32 + 1).to_le_bytes())[..]).is_err());
        assert!(read::<Event>(&mut &[1, 0][..]).is_err());
        assert!(read::<Event>(&mut &[][..]).unwrap().is_none());
        let mut bytes = vec![];
        write(&mut bytes, &Event::Notice("Unicode: café".into())).unwrap();
        assert!(
            matches!(read::<Event>(&mut bytes.as_slice()).unwrap(),Some(Event::Notice(s)) if s=="Unicode: café")
        );
        for event in [
            Event::Download {
                url: "https://example.org/a.pdf".into(),
                cookies: "example.org\tFALSE\t/\tTRUE\t0\tid\t1\n".into(),
            },
            Event::OpenWindow("https://example.org/".into()),
            Event::Cursor("pointer".into()),
            Event::TraverseUnhandled(-1),
            Event::Dialog {
                kind: "prompt".into(),
                message: "Name?".into(),
                default: "x".into(),
            },
        ] {
            let mut bytes = vec![];
            write(&mut bytes, &event).unwrap();
            let decoded = read::<Event>(&mut bytes.as_slice()).unwrap().unwrap();
            assert_eq!(format!("{decoded:?}"), format!("{event:?}"));
        }
        let mut bytes = vec![];
        write(&mut bytes, &Command::DialogReply(Some("yes".into()))).unwrap();
        assert!(matches!(
            read::<Command>(&mut bytes.as_slice()).unwrap(),
            Some(Command::DialogReply(Some(value))) if value == "yes"
        ));
        assert!(frame_bytes(0, 1).is_err());
        assert!(frame_bytes(u32::MAX, u32::MAX).is_err());
        assert_eq!(frame_bytes(640, 480).unwrap(), 640 * 480 * 4);
    }
    #[test]
    fn site_isolation_frames_capability_and_origin_policy() {
        assert!(capabilities(13).contains(&Capability::SiteIsolationFrames));
        assert!(!capabilities(12).contains(&Capability::SiteIsolationFrames));

        use crate::authority::{Origin, is_permitted_cross_origin_property};
        let origin_a = Origin::Tuple("https://a.test".into());
        let origin_b = Origin::Tuple("https://b.test".into());
        let opaque = Origin::Opaque;

        assert!(origin_a.is_same_origin(&origin_a));
        assert!(!origin_a.is_same_origin(&origin_b));
        assert!(!origin_a.is_same_origin(&opaque));
        assert!(!opaque.is_same_origin(&opaque));

        assert!(origin_a.allows_target_origin("*"));
        assert!(origin_a.allows_target_origin("https://a.test"));
        assert!(!origin_a.allows_target_origin("https://b.test"));
        assert!(!opaque.allows_target_origin("https://a.test"));

        assert!(is_permitted_cross_origin_property("postMessage"));
        assert!(is_permitted_cross_origin_property("location"));
        assert!(is_permitted_cross_origin_property("parent"));
        assert!(is_permitted_cross_origin_property("top"));
        assert!(!is_permitted_cross_origin_property("document"));
        assert!(!is_permitted_cross_origin_property("eval"));
        assert!(!is_permitted_cross_origin_property("cookie"));
    }
}
