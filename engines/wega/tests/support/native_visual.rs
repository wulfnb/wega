//! Shared differential capture and metrics; no renderer-specific masking.
#![allow(dead_code)]
use serde::Serialize;
use std::path::Path;
#[derive(Clone, Copy)]
pub struct Tolerance {
    pub channel: u8,
    pub mismatch_fraction: f64,
    pub geometry_px: f64,
}
impl Default for Tolerance {
    fn default() -> Self {
        Self {
            channel: 8,
            mismatch_fraction: 0.01,
            geometry_px: 0.5,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Diff {
    pub channel_tolerance: u8,
    pub pixel_threshold_fraction: f64,
    pub geometry_tolerance_px: f64,
    pub differing_pixels: usize,
    pub total_pixels: usize,
    pub mismatch_fraction: f64,
    pub mean_absolute_channel_error: f64,
    pub max_channel_error: u8,
    pub geometry_max_error_px: f64,
    pub within_tolerance: bool,
}
pub fn compare(
    a: &[u8],
    b: &[u8],
    ga: &serde_json::Value,
    gb: &serde_json::Value,
    t: Tolerance,
) -> (Diff, Vec<u8>) {
    assert_eq!(a.len(), b.len());
    let mut differences = 0;
    let mut total_error = 0u64;
    let mut max = 0;
    let mut pixels = Vec::with_capacity(a.len());
    for (a, b) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0.iter()) {
        let mut delta = 0;
        for i in 0..4 {
            let d = a[i].abs_diff(b[i]);
            delta = delta.max(d);
            total_error += d as u64;
            max = max.max(d);
        }
        if delta > t.channel {
            differences += 1;
            pixels.extend_from_slice(&[255, 0, 0, 255]);
        } else {
            let gray = (a[0] as u16 + a[1] as u16 + a[2] as u16) / 3;
            let gray = (gray / 4 + 190).min(255) as u8;
            pixels.extend_from_slice(&[gray, gray, gray, 255]);
        }
    }
    fn geometry(a: &serde_json::Value, b: &serde_json::Value) -> f64 {
        match (a, b) {
            (serde_json::Value::Number(a), serde_json::Value::Number(b)) => {
                (a.as_f64().unwrap() - b.as_f64().unwrap()).abs()
            }
            (serde_json::Value::Array(a), serde_json::Value::Array(b)) if a.len() == b.len() => a
                .iter()
                .zip(b)
                .map(|(a, b)| geometry(a, b))
                .fold(0.0, f64::max),
            _ if a == b => 0.0,
            _ => f64::INFINITY,
        }
    }
    let geometry = geometry(ga, gb);
    let fraction = differences as f64 / (a.len() / 4) as f64;
    (
        Diff {
            channel_tolerance: t.channel,
            pixel_threshold_fraction: t.mismatch_fraction,
            geometry_tolerance_px: t.geometry_px,
            differing_pixels: differences,
            total_pixels: a.len() / 4,
            mismatch_fraction: fraction,
            mean_absolute_channel_error: total_error as f64 / a.len() as f64,
            max_channel_error: max,
            geometry_max_error_px: geometry,
            within_tolerance: fraction <= t.mismatch_fraction && geometry <= t.geometry_px,
        },
        pixels,
    )
}
pub fn png(path: &Path, pixels: &[u8], width: u32, height: u32) {
    let info = skia_safe::ImageInfo::new(
        (width as i32, height as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let image = skia_safe::images::raster_from_data(
        &info,
        skia_safe::Data::new_copy(pixels),
        width as usize * 4,
    )
    .unwrap();
    let encoded = image
        .encode(None, skia_safe::EncodedImageFormat::PNG, None)
        .unwrap();
    std::fs::write(path, encoded.as_bytes()).unwrap();
}
pub fn geometry(view: &mut wega_engine::View) -> serde_json::Value {
    serde_json::from_str(&view.evaluate_json("Array.from(document.querySelectorAll('[data-probe]')).map(e=>{let r=e.getBoundingClientRect();return [e.id,r.x,r.y,r.width,r.height]})").unwrap()).unwrap()
}

pub fn chrome(
    html: &str,
    script: Option<&str>,
    scroll: f64,
    name: &str,
    dir: &Path,
) -> Option<(Vec<u8>, serde_json::Value)> {
    let binary = std::env::var("WEGA_CHROME_BIN").ok()?;
    let profile = tempfile::tempdir().unwrap();
    let source = format!(
        "{html}<script>addEventListener('load',()=>{{{};window.scrollTo(0,{});document.documentElement.setAttribute('data-wega-geometry',btoa(JSON.stringify(Array.from(document.querySelectorAll('[data-probe]')).map(e=>{{let r=e.getBoundingClientRect();return [e.id,r.x,r.y,r.width,r.height]}}))))}})</script>",
        script.unwrap_or(""),
        scroll
    );
    let file = dir.join(format!("{name}-chrome.html"));
    std::fs::write(&file, source).unwrap();
    let screenshot = dir.join(format!("{name}-chrome.png"));
    let mut child = std::process::Command::new("node")
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/support/chrome_capture.mjs"),
        )
        .args([
            binary.as_str(),
            profile.path().to_str().unwrap(),
            file.to_str().unwrap(),
            screenshot.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            panic!("bounded Chrome capture deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let geometry = serde_json::from_slice(&output.stdout).unwrap();
    let image = skia_safe::images::deferred_from_encoded_data(
        skia_safe::Data::new_copy(&std::fs::read(screenshot).unwrap()),
        None,
    )
    .unwrap();
    assert_eq!((image.width(), image.height()), (600, 420));
    let info = skia_safe::ImageInfo::new(
        (600, 420),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut pixels = vec![0u8; 600 * 420 * 4];
    assert!(image.read_pixels(
        &info,
        &mut pixels,
        600 * 4,
        (0, 0),
        skia_safe::image::CachingHint::Disallow
    ));
    Some((pixels, geometry))
}
