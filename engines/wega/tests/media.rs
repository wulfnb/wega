//! Integration tests for Step 20: Media playback, audio demuxing, video stream decoding,
//! MediaSession integration, bounded buffers, and fallback panels.

use std::{sync::Arc, time::Duration};
use url::Url;
use wega_engine::media::{MediaBufferManager, MediaClock, WavInfo, WebmInfo};
use wega_engine::{Engine, Limits, NetworkPolicy, View};
use wega_protocol::media::MediaSessionAction;

fn fixture_path(name: &str) -> String {
    let p1 = format!("tests/fixtures/{name}");
    if std::path::Path::new(&p1).exists() {
        return p1;
    }
    let p2 = format!("../tests/fixtures/{name}");
    if std::path::Path::new(&p2).exists() {
        return p2;
    }
    format!("../../tests/fixtures/{name}")
}

fn test_view(html: &str) -> View {
    let temp = std::env::temp_dir().join(format!("wega-media-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp);
    let engine = Engine::new(
        Limits {
            script_time: Duration::from_secs(5),
            ..Limits::default()
        },
        NetworkPolicy::default(),
        Arc::new(|| {}),
    )
    .unwrap()
    .with_storage_dir(temp);
    let url = Url::parse("https://example.com/index.html").unwrap();
    let mut view = engine.create_view(url).unwrap();
    view.set_html(html).unwrap();
    view.poll();
    view
}

#[test]
fn wav_demuxing_and_metadata() {
    let wav_bytes = std::fs::read(fixture_path("media.wav")).expect("read media.wav");
    let wav = WavInfo::parse(&wav_bytes).expect("parse wav");
    assert_eq!(wav.channels, 1);
    assert_eq!(wav.sample_rate, 16000);
    assert_eq!(wav.bits_per_sample, 16);
    assert!((wav.duration_seconds - 4.0).abs() < 0.05);
    assert_eq!(wav.data_length, 128000);
    assert_eq!(wav.total_samples, 64000);
}

#[test]
fn webm_ebml_demuxing_and_tracks() {
    let webm_bytes = std::fs::read(fixture_path("media.webm")).expect("read media.webm");
    let webm = WebmInfo::parse(&webm_bytes).expect("parse webm");
    assert_eq!(webm.timecode_scale, 1_000_000);
    assert!((webm.duration_seconds - 4.0).abs() < 0.05);

    let video = webm.video_track.expect("video track present");
    assert_eq!(video.codec_id, "V_VP8");
    assert_eq!(video.pixel_width, 320);
    assert_eq!(video.pixel_height, 180);
}

#[test]
fn media_clock_playback_rates_seeking_and_looping() {
    let mut clock = MediaClock::new();
    clock.duration = 4.0;
    assert!(clock.paused);
    assert_eq!(clock.current_time, 0.0);

    // Play
    clock.play().unwrap();
    assert!(!clock.paused);

    // Advance 1.5s
    clock.step_explicit(1.5);
    assert!((clock.current_time - 1.5).abs() < 1e-4);

    // 2x playback rate
    clock.set_playback_rate(2.0);
    clock.step_explicit(1.0);
    assert!((clock.current_time - 3.5).abs() < 1e-4);

    // Seek
    clock.seek(0.8);
    assert!((clock.current_time - 0.8).abs() < 1e-4);

    // Volume and mute
    clock.set_volume(0.5).unwrap();
    assert_eq!(clock.volume, 0.5);
    assert!(clock.set_volume(1.5).is_err());
    clock.set_muted(true);
    assert!(clock.muted);

    // Looping
    clock.set_loop(true);
    clock.seek(3.8);
    clock.set_playback_rate(1.0);
    clock.step_explicit(0.5); // 3.8 + 0.5 = 4.3 -> wraps to 0.3
    assert!((clock.current_time - 0.3).abs() < 1e-4);
    assert!(!clock.ended);

    // Without loop, reaches duration and pauses
    clock.set_loop(false);
    clock.seek(3.9);
    clock.step_explicit(0.5);
    assert_eq!(clock.current_time, 4.0);
    assert!(clock.ended);
    assert!(clock.paused);
}

#[test]
fn media_can_play_type_reporting() {
    let mut view = test_view("<!doctype html><html><body></body></html>");

    let res = view
        .evaluate_json(
            r#"(function() {
                const a = document.createElement("audio");
                const v = document.createElement("video");
                return {
                    wav: a.canPlayType("audio/wav"),
                    xwav: a.canPlayType("audio/x-wav"),
                    webm_vp8: v.canPlayType("video/webm; codecs=\"vp8, opus\""),
                    webm_generic: v.canPlayType("video/webm"),
                    audio_webm_opus: a.canPlayType("audio/webm; codecs=\"opus\""),
                    mp4: v.canPlayType("video/mp4"),
                    mp3: a.canPlayType("audio/mp3"),
                    bogus: v.canPlayType("video/unknown-xyz")
                };
            })()"#,
        )
        .unwrap();

    let v: serde_json::Value = serde_json::from_str(&res).unwrap();
    assert_eq!(v["wav"], "probably");
    assert_eq!(v["xwav"], "probably");
    assert_eq!(v["webm_vp8"], "probably");
    assert_eq!(v["webm_generic"], "maybe");
    assert_eq!(v["audio_webm_opus"], "probably");
    assert_eq!(v["mp4"], "maybe");
    assert_eq!(v["mp3"], "maybe");
    assert_eq!(v["bogus"], "");
}

#[test]
fn html_video_and_audio_elements_playback() {
    let html = r#"<!doctype html>
    <html>
      <body>
        <video id="video" src="media.webm" muted></video>
        <audio id="audio" src="media.wav" muted></audio>
      </body>
    </html>"#;
    let mut view = test_view(html);

    let res = view
        .evaluate_json(
            r#"(function() {
                const video = document.getElementById("video");
                const audio = document.getElementById("audio");

                video.load();
                audio.load();

                video.play();
                audio.play();

                return {
                    videoWidth: video.videoWidth,
                    videoHeight: video.videoHeight,
                    videoDuration: video.duration,
                    audioDuration: audio.duration,
                    videoPaused: video.paused,
                    audioPaused: audio.paused,
                    videoState: video.readyState,
                    audioState: audio.readyState,
                };
            })()"#,
        )
        .unwrap();

    let v: serde_json::Value = serde_json::from_str(&res).unwrap();
    assert_eq!(v["videoWidth"], 320);
    assert_eq!(v["videoHeight"], 180);
    assert_eq!(v["videoDuration"], 4.0);
    assert_eq!(v["audioDuration"], 4.0);
    assert_eq!(v["videoPaused"], false);
    assert_eq!(v["audioPaused"], false);
    assert_eq!(v["videoState"], 4); // HAVE_ENOUGH_DATA
    assert_eq!(v["audioState"], 4);
}

#[test]
fn media_session_metadata_state_and_action_dispatch() {
    let html = r#"<!doctype html>
    <html>
      <body>
        <video id="video" src="media.webm"></video>
        <script>
          window.actionLog = [];
          if (navigator.mediaSession) {
            navigator.mediaSession.metadata = new MediaMetadata({
              title: "Test Track",
              artist: "Wega Artist",
              album: "Wega Album"
            });
            navigator.mediaSession.setActionHandler("pause", (details) => {
              window.actionLog.push("pause");
              navigator.mediaSession.playbackState = "paused";
            });
            navigator.mediaSession.setActionHandler("play", (details) => {
              window.actionLog.push("play");
              navigator.mediaSession.playbackState = "playing";
            });
          }
        </script>
      </body>
    </html>"#;
    let mut view = test_view(html);

    // Verify initial metadata
    let res = view
        .evaluate_json(
            r#"({
                title: navigator.mediaSession.metadata.title,
                artist: navigator.mediaSession.metadata.artist,
                album: navigator.mediaSession.metadata.album,
                playbackState: navigator.mediaSession.playbackState
            })"#,
        )
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&res).unwrap();
    assert_eq!(v["title"], "Test Track");
    assert_eq!(v["artist"], "Wega Artist");
    assert_eq!(v["album"], "Wega Album");

    // Dispatch media session actions through view
    view.notify_media_session_action(MediaSessionAction::Play);
    let log1 = view.evaluate_json("window.actionLog").unwrap();
    assert!(log1.contains("play"));

    view.notify_media_session_action(MediaSessionAction::Pause);
    let log2 = view.evaluate_json("window.actionLog").unwrap();
    assert!(log2.contains("pause"));

    let state = view
        .evaluate_json("navigator.mediaSession.playbackState")
        .unwrap();
    assert_eq!(state, "\"paused\"");
}

#[test]
fn media_error_reporting_and_fallback_panel() {
    let html = r#"<!doctype html>
    <html>
      <body>
        <video id="broken" src="corrupt.xyz"></video>
      </body>
    </html>"#;
    let mut view = test_view(html);

    let res = view
        .evaluate_json(
            r#"(function() {
                const video = document.getElementById("broken");
                video.load();
                return {
                    hasError: !!video.error,
                    errorCode: video.error ? video.error.code : 0,
                    panels: document.querySelectorAll("[data-wega-media-message]").length,
                    links: document.querySelectorAll("[data-wega-media-message] a[download]").length
                };
            })()"#,
        )
        .unwrap();

    let v: serde_json::Value = serde_json::from_str(&res).unwrap();
    assert_eq!(v["hasError"], true);
    assert_eq!(v["errorCode"], 4); // MEDIA_ERR_SRC_NOT_SUPPORTED
    assert_eq!(v["panels"], 1);
    assert_eq!(v["links"], 1);
}

#[test]
fn media_bounded_buffer_memory_quota() {
    let mut buffer = MediaBufferManager::new(1024 * 1024); // 1 MB quota
    assert_eq!(buffer.allocated_bytes(), 0);

    // Allocate 512 KB
    assert!(buffer.allocate(512 * 1024).is_ok());
    assert_eq!(buffer.allocated_bytes(), 512 * 1024);

    // Allocate another 400 KB
    assert!(buffer.allocate(400 * 1024).is_ok());
    assert_eq!(buffer.allocated_bytes(), 912 * 1024);

    // Exceed remaining quota (150 KB > 112 KB remaining)
    assert!(buffer.allocate(150 * 1024).is_err());
    assert_eq!(buffer.allocated_bytes(), 912 * 1024);

    // Deallocate 512 KB
    buffer.deallocate(512 * 1024);
    assert_eq!(buffer.allocated_bytes(), 400 * 1024);

    // Now allocation succeeds
    assert!(buffer.allocate(150 * 1024).is_ok());
    assert_eq!(buffer.allocated_bytes(), 550 * 1024);
}
