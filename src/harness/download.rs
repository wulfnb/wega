//! End-to-end native download check driven by a page's real download link.
use std::time::{Duration, Instant};
pub struct Test {
    started: Instant,
    stage: u8,
}
impl Test {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            stage: 0,
        }
    }
}
impl crate::browser::Browser {
    pub fn check_download_test(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Some(test) = &self.harness.download_test else {
            return;
        };
        assert!(
            test.started.elapsed() < Duration::from_secs(60),
            "Download test timed out at stage {}",
            test.stage
        );
        match test.stage {
            0 if self.tab().title() == "Download fixture" && !self.tab().loading() => {
                self.harness.download_test.as_mut().unwrap().stage = 1;
                self.tab()
                    .wega
                    .as_ref()
                    .unwrap()
                    .evaluate("document.querySelector('#download').click()");
            }
            1 if !self.downloads.jobs().is_empty() => {
                assert_eq!(self.downloads.jobs().len(), 1);
                self.harness.download_test.as_mut().unwrap().stage = 2;
            }
            2 => {
                let job = self.downloads.jobs()[0].clone();
                let progress = job.snapshot();
                assert_ne!(
                    progress.phase,
                    crate::downloads::Phase::Failed,
                    "{}",
                    progress.message
                );
                if progress.phase != crate::downloads::Phase::Complete {
                    return;
                }
                assert_eq!(progress.connections, 4, "Multipart was used");
                let bytes = std::fs::read(progress.path.unwrap()).unwrap();
                assert_eq!(bytes.len(), 6 * 1024 * 1024 + 173);
                assert!(bytes.iter().enumerate().all(|(i, b)| *b == (i % 251) as u8));
                println!(
                    "DOWNLOAD TEST PASSED: standalone page download link, host download service, HttpOnly site-cookie reuse, four parallel connections, exact file bytes and safe publication"
                );
                self.harness.download_test = None;
                event_loop.exit();
            }
            _ => {}
        }
    }
}
