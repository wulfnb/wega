//! Versioned, bounded private pipe protocol shared with the independent engine worker.
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
pub mod authority;
pub mod browsing;
pub mod identity;
pub mod lifecycle;
pub mod media;
pub mod permissions;
pub mod queue;
pub mod requests;
pub mod storage;
pub const VERSION: u32 = 21;
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
    ScriptStopped(String),
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
    Favicon(String),
    Status(Option<String>),
    Authentication {
        url: String,
        realm: Option<String>,
    },
    Dialog {
        id: DialogId,
        kind: String,
        message: String,
        default: String,
    },
    DialogClosed(DialogId),
    Accessibility(String),
    FileDialog {
        id: u64,
        multiple: bool,
        accept: Vec<String>,
    },
    ClipboardWrite(String),
    FindResult {
        active_match_ordinal: u32,
        number_of_matches: u32,
        query: String,
    },
    MediaSession(media::MediaSessionEvent),
    PermissionPrompt(permissions::PermissionPromptRequest),
    PermissionRevoked {
        origin: String,
        feature: permissions::PermissionFeature,
    },
    NotificationShow {
        id: u64,
        title: String,
        options: permissions::NotificationOptions,
    },
    NotificationClose {
        id: u64,
    },
    Fullscreen(bool),
    ProtocolRegister(permissions::ProtocolHandlerRegistration),
    ExternalApp(String),
}

pub fn content_event(version: u32, context: DocumentContext, event: ContentEvent) -> Event {
    let event = match event {
        ContentEvent::ActivatedOpenWindow { url, .. } if version < 11 => {
            ContentEvent::OpenWindow(url)
        }
        ContentEvent::ScriptStopped(reason) if version < 14 => ContentEvent::Notice(reason),
        ContentEvent::MediaSession(_) if version < 20 => {
            ContentEvent::Notice("Media session update".into())
        }
        ContentEvent::PermissionPrompt(_) if version < 21 => {
            ContentEvent::Notice("Permission prompt".into())
        }
        ContentEvent::PermissionRevoked { feature, .. } if version < 21 => {
            ContentEvent::Notice(format!("Permission revoked: {feature}"))
        }
        ContentEvent::NotificationShow { title, .. } if version < 21 => {
            ContentEvent::Notice(format!("Notification: {title}"))
        }
        ContentEvent::NotificationClose { id } if version < 21 => {
            ContentEvent::Notice(format!("Notification closed {id}"))
        }
        ContentEvent::Fullscreen(f) if version < 21 => {
            ContentEvent::Notice(format!("Fullscreen: {f}"))
        }
        ContentEvent::ProtocolRegister(r) if version < 21 => {
            ContentEvent::Notice(format!("Register protocol: {}", r.scheme))
        }
        ContentEvent::ExternalApp(u) if version < 21 => {
            ContentEvent::Notice(format!("External app: {u}"))
        }
        event => event,
    };
    if version >= 9 {
        return Event::Content { context, event };
    }
    match event {
        ContentEvent::Navigate(url) => Event::Navigate(url),
        ContentEvent::Notice(message) | ContentEvent::ScriptStopped(message) => {
            Event::Notice(message)
        }
        ContentEvent::Compatibility(reason) => Event::Compatibility {
            serial: context.document.0,
            reason,
        },
        ContentEvent::Download { url, cookies } => Event::Download { url, cookies },
        ContentEvent::OpenWindow(url) => Event::OpenWindow(url),
        ContentEvent::ActivatedOpenWindow { url, .. } => Event::OpenWindow(url),
        ContentEvent::TraverseUnhandled(delta) => Event::TraverseUnhandled(delta),
        ContentEvent::Cursor(cursor) => Event::Cursor(cursor),
        ContentEvent::Favicon(url) => Event::Notice(format!("Favicon: {url}")),
        ContentEvent::Status(Some(status)) => Event::Notice(status),
        ContentEvent::Status(None) => Event::Notice("Status cleared".into()),
        ContentEvent::Authentication { url, .. } => {
            Event::Notice(format!("Authentication requested for {url}"))
        }
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
        ContentEvent::Accessibility(_) => Event::Notice("Accessibility update".into()),
        ContentEvent::FileDialog { id, .. } => Event::Notice(format!("File dialog {id}")),
        ContentEvent::ClipboardWrite(text) => Event::Notice(format!("Clipboard write: {text}")),
        ContentEvent::FindResult {
            active_match_ordinal,
            number_of_matches,
            query,
        } => Event::Notice(format!(
            "Find {query}: {active_match_ordinal}/{number_of_matches}"
        )),
        ContentEvent::MediaSession(_) => Event::Notice("Media session update".into()),
        ContentEvent::PermissionPrompt(_) => Event::Notice("Permission prompt".into()),
        ContentEvent::PermissionRevoked { feature, .. } => {
            Event::Notice(format!("Permission revoked: {feature}"))
        }
        ContentEvent::NotificationShow { title, .. } => {
            Event::Notice(format!("Notification: {title}"))
        }
        ContentEvent::NotificationClose { id } => {
            Event::Notice(format!("Notification closed {id}"))
        }
        ContentEvent::Fullscreen(f) => Event::Notice(format!("Fullscreen: {f}")),
        ContentEvent::ProtocolRegister(r) => {
            Event::Notice(format!("Register protocol: {}", r.scheme))
        }
        ContentEvent::ExternalApp(u) => Event::Notice(format!("External app: {u}")),
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
    InheritedBroker,
    InheritedFrames,
    ResourceBroker,
    DocumentTaskScheduling,
    ScriptRecovery,
    SiteIsolationFrames,
    AuthoritativeStorage,
    MediaPlayback,
    PermissionsRevocation,
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
    Capability::InheritedBroker,
    Capability::InheritedFrames,
    Capability::ResourceBroker,
    Capability::DocumentTaskScheduling,
    Capability::ScriptRecovery,
    Capability::SiteIsolationFrames,
    Capability::AuthoritativeStorage,
    Capability::MediaPlayback,
    Capability::PermissionsRevocation,
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
                    Capability::InheritedBroker => 13,
                    Capability::InheritedFrames => 13,
                    Capability::ResourceBroker
                    | Capability::DocumentTaskScheduling
                    | Capability::ScriptRecovery => 14,
                    Capability::SiteIsolationFrames => 15,
                    Capability::AuthoritativeStorage => 16,
                    Capability::MediaPlayback => 20,
                    Capability::PermissionsRevocation => 21,
                }
        })
        .collect()
}

pub fn scoped_context(command: &Command) -> Option<RequestContext> {
    match command {
        Command::EvaluateDocument { context, .. }
        | Command::CheckDocumentState(context)
        | Command::CaptureDocument { context, .. }
        | Command::NavigateDocument { context, .. } => Some(*context),
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
        identity: Option<Box<identity::SessionIdentity>>,
        version: u32,
        url: String,
        offline: bool,
        blocked_hosts: String,
        allowed_hosts: String,
        storage_dir: Option<String>,
        broker_address: Option<String>,
        broker_token: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        broker_fds: Vec<u64>,
        frame_path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame_fd: Option<u64>,
    },
    Load {
        url: String,
        serial: u64,
        #[serde(default)]
        bypass_cache: bool,
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
    ClearHistory,
    Policy {
        offline: bool,
        blocked_hosts: String,
        allowed_hosts: String,
        #[serde(default)]
        user_script: String,
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
    AccessibilityAction {
        action: String,
        target: u64,
        #[serde(default)]
        data: Option<String>,
    },
    EnableAccessibility(bool),
    FileSelected {
        id: u64,
        #[serde(default)]
        paths: Vec<String>,
    },
    Copy,
    Cut,
    Paste(String),
    SelectAll,
    FindInPage {
        query: String,
        #[serde(default)]
        backwards: bool,
        #[serde(default)]
        case_sensitive: bool,
    },
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
    MediaSessionAction(media::MediaSessionAction),
    AnswerPermission {
        id: u64,
        allow: bool,
    },
    RevokePermission {
        origin: String,
        feature: permissions::PermissionFeature,
    },
    ExitFullscreen,
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
    fn inherited_resource_fields_and_capabilities_are_versioned() {
        assert!(!capabilities(12).contains(&Capability::InheritedBroker));
        assert!(!capabilities(12).contains(&Capability::InheritedFrames));
        assert!(capabilities(13).contains(&Capability::InheritedBroker));
        assert!(capabilities(13).contains(&Capability::InheritedFrames));
        let old = Command::Init {
            identity: None,
            version: 4,
            url: "about:blank".into(),
            offline: true,
            blocked_hosts: String::new(),
            allowed_hosts: String::new(),
            storage_dir: None,
            broker_address: None,
            broker_token: None,
            broker_fds: Vec::new(),
            frame_path: None,
            frame_fd: None,
        };
        let packet = serde_json::to_string(&old).unwrap();
        assert!(!packet.contains("broker_fds"));
        assert!(!packet.contains("frame_fd"));
        assert!(
            matches!(serde_json::from_str::<Command>(&packet).unwrap(), Command::Init { broker_fds, frame_fd: None, .. } if broker_fds.is_empty())
        );
    }
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
        assert!(capabilities(15).contains(&Capability::SiteIsolationFrames));
        assert!(!capabilities(14).contains(&Capability::SiteIsolationFrames));

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
    #[test]
    fn navigation_and_browser_integrations_wire_compatibility() {
        let load_default: Command =
            serde_json::from_str(r#"{"Load":{"url":"https://example.test/","serial":1}}"#).unwrap();
        match load_default {
            Command::Load { bypass_cache, .. } => assert!(!bypass_cache),
            _ => panic!("Expected Command::Load"),
        }

        let load_bypass: Command = serde_json::from_str(
            r#"{"Load":{"url":"https://example.test/","serial":1,"bypass_cache":true}}"#,
        )
        .unwrap();
        match load_bypass {
            Command::Load { bypass_cache, .. } => assert!(bypass_cache),
            _ => panic!("Expected Command::Load"),
        }

        let clear: Command = serde_json::from_str(r#""ClearHistory""#).unwrap();
        assert!(matches!(clear, Command::ClearHistory));

        let policy_default: Command = serde_json::from_str(
            r#"{"Policy":{"offline":false,"blocked_hosts":"","allowed_hosts":""}}"#,
        )
        .unwrap();
        match policy_default {
            Command::Policy { user_script, .. } => assert!(user_script.is_empty()),
            _ => panic!("Expected Command::Policy"),
        }

        let policy_script: Command = serde_json::from_str(r#"{"Policy":{"offline":false,"blocked_hosts":"","allowed_hosts":"","user_script":"console.log(1)"}}"#).unwrap();
        match policy_script {
            Command::Policy { user_script, .. } => assert_eq!(user_script, "console.log(1)"),
            _ => panic!("Expected Command::Policy"),
        }

        let context = DocumentContext {
            document: DocumentId(1),
            revision: DocumentRevision(1),
        };
        let favicon_event = content_event(
            VERSION,
            context,
            ContentEvent::Favicon("https://example.test/favicon.ico".into()),
        );
        assert!(
            matches!(favicon_event, Event::Content { event: ContentEvent::Favicon(ref u), .. } if u == "https://example.test/favicon.ico")
        );

        let status_event = content_event(
            VERSION,
            context,
            ContentEvent::Status(Some("https://example.test/link".into())),
        );
        assert!(
            matches!(status_event, Event::Content { event: ContentEvent::Status(Some(ref s)), .. } if s == "https://example.test/link")
        );

        let auth_event = content_event(
            VERSION,
            context,
            ContentEvent::Authentication {
                url: "https://example.test/admin".into(),
                realm: Some("Secured".into()),
            },
        );
        assert!(
            matches!(auth_event, Event::Content { event: ContentEvent::Authentication { ref realm, .. }, .. } if realm.as_deref() == Some("Secured"))
        );
    }
    #[test]
    fn input_controls_and_accessibility_transport() {
        let action: Command =
            serde_json::from_str(r#"{"AccessibilityAction":{"action":"focus","target":42}}"#)
                .unwrap();
        match action {
            Command::AccessibilityAction {
                action,
                target,
                data,
            } => {
                assert_eq!(action, "focus");
                assert_eq!(target, 42);
                assert!(data.is_none());
            }
            _ => panic!("Expected AccessibilityAction"),
        }

        let file_selected: Command =
            serde_json::from_str(r#"{"FileSelected":{"id":10,"paths":["/tmp/test.txt"]}}"#)
                .unwrap();
        match file_selected {
            Command::FileSelected { id, paths } => {
                assert_eq!(id, 10);
                assert_eq!(paths, vec!["/tmp/test.txt"]);
            }
            _ => panic!("Expected FileSelected"),
        }

        let context = DocumentContext {
            document: DocumentId(1),
            revision: DocumentRevision(1),
        };
        let a11y_event = content_event(
            VERSION,
            context,
            ContentEvent::Accessibility(r#"{"tree_id":"root"}"#.into()),
        );
        assert!(
            matches!(a11y_event, Event::Content { event: ContentEvent::Accessibility(ref json), .. } if json.contains("tree_id"))
        );

        let file_dialog = content_event(
            VERSION,
            context,
            ContentEvent::FileDialog {
                id: 7,
                multiple: true,
                accept: vec![".png".into()],
            },
        );
        assert!(matches!(
            file_dialog,
            Event::Content {
                event: ContentEvent::FileDialog {
                    id: 7,
                    multiple: true,
                    ..
                },
                ..
            }
        ));
    }
    #[test]
    fn selection_editing_find_clipboard_transport() {
        let copy_cmd: Command = serde_json::from_str(r#""Copy""#).unwrap();
        assert!(matches!(copy_cmd, Command::Copy));

        let cut_cmd: Command = serde_json::from_str(r#""Cut""#).unwrap();
        assert!(matches!(cut_cmd, Command::Cut));

        let paste_cmd: Command = serde_json::from_str(r#"{"Paste":"hello"}"#).unwrap();
        assert!(matches!(paste_cmd, Command::Paste(ref text) if text == "hello"));

        let select_all_cmd: Command = serde_json::from_str(r#""SelectAll""#).unwrap();
        assert!(matches!(select_all_cmd, Command::SelectAll));

        let find_cmd: Command =
            serde_json::from_str(r#"{"FindInPage":{"query":"test","backwards":true}}"#).unwrap();
        assert!(
            matches!(find_cmd, Command::FindInPage { ref query, backwards: true, case_sensitive: false } if query == "test")
        );

        let context = DocumentContext {
            document: DocumentId(1),
            revision: DocumentRevision(1),
        };
        let clip_event = content_event(
            VERSION,
            context,
            ContentEvent::ClipboardWrite("copied text".into()),
        );
        assert!(
            matches!(clip_event, Event::Content { event: ContentEvent::ClipboardWrite(ref s), .. } if s == "copied text")
        );

        let find_event = content_event(
            VERSION,
            context,
            ContentEvent::FindResult {
                active_match_ordinal: 2,
                number_of_matches: 5,
                query: "sample".into(),
            },
        );
        assert!(
            matches!(find_event, Event::Content { event: ContentEvent::FindResult { active_match_ordinal: 2, number_of_matches: 5, ref query }, .. } if query == "sample")
        );
    }
    #[test]
    fn authoritative_storage_capability_and_version() {
        assert_eq!(VERSION, 21);
        assert!(capabilities(16).contains(&Capability::AuthoritativeStorage));
        assert!(!capabilities(15).contains(&Capability::AuthoritativeStorage));
        assert!(capabilities(20).contains(&Capability::MediaPlayback));
        assert!(!capabilities(19).contains(&Capability::MediaPlayback));
        assert!(capabilities(21).contains(&Capability::PermissionsRevocation));
        assert!(!capabilities(20).contains(&Capability::PermissionsRevocation));
    }
    #[test]
    fn media_session_command_and_event_transport() {
        let action_cmd: Command = serde_json::from_str(r#"{"MediaSessionAction":"Play"}"#).unwrap();
        assert!(matches!(
            action_cmd,
            Command::MediaSessionAction(media::MediaSessionAction::Play)
        ));

        let seek_cmd: Command = serde_json::from_str(
            r#"{"MediaSessionAction":{"SeekTo":{"seek_time":12.5,"fast_seek":false}}}"#,
        )
        .unwrap();
        match seek_cmd {
            Command::MediaSessionAction(media::MediaSessionAction::SeekTo {
                seek_time,
                fast_seek,
            }) => {
                assert_eq!(seek_time, 12.5);
                assert!(!fast_seek);
            }
            _ => panic!("Expected SeekTo"),
        }

        let context = DocumentContext {
            document: DocumentId(1),
            revision: DocumentRevision(1),
        };
        let meta_event = content_event(
            VERSION,
            context,
            ContentEvent::MediaSession(media::MediaSessionEvent::SetMetadata(
                media::MediaMetadata {
                    title: "Test Track".into(),
                    artist: "Artist".into(),
                    album: "Album".into(),
                    artwork: vec![],
                },
            )),
        );
        assert!(
            matches!(meta_event, Event::Content { event: ContentEvent::MediaSession(media::MediaSessionEvent::SetMetadata(ref m)), .. } if m.title == "Test Track")
        );

        let downgraded = content_event(
            19,
            context,
            ContentEvent::MediaSession(media::MediaSessionEvent::PlaybackStateChange(
                media::MediaSessionPlaybackState::Playing,
            )),
        );
        assert!(
            matches!(downgraded, Event::Content { event: ContentEvent::Notice(ref s), .. } if s.contains("Media session"))
        );
    }

    #[test]
    fn permissions_and_device_capabilities_transport() {
        let answer_cmd: Command =
            serde_json::from_str(r#"{"AnswerPermission":{"id":42,"allow":true}}"#).unwrap();
        assert!(matches!(
            answer_cmd,
            Command::AnswerPermission {
                id: 42,
                allow: true
            }
        ));

        let revoke_cmd: Command = serde_json::from_str(
            r#"{"RevokePermission":{"origin":"https://a.test","feature":"Camera"}}"#,
        )
        .unwrap();
        assert!(
            matches!(revoke_cmd, Command::RevokePermission { ref origin, feature: permissions::PermissionFeature::Camera } if origin == "https://a.test")
        );

        let exit_fs_cmd: Command = serde_json::from_str(r#""ExitFullscreen""#).unwrap();
        assert!(matches!(exit_fs_cmd, Command::ExitFullscreen));

        let context = DocumentContext {
            document: DocumentId(1),
            revision: DocumentRevision(1),
        };
        let notif_event = content_event(
            VERSION,
            context,
            ContentEvent::NotificationShow {
                id: 99,
                title: "Test Alert".into(),
                options: permissions::NotificationOptions {
                    body: Some("Alert body".into()),
                    ..Default::default()
                },
            },
        );
        assert!(
            matches!(notif_event, Event::Content { event: ContentEvent::NotificationShow { id: 99, ref title, .. }, .. } if title == "Test Alert")
        );

        let fs_event = content_event(VERSION, context, ContentEvent::Fullscreen(true));
        assert!(matches!(
            fs_event,
            Event::Content {
                event: ContentEvent::Fullscreen(true),
                ..
            }
        ));
    }
}
