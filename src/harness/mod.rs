//! Standalone native integration harness.
use std::time::{Duration, Instant};
#[derive(Default)]
pub struct Harness {
    start: Option<Instant>,
    evaluated: bool,
}
impl Harness {
    pub fn new(options: &wega::options::Options) -> Self {
        Self {
            start: options.test_mode().then(Instant::now),
            evaluated: false,
        }
    }
    pub fn running(&self) -> bool {
        self.start.is_some()
    }
}
impl crate::browser::Browser {
    pub fn check_smoke_test(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Some(start) = self.harness.start else {
            return;
        };
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "Standalone native test timed out"
        );
        let Some(page) = &self.tab().wega else {
            return;
        };
        if page.loading() || page.texture.is_none() {
            return;
        }
        if !self.harness.evaluated {
            page.evaluate(
                "JSON.stringify({title:document.title,url:location.href,dom:!!document.body})",
            );
            self.harness.evaluated = true;
            return;
        }
        let mut state = page.state.lock().unwrap();
        if let Some(result) = state.evaluated.take() {
            let result = result.expect("Standalone JavaScript evaluation");
            assert!(
                result.contains("dom"),
                "Worker returned document state: {result}"
            );
            println!(
                "STANDALONE ENGINE TEST PASSED: native window, real worker pixels, DOM and V8 evaluation"
            );
            drop(state);
            self.harness.start = None;
            event_loop.exit();
        }
    }
}
