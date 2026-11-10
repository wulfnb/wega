//! Deterministic exports of the browser's own GPU frames for rendering diagnostics.
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

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
