//! Media Session actions, metadata and playback state protocol types.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum MediaSessionAction {
    Play,
    Pause,
    Stop,
    SeekBackward,
    SeekForward,
    PreviousTrack,
    NextTrack,
    SkipAd,
    SeekTo { seek_time: f64, fast_seek: bool },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaSessionPlaybackState {
    #[default]
    None,
    Paused,
    Playing,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaArtwork {
    pub src: String,
    pub sizes: String,
    pub r#type: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    #[serde(default)]
    pub artwork: Vec<MediaArtwork>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaPositionState {
    pub duration: f64,
    pub playback_rate: f64,
    pub position: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MediaSessionEvent {
    SetMetadata(MediaMetadata),
    PlaybackStateChange(MediaSessionPlaybackState),
    SetPositionState(MediaPositionState),
}
