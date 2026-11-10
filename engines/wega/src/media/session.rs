//! Media Session API: metadata, playback state, position state, and action dispatch.
use wega_protocol::media::{
    MediaMetadata, MediaPositionState, MediaSessionEvent, MediaSessionPlaybackState,
};

#[derive(Debug, Default)]
pub struct MediaSession {
    pub metadata: Option<MediaMetadata>,
    pub playback_state: MediaSessionPlaybackState,
    pub position_state: Option<MediaPositionState>,
}

impl MediaSession {
    pub fn new() -> Self {
        Self {
            metadata: None,
            playback_state: MediaSessionPlaybackState::None,
            position_state: None,
        }
    }

    pub fn set_metadata(&mut self, metadata: MediaMetadata) -> MediaSessionEvent {
        self.metadata = Some(metadata.clone());
        MediaSessionEvent::SetMetadata(metadata)
    }

    pub fn set_playback_state(&mut self, state: MediaSessionPlaybackState) -> MediaSessionEvent {
        self.playback_state = state;
        MediaSessionEvent::PlaybackStateChange(state)
    }

    pub fn set_position_state(&mut self, position: MediaPositionState) -> MediaSessionEvent {
        self.position_state = Some(position);
        MediaSessionEvent::SetPositionState(position)
    }
}
