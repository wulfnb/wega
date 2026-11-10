# Step 20 Contract: Media Playback and Audio Subsystem

## 1. Overview & Objectives

Step 20 delivers real media playback, audio demuxing, video stream decoding, and MediaSession integration for Wega:
- **Audio & Video Demuxing Subsystem** (`engines/wega/src/media/`):
  - **RIFF WAVE Parser** (`wav.rs`):
    - Parses RIFF/WAVE container chunks (`fmt `, `data`).
    - Extracts channels, sample rate, bit depth, byte rate, block align, data lengths, and total samples.
    - Computes accurate duration and decodes linear PCM samples.
  - **Matroska / WebM EBML Demuxer** (`webm.rs`):
    - Variable-size integer (VINT) decoder parsing EBML header and IDs.
    - Parses Segment Info: TimecodeScale (nanoseconds per tick) and Duration.
    - Parses TrackEntry for Video: CodecID (`V_VP8`, `V_VP9`), PixelWidth, PixelHeight.
    - Parses TrackEntry for Audio: CodecID (`A_OPUS`, `A_VORBIS`), Channels, SamplingFrequency.
    - Extracts video stream dimensions (`videoWidth`, `videoHeight`) and frame timing.
  - **Media Playback Clock** (`clock.rs`):
    - Monotonic timeline tracker supporting play, pause, seek, volume/mute, and looping.
    - Playback rate adjustments (e.g. 0.5x, 1x, 2x).
    - Media ready states: `HAVE_NOTHING (0)`, `HAVE_METADATA (1)`, `HAVE_CURRENT_DATA (2)`, `HAVE_FUTURE_DATA (3)`, `HAVE_ENOUGH_DATA (4)`.
    - Network states: `NETWORK_EMPTY (0)`, `NETWORK_IDLE (1)`, `NETWORK_LOADING (2)`, `NETWORK_NO_SOURCE (3)`.
    - Error reporting: `MEDIA_ERR_ABORTED (1)`, `MEDIA_ERR_NETWORK (2)`, `MEDIA_ERR_DECODE (3)`, `MEDIA_ERR_SRC_NOT_SUPPORTED (4)`.
  - **Bounded Buffers & Quotas** (`buffer.rs`):
    - Bounded media buffer allocation tracking and memory accounting.
    - Rejects allocations exceeding memory quotas with `QuotaExceededError`.
    - Tracks buffered `TimeRanges`.
  - **Media Engine Coordinator** (`engine.rs`):
    - Manages live media elements by `MediaId`.
    - Capability reporting via `can_play_type`: returns `"probably"` for supported formats (`audio/wav`, `video/webm` with VP8/VP9/Opus codecs), `"maybe"` for generic types (`video/mp4`, `audio/mp3`, `audio/ogg`), and `""` for unsupported formats.
    - Video frame generator generating RGBA frames for `<video>` rendering.
    - Autoplay policy support: autoplay allowed for muted media.
  - **MediaSession Coordination** (`session.rs`):
    - Manages `MediaMetadata` (title, artist, album, artwork).
    - Manages `playbackState` (`none`, `paused`, `playing`).
    - Manages `positionState` (duration, playbackRate, position).
    - Dispatches actions: `Play`, `Pause`, `Stop`, `SeekBackward`, `SeekForward`, `PreviousTrack`, `NextTrack`, `SeekTo`.
- **Protocol & Inter-Process Architecture** (`engines/protocol/src/media.rs`):
  - Protocol types: `MediaSessionAction`, `MediaSessionPlaybackState`, `MediaMetadata`, `MediaPositionState`, `MediaSessionEvent`.
  - `ContentEvent::MediaSession(MediaSessionEvent)` and `Command::MediaSessionAction(MediaSessionAction)`.
  - Protocol version incremented to 20 with `Capability::MediaPlayback`.
- **JavaScript & DOM Semantics** (`engines/wega/src/dom/semantics.js`):
  - `HTMLMediaElement` with full properties (`currentTime`, `duration`, `paused`, `volume`, `muted`, `loop`, `playbackRate`, `readyState`, `networkState`, `error`, `buffered`, `canPlayType()`, `play()`, `pause()`, `load()`, `fastSeek()`).
  - `HTMLVideoElement` (`videoWidth`, `videoHeight`, `poster`, `playsInline`).
  - `HTMLAudioElement` and global `Audio` constructor (`new Audio([src])`).
  - `MediaError`, `TimeRanges`, `MediaMetadata`, `MediaSession`.
  - `navigator.mediaSession` singleton attached to `navigator`.
  - Element-local fallback panel (`data-wega-media-message` with download link) on media errors.

## 2. Architecture & Implementation

### Module Layout (`engines/wega/src/media/`)
- `wav.rs`: RIFF WAVE container parser.
- `webm.rs`: WebM EBML demuxer and track metadata parser.
- `clock.rs`: Monotonic playback clock, rate changes, seeking, looping, and state machine.
- `buffer.rs`: Bounded buffer manager and `TimeRange` accounting.
- `session.rs`: MediaSession manager tracking metadata and states.
- `engine.rs`: High-level media engine coordinating instances and codec inquiries.
- `mod.rs`: Module exports; exposed in `engines/wega/src/lib.rs` as `pub mod media;`.

### DOM & Host Bridge Integration
- `Dom` (`engines/wega/src/dom.rs`):
  - Added `pub media_engine: RefCell<crate::media::MediaEngine>`.
  - Handled operations: `mediaCanPlayType`, `mediaCreate`, `mediaLoad`, `mediaPlay`, `mediaPause`, `mediaSeek`, `mediaSetVolume`, `mediaSetMuted`, `mediaSetRate`, `mediaSetLoop`, `mediaGetState`, `mediaDestroy`, `mediaSessionEvent`.
- `View` (`engines/wega/src/view.rs`):
  - Added `notify_media_session_action(action)` executing registered JS media session handlers.
- `Main` (`engines/wega/src/main.rs`):
  - Wired `Command::MediaSessionAction` to `view.notify_media_session_action`.

## 3. Verification & Evidence

- Integration test suite `engines/wega/tests/media.rs`:
  - `wav_demuxing_and_metadata` — PASSED (channels=1, rate=16000, bits=16, duration=4.0s)
  - `webm_ebml_demuxing_and_tracks` — PASSED (codec=VP8, 320x180, duration=4.0s)
  - `media_clock_playback_rates_seeking_and_looping` — PASSED (seek, 2x rate, looping wrap-around)
  - `media_can_play_type_reporting` — PASSED ("probably" on WAV/WebM-VP8, "maybe" on MP4/MP3, "" on unknown)
  - `html_video_and_audio_elements_playback` — PASSED (videoWidth=320, videoHeight=180, duration=4.0s, play/pause)
  - `media_session_metadata_state_and_action_dispatch` — PASSED (metadata, actionLog, state transitions)
  - `media_error_reporting_and_fallback_panel` — PASSED (error code=4, data-wega-media-message panel and download link)
  - `media_bounded_buffer_memory_quota` — PASSED (allocation quotas and recovery)
- `cargo test -p wega-protocol` — 38 tests PASSED.
- `cargo test --manifest-path engines/Cargo.toml --lib -p wega-engine` — 29 unit tests PASSED.
- `cargo check --bin wega` — PASSED with 0 errors (Servo compatibility intact).
