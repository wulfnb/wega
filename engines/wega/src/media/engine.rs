//! High-level media engine coordinating media elements, demuxers, buffers, and sessions.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use wega_protocol::media::{MediaSessionAction, MediaSessionPlaybackState};

use super::buffer::MediaBufferManager;
use super::clock::{MediaClock, MediaError, MediaErrorCode, NetworkState, ReadyState};
use super::session::MediaSession;
use super::wav::WavInfo;
use super::webm::WebmInfo;

static NEXT_MEDIA_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MediaId(pub u64);

impl MediaId {
    pub fn next() -> Self {
        Self(NEXT_MEDIA_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MediaKind {
    Audio,
    Video,
}

#[derive(Debug)]
pub struct MediaInstance {
    pub id: MediaId,
    pub kind: MediaKind,
    pub clock: MediaClock,
    pub buffer: MediaBufferManager,
    pub src: String,
    pub current_src: String,
    pub video_width: u32,
    pub video_height: u32,
    pub data: Option<Vec<u8>>,
    pub format_name: String,
}

impl MediaInstance {
    pub fn new(id: MediaId, kind: MediaKind, max_buffer_bytes: usize) -> Self {
        Self {
            id,
            kind,
            clock: MediaClock::new(),
            buffer: MediaBufferManager::new(max_buffer_bytes),
            src: String::new(),
            current_src: String::new(),
            video_width: 0,
            video_height: 0,
            data: None,
            format_name: String::new(),
        }
    }

    pub fn load_bytes(&mut self, url: &str, bytes: Vec<u8>) -> Result<(), MediaError> {
        self.src = url.to_string();
        self.current_src = url.to_string();
        self.clock.error = None;
        self.clock.network_state = NetworkState::Loading;

        // Try WebM
        if let Ok(webm) = WebmInfo::parse(&bytes) {
            self.format_name = "webm".into();
            self.clock.duration = webm.duration_seconds;
            if let Some(vid) = webm.video_track {
                self.video_width = vid.pixel_width;
                self.video_height = vid.pixel_height;
            }
            self.clock.ready_state = ReadyState::HaveEnoughData;
            self.clock.network_state = NetworkState::Idle;
            let _ = self.buffer.allocate(bytes.len());
            self.buffer.set_buffered_range(0.0, webm.duration_seconds);
            self.data = Some(bytes);
            return Ok(());
        }

        // Try WAV
        if let Ok(wav) = WavInfo::parse(&bytes) {
            self.format_name = "wav".into();
            self.clock.duration = wav.duration_seconds;
            self.clock.ready_state = ReadyState::HaveEnoughData;
            self.clock.network_state = NetworkState::Idle;
            let _ = self.buffer.allocate(bytes.len());
            self.buffer.set_buffered_range(0.0, wav.duration_seconds);
            self.data = Some(bytes);
            return Ok(());
        }

        // If neither parsed, report decode error
        let err = MediaError {
            code: MediaErrorCode::Decode,
            message: "Unsupported media format or corrupted stream".into(),
        };
        self.clock.error = Some(err.clone());
        self.clock.network_state = NetworkState::NoSource;
        self.clock.ready_state = ReadyState::HaveNothing;
        Err(err)
    }

    pub fn render_current_video_frame(&mut self) -> Option<Vec<u8>> {
        if self.video_width == 0 || self.video_height == 0 {
            return None;
        }
        let w = self.video_width as usize;
        let h = self.video_height as usize;
        let mut pixels = vec![0u8; w * h * 4];
        let t = self.clock.current_position();
        let phase = ((t * 2.0).sin() * 0.5 + 0.5) as f32;

        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) * 4;
                let r = ((x as f32 / w as f32) * 255.0) as u8;
                let g = (phase * 255.0) as u8;
                let b = ((y as f32 / h as f32) * 255.0) as u8;
                pixels[idx] = r;
                pixels[idx + 1] = g;
                pixels[idx + 2] = b;
                pixels[idx + 3] = 255;
            }
        }
        Some(pixels)
    }
}

#[derive(Debug)]
pub struct MediaEngine {
    instances: HashMap<MediaId, MediaInstance>,
    pub session: MediaSession,
    max_total_buffer: usize,
    current_total_buffer: usize,
}

impl Default for MediaEngine {
    fn default() -> Self {
        Self::new(64 * 1024 * 1024)
    }
}

impl MediaEngine {
    pub fn new(max_total_buffer: usize) -> Self {
        Self {
            instances: HashMap::new(),
            session: MediaSession::new(),
            max_total_buffer,
            current_total_buffer: 0,
        }
    }

    pub fn can_play_type(mime: &str) -> &'static str {
        let clean = mime.trim().to_ascii_lowercase();
        if clean.is_empty() {
            return "";
        }
        let parts: Vec<&str> = clean.split(';').map(|s| s.trim()).collect();
        let base_type = parts[0];
        let codecs_param = parts.iter().find(|p| p.starts_with("codecs=")).map(|p| {
            p.trim_start_matches("codecs=")
                .trim_matches('"')
                .trim_matches('\'')
        });

        match base_type {
            "audio/wav" | "audio/x-wav" | "audio/wave" => "probably",
            "video/webm" => {
                if let Some(codecs) = codecs_param {
                    if codecs.contains("vp8")
                        || codecs.contains("vp9")
                        || codecs.contains("opus")
                        || codecs.contains("vorbis")
                    {
                        "probably"
                    } else {
                        ""
                    }
                } else {
                    "maybe"
                }
            }
            "audio/webm" => {
                if let Some(codecs) = codecs_param {
                    if codecs.contains("opus") || codecs.contains("vorbis") {
                        "probably"
                    } else {
                        ""
                    }
                } else {
                    "maybe"
                }
            }
            "audio/mpeg" | "audio/mp3" => "maybe",
            "video/mp4" | "audio/mp4" => "maybe",
            "audio/ogg" | "video/ogg" => "maybe",
            _ => "",
        }
    }

    pub fn create_instance(&mut self, kind: MediaKind) -> MediaId {
        let id = MediaId::next();
        let instance = MediaInstance::new(id, kind, self.max_total_buffer);
        self.instances.insert(id, instance);
        id
    }

    pub fn get_instance_mut(&mut self, id: MediaId) -> Option<&mut MediaInstance> {
        self.instances.get_mut(&id)
    }

    pub fn get_instance(&self, id: MediaId) -> Option<&MediaInstance> {
        self.instances.get(&id)
    }

    pub fn destroy_instance(&mut self, id: MediaId) -> bool {
        if let Some(mut inst) = self.instances.remove(&id) {
            let allocated = inst.buffer.allocated_bytes();
            self.current_total_buffer = self.current_total_buffer.saturating_sub(allocated);
            inst.buffer.deallocate(allocated);
            true
        } else {
            false
        }
    }

    pub fn handle_action(&mut self, action: MediaSessionAction) {
        match action {
            MediaSessionAction::Play => {
                for inst in self.instances.values_mut() {
                    let _ = inst.clock.play();
                }
                self.session
                    .set_playback_state(MediaSessionPlaybackState::Playing);
            }
            MediaSessionAction::Pause | MediaSessionAction::Stop => {
                for inst in self.instances.values_mut() {
                    inst.clock.pause();
                }
                self.session
                    .set_playback_state(MediaSessionPlaybackState::Paused);
            }
            MediaSessionAction::SeekBackward => {
                for inst in self.instances.values_mut() {
                    let cur = inst.clock.current_position();
                    inst.clock.seek((cur - 10.0).max(0.0));
                }
            }
            MediaSessionAction::SeekForward => {
                for inst in self.instances.values_mut() {
                    let cur = inst.clock.current_position();
                    inst.clock.seek(cur + 10.0);
                }
            }
            MediaSessionAction::SeekTo {
                seek_time,
                fast_seek: _,
            } => {
                for inst in self.instances.values_mut() {
                    inst.clock.seek(seek_time);
                }
            }
            MediaSessionAction::PreviousTrack
            | MediaSessionAction::NextTrack
            | MediaSessionAction::SkipAd => {}
        }
    }
}
