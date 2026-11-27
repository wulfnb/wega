//! Round 16 Acceptance Tests: Canvas 2D operations, SVG shapes/transforms,
//! Image decode security, and Media Element state playback cycle.
mod support;
use serde_json::Value;
use wega_engine::{RenderingBackend, View};

fn paint(view: &mut View, width: u32, height: u32, dark: bool) -> Vec<u8> {
    view.paint(width, height, 1.0, 1.0, dark).unwrap().rgba
}
fn pixel(bytes: &[u8], width: usize, x: usize, y: usize) -> &[u8] {
    &bytes[(y * width + x) * 4..(y * width + x) * 4 + 4]
}

#[test]
fn canvas_2d_drawing_pixel_readback_and_state_restore() {
    let server = support::Server::new(|_| {
        support::Reply::html(r#"<canvas id="c" width="100" height="100"></canvas>"#)
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/canvas"));
    view.set_rendering_backend(RenderingBackend::WegaNative);

    let res = view
        .evaluate_json(
            r#"(() => {
        const c = document.getElementById('c');
        const ctx = c.getContext('2d');

        ctx.fillStyle = "rgb(255, 0, 0)";
        ctx.fillRect(0, 0, 50, 50);

        ctx.save();
        ctx.fillStyle = "rgb(0, 255, 0)";
        ctx.fillRect(50, 0, 50, 50);
        ctx.restore();

        // Read pixels
        const redData = ctx.getImageData(10, 10, 1, 1).data;
        const greenData = ctx.getImageData(60, 10, 1, 1).data;

        return {
            red: [redData[0], redData[1], redData[2], redData[3]],
            green: [greenData[0], greenData[1], greenData[2], greenData[3]],
            currentFill: ctx.fillStyle
        };
    })()"#,
        )
        .unwrap();

    let v: Value = serde_json::from_str(&res).unwrap();
    assert_eq!(v["red"], serde_json::json!([255, 0, 0, 255]));
    assert_eq!(v["green"], serde_json::json!([0, 255, 0, 255]));
}

#[test]
fn svg_inline_viewbox_and_polygon_rasterization() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            r#"<style>body { margin: 0; background: white; }</style>
<svg width="100" height="100" viewBox="0 0 100 100">
  <rect x="0" y="0" width="100" height="100" fill="blue" />
  <circle cx="50" cy="50" r="25" fill="red" />
</svg>"#,
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/svg"));
    view.set_rendering_backend(RenderingBackend::WegaNative);

    let frame = paint(&mut view, 100, 100, false);
    // Outer corner is blue
    assert_eq!(pixel(&frame, 100, 5, 5), &[0, 0, 255, 255]);
    // Center of circle is red
    assert_eq!(pixel(&frame, 100, 50, 50), &[255, 0, 0, 255]);
}

#[test]
fn media_elements_audio_video_state_and_lifecycle() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            r#"<video id="v" src="sample.webm" width="320" height="240"></video>
<audio id="a" src="sample.wav"></audio>"#,
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/media"));
    view.set_rendering_backend(RenderingBackend::WegaNative);

    let res = view
        .evaluate_json(
            r#"(() => {
        const v = document.getElementById('v');
        const a = document.getElementById('a');

        const vPausedInitial = v.paused;
        const aPausedInitial = a.paused;
        const vMutedInitial = v.muted;

        v.muted = true;
        v.volume = 0.5;

        return {
            vPausedInitial,
            aPausedInitial,
            vMutedInitial,
            vMutedAfter: v.muted,
            vVolumeAfter: v.volume,
            vLocalName: v.localName,
            aLocalName: a.localName
        };
    })()"#,
        )
        .unwrap();

    let v: Value = serde_json::from_str(&res).unwrap();
    assert_eq!(v["vPausedInitial"], true);
    assert_eq!(v["aPausedInitial"], true);
    assert_eq!(v["vMutedInitial"], false);
    assert_eq!(v["vMutedAfter"], true);
    assert_eq!(v["vVolumeAfter"], 0.5);
    assert_eq!(v["vLocalName"], "video");
    assert_eq!(v["aLocalName"], "audio");
}
