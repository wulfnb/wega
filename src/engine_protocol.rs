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
    Shutdown,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
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
        assert!(frame_bytes(0, 1).is_err());
        assert!(frame_bytes(u32::MAX, u32::MAX).is_err());
        assert_eq!(frame_bytes(640, 480).unwrap(), 640 * 480 * 4);
    }
}
