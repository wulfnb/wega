//! End-to-end native check of the inspector: the real window, a real `Page`, a real
//! `wega-engine` process and the real status panel path. Includes a real stalled engine.
use std::time::{Duration, Instant};
use wega::engine_protocol::inspector::InspectorSessionId;

pub struct Test {
    started: Instant,
    stage: u8,
    first_document: u64,
    first_session: Option<InspectorSessionId>,
    stalled_at: Option<Instant>,
    frames_while_stalled: u64,
    worker: u32,
}

impl Test {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            stage: 0,
            first_document: 0,
            first_session: None,
            stalled_at: None,
            frames_while_stalled: 0,
            worker: 0,
        }
    }
}

/// Stop or resume the worker the test itself spawned. Uses the system `kill` so the harness
/// adds no `unsafe` to the tree.
fn signal(pid: u32, signal: &str) {
    let status = std::process::Command::new("kill")
        .args([signal, &pid.to_string()])
        .status()
        .expect("kill is available on this platform");
    assert!(status.success(), "signalling the worker failed");
}

impl crate::browser::Browser {
    fn inspector_live(&self) -> Option<InspectorSessionId> {
        let page = self.tab().wega.as_ref()?;
        page.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .inspector
            .as_ref()?
            .registry()
            .live()
    }

    pub fn check_inspector_test(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Some(test) = &self.harness.inspector_test else {
            return;
        };
        assert!(
            test.started.elapsed() < Duration::from_secs(90),
            "Inspector test timed out at stage {}",
            test.stage
        );
        let stage = test.stage;
        let Some(page) = self.tab().wega.as_ref() else {
            return;
        };
        let current_document = {
            let state = page
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (state.requests.document().0, state.requests.revision().0)
        };
        // Invariant at every step: any snapshot the panel would show (the same accessor the
        // renderer uses) is of the live page's current document and of this tab.
        if let Some(snapshot) = self.devtools_snapshot() {
            assert_eq!(
                snapshot.target.tab.0,
                self.tab().key,
                "snapshot of another tab"
            );
            assert_eq!(
                (snapshot.document.document.0, snapshot.document.revision.0),
                current_document,
                "stale document snapshot displayed"
            );
        }
        let next = |browser: &mut Self, stage: u8| {
            println!(
                "inspector-test: stage {stage} after {} ms (view {:?})",
                browser
                    .harness
                    .inspector_test
                    .as_ref()
                    .unwrap()
                    .started
                    .elapsed()
                    .as_millis(),
                browser.devtools.state
            );
            browser.harness.inspector_test.as_mut().unwrap().stage = stage;
        };
        match stage {
            // Closed: no session exists while normal browsing proceeds.
            0 if !self.tab().loading() && page.texture.is_some() => {
                assert!(self.inspector_live().is_none(), "session without DevTools");
                assert!(!self.devtools.is_open());
                self.chrome.show_console = true;
                self.window.request_redraw();
                next(self, 1);
            }
            1 => {
                self.window.request_redraw();
                let Some(snapshot) = self.devtools_snapshot().cloned() else {
                    return;
                };
                assert_eq!(snapshot.url, self.tab().url());
                assert!(snapshot.title.contains("interaction"), "{}", snapshot.title);
                assert!(snapshot.nodes > 0, "engine node count");
                assert!(snapshot.viewport.is_some(), "viewport reported");
                let session = self.devtools.session().expect("attached session");
                assert_eq!(self.inspector_live(), Some(session));
                let test = self.harness.inspector_test.as_mut().unwrap();
                test.first_document = snapshot.document.document.0;
                test.first_session = Some(session);
                let mut url = self.tab().url().to_string();
                url.push_str("?second");
                if let Ok(url) = url::Url::parse(&url) {
                    let _ = self.dispatch_navigation_confirmed(wega::navigation::Command::Load(
                        url.to_string(),
                    ));
                }
                next(self, 2);
            }
            2 => {
                self.window.request_redraw();
                let first = self.harness.inspector_test.as_ref().unwrap().first_document;
                let Some(snapshot) = self.devtools_snapshot().cloned() else {
                    return;
                };
                if snapshot.document.document.0 > first {
                    assert!(snapshot.url.ends_with("?second"), "{}", snapshot.url);
                    next(self, 3);
                }
            }
            // Close: the session is revoked in the page's authority and the page keeps running.
            3 => {
                let pid = page.pid();
                self.close_developer_tools();
                assert!(self.devtools.session().is_none());
                assert!(
                    self.inspector_live().is_none(),
                    "session survived the panel"
                );
                assert!(self.devtools.snapshot().is_none());
                assert!(
                    self.tab().wega.is_some(),
                    "closing the panel closed the page"
                );
                assert_eq!(self.tab().wega.as_ref().unwrap().pid(), pid);
                self.harness.inspector_test.as_mut().unwrap().worker = pid;
                self.chrome.show_console = true;
                self.window.request_redraw();
                next(self, 4);
            }
            4 => {
                self.window.request_redraw();
                if self.devtools_snapshot().is_none() {
                    return;
                }
                let session = self.devtools.session().unwrap();
                assert_ne!(
                    Some(session),
                    self.harness.inspector_test.as_ref().unwrap().first_session,
                    "a session id was reused"
                );
                // A real stalled engine: stop the worker process.
                signal(
                    self.harness.inspector_test.as_ref().unwrap().worker,
                    "-STOP",
                );
                let test = self.harness.inspector_test.as_mut().unwrap();
                test.stalled_at = Some(Instant::now());
                next(self, 5);
            }
            5 => {
                self.window.request_redraw();
                let test = self.harness.inspector_test.as_mut().unwrap();
                test.frames_while_stalled += 1;
                if !matches!(
                    self.devtools.state,
                    crate::devtools_session::ViewState::NotResponding
                ) {
                    return;
                }
                assert!(
                    self.devtools.snapshot().is_none(),
                    "data shown for a hung page"
                );
                // The UI loop is alive (this very function keeps running) and the page is intact.
                signal(
                    self.harness.inspector_test.as_ref().unwrap().worker,
                    "-CONT",
                );
                next(self, 6);
            }
            6 => {
                self.window.request_redraw();
                if self.devtools_snapshot().is_none() {
                    return;
                }
                let test = self.harness.inspector_test.as_ref().unwrap();
                println!(
                    "INSPECTOR TEST PASSED: attach, real status snapshot, navigation without stale data, detach revokes, reattach with a new session, stalled engine reported without freezing the UI ({} UI polls in {} ms)",
                    test.frames_while_stalled,
                    test.stalled_at.map_or(0, |at| at.elapsed().as_millis())
                );
                self.harness.inspector_test = None;
                event_loop.exit();
            }
            _ => {}
        }
    }
}
