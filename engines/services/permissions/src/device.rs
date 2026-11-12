//! Active device lease and hardware resource management.
use std::collections::HashMap;
use wega_protocol::DocumentContext;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaTrackKind {
    Audio,
    Video,
}

impl MediaTrackKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DeviceTrack {
    pub id: String,
    pub kind: MediaTrackKind,
    pub label: String,
    pub origin: String,
    pub document: DocumentContext,
    pub stopped: bool,
    pub cloned_from: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DeviceWakeLock {
    pub id: u64,
    pub origin: String,
    pub document: DocumentContext,
    pub released: bool,
}

#[derive(Default, Debug)]
pub struct DeviceManager {
    tracks: HashMap<String, DeviceTrack>,
    wake_locks: HashMap<u64, DeviceWakeLock>,
    next_track_id: u64,
    next_lock_id: u64,
}

impl DeviceManager {
    pub fn create_track(
        &mut self,
        origin: String,
        document: DocumentContext,
        kind: MediaTrackKind,
        label: Option<String>,
    ) -> String {
        self.next_track_id += 1;
        let id = format!("{}-track-{}", kind.as_str(), self.next_track_id);
        let default_label = match kind {
            MediaTrackKind::Audio => "Default Audio Device",
            MediaTrackKind::Video => "Default Video Camera",
        };
        let label = label.unwrap_or_else(|| default_label.to_string());
        let track = DeviceTrack {
            id: id.clone(),
            kind,
            label,
            origin,
            document,
            stopped: false,
            cloned_from: None,
        };
        self.tracks.insert(id.clone(), track);
        id
    }

    pub fn clone_track(&mut self, source_id: &str) -> Result<String, String> {
        let source = self
            .tracks
            .get(source_id)
            .ok_or_else(|| format!("Track not found: {source_id}"))?
            .clone();

        self.next_track_id += 1;
        let id = format!("{}-track-{}", source.kind.as_str(), self.next_track_id);
        let clone = DeviceTrack {
            id: id.clone(),
            kind: source.kind,
            label: format!("{} (clone)", source.label),
            origin: source.origin,
            document: source.document,
            stopped: source.stopped,
            cloned_from: Some(source_id.to_string()),
        };
        self.tracks.insert(id.clone(), clone);
        Ok(id)
    }

    pub fn stop_track(&mut self, track_id: &str) -> bool {
        if let Some(track) = self.tracks.get_mut(track_id) {
            track.stopped = true;
            true
        } else {
            false
        }
    }

    /// Stops all tracks (and their clones) for the specified origin and kind.
    pub fn stop_tracks_for_origin_and_kind(
        &mut self,
        origin: &str,
        kind: MediaTrackKind,
    ) -> Vec<String> {
        let mut stopped = Vec::new();
        for (id, track) in self.tracks.iter_mut() {
            if track.origin == origin && track.kind == kind && !track.stopped {
                track.stopped = true;
                stopped.push(id.clone());
            }
        }
        stopped
    }

    pub fn is_track_active(&self, track_id: &str) -> bool {
        self.tracks
            .get(track_id)
            .map(|t| !t.stopped)
            .unwrap_or(false)
    }

    pub fn get_track(&self, track_id: &str) -> Option<&DeviceTrack> {
        self.tracks.get(track_id)
    }

    pub fn create_wake_lock(&mut self, origin: String, document: DocumentContext) -> u64 {
        self.next_lock_id += 1;
        let id = self.next_lock_id;
        self.wake_locks.insert(
            id,
            DeviceWakeLock {
                id,
                origin,
                document,
                released: false,
            },
        );
        id
    }

    pub fn release_wake_lock(&mut self, lock_id: u64) -> bool {
        if let Some(lock) = self.wake_locks.get_mut(&lock_id) {
            lock.released = true;
            true
        } else {
            false
        }
    }

    pub fn release_wake_locks_for_origin(&mut self, origin: &str) -> Vec<u64> {
        let mut released = Vec::new();
        for (id, lock) in self.wake_locks.iter_mut() {
            if lock.origin == origin && !lock.released {
                lock.released = true;
                released.push(*id);
            }
        }
        released
    }

    pub fn is_wake_lock_active(&self, lock_id: u64) -> bool {
        self.wake_locks
            .get(&lock_id)
            .map(|l| !l.released)
            .unwrap_or(false)
    }

    pub fn cleanup_document(&mut self, document: DocumentContext) -> (Vec<String>, Vec<u64>) {
        let mut stopped_tracks = Vec::new();
        for (id, track) in self.tracks.iter_mut() {
            if track.document == document && !track.stopped {
                track.stopped = true;
                stopped_tracks.push(id.clone());
            }
        }
        let mut released_locks = Vec::new();
        for (id, lock) in self.wake_locks.iter_mut() {
            if lock.document == document && !lock.released {
                lock.released = true;
                released_locks.push(*id);
            }
        }
        (stopped_tracks, released_locks)
    }
}
