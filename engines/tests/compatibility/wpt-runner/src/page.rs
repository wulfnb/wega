//! Loading one WPT page into a Wega `View` and waiting until it is ready, the
//! way `examples/render_url.rs` drives the engine.
use std::time::{Duration, Instant};
use wega_engine::{Engine, Url, View};

pub const WIDTH: u32 = 800;
pub const HEIGHT: u32 = 600;

pub fn load(engine: &Engine, url: &Url, deadline: Instant) -> Result<View, String> {
    let mut view = engine.create_view(url.clone())?;
    view.poll();
    while view.loading {
        if Instant::now() > deadline {
            return Err("TIMEOUT: page load".into());
        }
        view.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(view)
}

/// Pump the event loop, painting when the page asks for a frame so
/// `requestAnimationFrame` callbacks and resource wake-ups make progress.
pub fn pump(view: &mut View) -> Result<(), String> {
    view.poll();
    if view.paint_pending() {
        view.paint(WIDTH, HEIGHT, 1.0, 1.0, false)?;
    }
    Ok(())
}

/// The reftest screenshot: wait for `reftest-wait` to be removed, then paint
/// twice so late images and fonts land in the frame.
pub fn screenshot(engine: &Engine, url: &Url) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut view = load(engine, url, deadline)?;
    loop {
        let waiting = view.evaluate_json(
            "!!(document.documentElement && document.documentElement.classList.contains('reftest-wait'))",
        )?;
        if waiting != "true" {
            break;
        }
        if Instant::now() > deadline {
            return Err("TIMEOUT: reftest-wait".into());
        }
        pump(&mut view)?;
        std::thread::sleep(Duration::from_millis(5));
    }
    view.paint(WIDTH, HEIGHT, 1.0, 1.0, false)?;
    let settle = Instant::now() + Duration::from_millis(60);
    while Instant::now() < settle {
        view.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(view.paint(WIDTH, HEIGHT, 1.0, 1.0, false)?.rgba)
}

pub fn save_png(path: &std::path::Path, rgba: &[u8]) {
    let info = skia_safe::ImageInfo::new(
        (WIDTH as i32, HEIGHT as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let Some(image) = skia_safe::images::raster_from_data(
        &info,
        skia_safe::Data::new_copy(rgba),
        WIDTH as usize * 4,
    ) else {
        return;
    };
    if let Some(png) = image.encode(None, skia_safe::EncodedImageFormat::PNG, None) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, png.as_bytes());
    }
}
