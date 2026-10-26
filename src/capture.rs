//! Deterministic exports of the browser's own GPU frames for rendering diagnostics.
use servo::RenderingContext;
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

pub struct Capture {
    pub output: PathBuf,
    start: Instant,
    loaded: Option<Instant>,
    requested_report: bool,
    report: Rc<RefCell<Option<String>>>,
}

impl Capture {
    pub fn new(output: PathBuf) -> Self {
        Self {
            output,
            start: Instant::now(),
            loaded: None,
            requested_report: false,
            report: Rc::new(RefCell::new(None)),
        }
    }
    pub fn ready_for_frame(&self) -> bool {
        self.loaded
            .is_some_and(|at| at.elapsed() > Duration::from_secs(2))
    }
}

impl crate::browser::Browser {
    pub fn check_capture(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.is_wega() {
            self.check_wega_capture(event_loop);
            return;
        }
        let complete = self.view().load_status() == servo::LoadStatus::Complete;
        let view = self.view().clone();
        let Some(capture) = &mut self.capture else {
            return;
        };
        assert!(
            capture.start.elapsed() < Duration::from_secs(60),
            "Page capture timed out"
        );
        if complete && capture.loaded.is_none() {
            capture.loaded = Some(Instant::now());
        }
        if capture.ready_for_frame() && !capture.requested_report {
            capture.requested_report = true;
            let report = capture.report.clone();
            view.evaluate_javascript(
                "JSON.stringify({title:document.title,url:location.href,textGradients:Array.from(document.querySelectorAll('.bg-clip-text')).map(e=>({text:e.textContent,color:getComputedStyle(e).color,background:getComputedStyle(e).backgroundImage}))},null,2)",
                move |result| *report.borrow_mut() = Some(match result { Ok(servo::JSValue::String(text)) => text, other => format!("{other:?}") }),
            );
        }
        let Some(loaded) = capture.loaded else {
            return;
        };
        if loaded.elapsed() < Duration::from_secs(3) {
            return;
        }
        let Some(report) = capture.report.borrow_mut().take() else {
            return;
        };
        let load_seconds = loaded.duration_since(capture.start).as_secs_f64();
        let output = capture.output.clone();
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).expect("Capture directory");
        }
        let surface = &self.live().surface;
        surface.make_current().expect("Capture GPU context");
        self.view().paint();
        let size = surface.size();
        let rect = servo::DeviceIntRect::from_size(servo::DeviceIntSize::new(
            size.width as i32,
            size.height as i32,
        ));
        surface
            .read_to_image(rect)
            .expect("Capture pixels")
            .save(&output)
            .expect("Save page capture");
        let mut report: serde_json::Value = serde_json::from_str(&report).expect("Capture JSON");
        report["engine"] = "servo".into();
        report["loadSeconds"] = load_seconds.into();
        std::fs::write(
            output.with_extension("json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .expect("Save capture report");
        println!("CAPTURE PASSED: {}", output.display());
        self.capture = None;
        event_loop.exit();
    }
    fn check_wega_capture(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let complete = !self.tab().loading();
        let title = self.tab().title();
        let url = self.tab().url();
        let Some(page) = self.tab().wega.as_ref() else {
            return;
        };
        let shared = page.state.clone();
        let sender = page.command_sender().expect("Wega capture command channel");
        let mut state = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(capture) = &mut self.capture else {
            return;
        };
        assert!(!state.failed, "Wega worker failed during capture");
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
            let report = serde_json::json!({"engine":"wega","title":title,"url":url,"nodes":state.nodes,"jsHeapBytes":state.js_heap,"workerRssBytes":state.rss_bytes,"servoInitialized":self.servo.initialized(),"loadSeconds":capture.loaded.map(|at|at.duration_since(capture.start).as_secs_f64()),"compatibility":state.compatibility});
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
