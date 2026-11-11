//! Deterministic exports of the browser's own GPU frames for rendering diagnostics.
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

/// The canvas behind transparent document pixels follows the worker color scheme.
pub(crate) fn canvas_color(dark: bool) -> egui::Color32 {
    if dark {
        egui::Color32::from_rgb(18, 18, 18)
    } else {
        egui::Color32::WHITE
    }
}
pub(crate) fn page_image(
    width: u32,
    height: u32,
    pixels: &[u8],
    dark: bool,
) -> Option<image::RgbaImage> {
    let background = canvas_color(dark).to_array();
    let mut image = image::RgbaImage::from_raw(width, height, pixels.to_vec())?;
    for pixel in image.pixels_mut() {
        // Skia's shared frames are premultiplied RGBA.
        let remaining = 255 - u16::from(pixel[3]);
        for channel in 0..3 {
            pixel[channel] = (u16::from(pixel[channel])
                + (u16::from(background[channel]) * remaining + 127) / 255)
                .min(255) as u8;
        }
        pixel[3] = 255;
    }
    Some(image)
}

pub struct Capture {
    pub output: PathBuf,
    start: Instant,
    loaded: Option<Instant>,
    requested_report: bool,
}

impl Capture {
    pub fn new(output: PathBuf) -> Self {
        Self {
            output,
            start: Instant::now(),
            loaded: None,
            requested_report: false,
        }
    }
    pub fn ready_for_frame(&self) -> bool {
        self.loaded
            .is_some_and(|at| at.elapsed() > Duration::from_secs(2))
    }
}

impl crate::browser::Browser {
    pub fn check_capture(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.capture.is_some() {
            self.check_wega_capture(event_loop);
        }
    }
    fn check_wega_capture(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let complete = !self.tab().loading();
        let title = self.tab().title();
        let url = self.tab().url();
        let Some(page) = self.tab().wega.as_ref() else {
            return;
        };
        let shared = page.state.clone();
        let resident_bytes = page.resident_bytes();
        let sender = page.command_sender().expect("Wega capture command channel");
        let mut state = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(capture) = &mut self.capture else {
            return;
        };
        assert!(
            !state.lifecycle.failed(),
            "Wega worker failed during capture: {:?}",
            state.recovery_reason
        );
        assert!(
            capture.start.elapsed() < Duration::from_secs(60),
            "Wega page capture timed out"
        );
        if complete && capture.loaded.is_none() {
            capture.loaded = Some(Instant::now());
        }
        if capture.ready_for_frame() && !capture.requested_report {
            if let Some(parent) = capture.output.parent() {
                std::fs::create_dir_all(parent).expect("Capture directory");
            }
            state.capture = Some(capture.output.clone());
            sender
                .try_send(wega::engine_protocol::Command::Redraw)
                .expect("Request Wega capture");
            capture.requested_report = true;
        }
        if let Some(result) = state.capture_result.take() {
            result.expect("Save Wega page capture");
            let report = serde_json::json!({"engine":"wega","title":title,"url":url,"nodes":state.nodes,"jsHeapBytes":state.js_heap,"workerRssBytes":resident_bytes,"loadSeconds":capture.loaded.map(|at|at.duration_since(capture.start).as_secs_f64()),"compatibility":state.compatibility});
            std::fs::write(
                capture.output.with_extension("json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            )
            .expect("Save Wega capture report");
            println!("CAPTURE PASSED (Wega): {}", capture.output.display());
            self.capture = None;
            event_loop.exit();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn transparent_page_pixels_use_the_same_opaque_canvas_in_exports_and_ui() {
        let pixels = [0, 0, 0, 0, 100, 0, 0, 128, 10, 20, 30, 255];
        let light = super::page_image(3, 1, &pixels, false).unwrap();
        assert_eq!(light.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(light.get_pixel(1, 0).0, [227, 127, 127, 255]);
        let dark = super::page_image(3, 1, &pixels, true).unwrap();
        assert_eq!(dark.get_pixel(0, 0).0, [18, 18, 18, 255]);
        assert_eq!(dark.get_pixel(2, 0).0, [10, 20, 30, 255]);
    }
}
