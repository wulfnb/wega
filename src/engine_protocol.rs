//! Versioned, bounded private pipe protocol shared with the independent engine worker.
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
pub const VERSION: u32 = 3;
pub const MAX_MESSAGE: usize = 1024 * 1024;
pub const MAX_PIXELS: usize = 4096 * 2160;

#[derive(Debug, Serialize, Deserialize)]
pub enum Command {
    Init {
        version: u32,
        url: String,
        offline: bool,
        blocked_hosts: String,
        allowed_hosts: String,
        storage_dir: Option<String>,
    },
    Load(String),
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
    Ready {
        version: u32,
    },
    State {
        url: String,
        title: String,
        loading: bool,
        nodes: usize,
        js_heap: usize,
        /// The page's own session history (pushState, fragments) can go back / forward.
        back: bool,
        forward: bool,
    },
    Navigate(String),
    Notice(String),
    Evaluated {
        value: String,
        error: Option<String>,
    },
    /// Exactly width*height*4 raw RGBA bytes follow this bounded JSON packet.
    Frame {
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
