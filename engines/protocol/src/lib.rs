//! Versioned, bounded private pipe protocol shared with the independent engine worker.
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
pub mod requests;
pub const VERSION: u32 = 6;
pub const MAX_MESSAGE: usize = 1024 * 1024;
pub const MAX_PIXELS: usize = 4096 * 2160;
pub const MAX_RESULT_BYTES: usize = 128 * 1024;

/// A generation within one private content-process session. Zero is its initial document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestContext {
    pub document: DocumentId,
    pub request: RequestId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    ScopedEvaluation,
    ScopedPageState,
    SharedFrames,
}

pub const CAPABILITIES: &[Capability] = &[
    Capability::ScopedEvaluation,
    Capability::ScopedPageState,
    Capability::SharedFrames,
];

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

#[derive(Debug, Serialize, Deserialize)]
pub enum Command {
    Capabilities,
    EvaluateDocument {
        context: RequestContext,
        source: String,
    },
    CheckDocumentState(RequestContext),
    Init {
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
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
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
}
