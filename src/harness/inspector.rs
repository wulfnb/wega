//! End-to-end native check of the inspector: the real window, a real `Page`, a real
//! `wega-engine` process and the real status panel path. Includes a real stalled engine.
use crate::devtools_console::ConsoleLine;
use std::time::{Duration, Instant};
use wega::engine_protocol::inspector::InspectorSessionId;

pub struct Test {
    started: Instant,
    stage: u8,
    first_document: u64,
    first_session: Option<InspectorSessionId>,
    stalled_at: Option<Instant>,
    unresponsive_after: Option<Duration>,
    resumed_at: Option<Instant>,
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
            unresponsive_after: None,
            resumed_at: None,
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
                // Output and an uncaught error produced while DevTools is closed.
                page.evaluate(
                    "console.log('before DevTools', {n: 1}); setTimeout(() => { throw new Error('before open') }, 0)",
                );
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
                next(self, 10);
            }
            10 => {
                self.window.request_redraw();
                let has = |browser: &Self, needle: &str| {
                    browser.devtools.console.lines().any(|line| {
                        matches!(line, ConsoleLine::Message(m) if m.text.contains(needle) || m.exception.as_ref().is_some_and(|e| e.message.contains(needle)))
                    })
                };
                // Retained from before the panel opened, delivered by the engine on enable.
                if !has(self, "before DevTools") || !has(self, "before open") {
                    return;
                }
                let exception = self.devtools.console.lines().find_map(|line| match line {
                    ConsoleLine::Message(m) => m.exception.clone(),
                    _ => None,
                });
                let exception = exception.expect("uncaught error retained");
                assert_eq!(exception.name, "Error");
                assert!(
                    exception.source.is_some(),
                    "source location for the exception"
                );
                assert!(self.devtools.console.enabled);
                page.evaluate(
                    "console.log('live message', {a: [1, 2]}); console.warn('live warning')",
                );
                // Nothing may run yet: opening the tools granted nothing.
                assert!(self.devtools.console.grant.is_none());
                self.devtools_run_command("1+1");
                assert!(
                    matches!(self.devtools.console.lines().last(), Some(ConsoleLine::Notice(t)) if t.contains("not enabled")),
                    "evaluation must be refused without a grant"
                );
                next(self, 11);
            }
            11 => {
                self.window.request_redraw();
                if !self.devtools.console.lines().any(|line| {
                    matches!(line, ConsoleLine::Message(m) if m.text.contains("live warning"))
                }) {
                    return;
                }
                let live = self.devtools.console.lines().find_map(|line| match line {
                    ConsoleLine::Message(m) if m.text.starts_with("live message") => {
                        Some(m.clone())
                    }
                    _ => None,
                });
                let live = live.expect("live message");
                assert_eq!(live.args.len(), 2);
                assert_eq!(live.args[1].props[0].key, "a");
                // The user's explicit action (the panel's button).
                self.devtools_enable_evaluation(false);
                next(self, 12);
            }
            12 => {
                self.window.request_redraw();
                let Some(document) = self.devtools_document() else {
                    return;
                };
                if self.devtools.console.evaluation_enabled(document).is_none() {
                    return;
                }
                self.devtools_run_command("6*7");
                next(self, 13);
            }
            13 => {
                self.window.request_redraw();
                if !self
                    .devtools
                    .console
                    .lines()
                    .any(|line| matches!(line, ConsoleLine::Result(p) if p.text == "42"))
                {
                    return;
                }
                self.devtools_run_command("(function failing(){ null.x })()");
                next(self, 14);
            }
            14 => {
                self.window.request_redraw();
                let failure = self.devtools.console.lines().find_map(|line| match line {
                    ConsoleLine::Failure(details) => Some(details.clone()),
                    _ => None,
                });
                let Some(failure) = failure else {
                    return;
                };
                assert_eq!(failure.name, "TypeError");
                assert!(failure.stack.as_deref().unwrap_or("").contains("failing"));
                self.tools.elements_tab = true;
                self.tools.console_tab = false;
                self.devtools.elements_active = true;
                next(self, 15);
            }
            15 => {
                self.window.request_redraw();
                // Expand through real protocol pages, using no page JS for inspection.
                for row in self.devtools.elements.rows.values_mut() {
                    row.expanded = true;
                }
                let selected = self
                    .devtools
                    .elements
                    .rows
                    .values()
                    .find(|r| {
                        r.node
                            .attributes
                            .iter()
                            .any(|a| a.name == "id" && a.value == "counter")
                    })
                    .map(|r| r.node.id);
                let Some(selected) = selected else { return };
                self.devtools.elements.select(selected);
                self.apply(wega::application::Command::Dock(
                    crate::tool_window::Dock::Window,
                ));
                next(self, 16);
            }
            16 => {
                self.window.request_redraw();
                let Some(window) = &self.tool_window else {
                    return;
                };
                if window.frames < 2 || self.devtools.elements.pending.is_some() {
                    return;
                }
                assert!(self.devtools.elements.selected.is_some());
                assert!(self.devtools.elements.rows.len() > 5);
                assert_eq!(self.devtools.elements.document, self.devtools_document());
                let target = self.devtools.target_tab();
                // A detached window stays pinned while the browser selects another tab.
                self.new_tab();
                self.devtools_tick(&egui::Context::default());
                assert_eq!(self.devtools.target_tab(), target);
                self.select_tab(0);
                self.apply(wega::application::Command::Dock(
                    crate::tool_window::Dock::Bottom,
                ));
                println!(
                    "inspector-test: Elements expanded, selected, drawn in detached OS window and pinned across a real tab switch"
                );
                next(self, 17);
            }
            17 => {
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
                    assert!(
                        self.devtools.elements.selected.is_none(),
                        "selection survived document replacement"
                    );
                    // Navigation revoked evaluation authority and started a fresh history.
                    let document = self.devtools_document().unwrap();
                    assert!(
                        self.devtools.console.evaluation_enabled(document).is_none(),
                        "the grant survived navigation"
                    );
                    self.devtools_run_command("globalThis.afterNavigation = 1");
                    assert!(
                        matches!(self.devtools.console.lines().last(), Some(ConsoleLine::Notice(t)) if t.contains("not enabled")),
                        "evaluation must be refused after navigation"
                    );
                    assert!(!self.devtools.console.lines().any(|line| {
                        matches!(line, ConsoleLine::Message(m) if m.text.contains("before DevTools"))
                    }), "old document output must be gone");
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
                let test = self.harness.inspector_test.as_mut().unwrap();
                test.unresponsive_after = test.stalled_at.map(|at| at.elapsed());
                test.resumed_at = Some(Instant::now());
                next(self, 6);
            }
            6 => {
                self.window.request_redraw();
                if self.devtools_snapshot().is_none() {
                    return;
                }
                let test = self.harness.inspector_test.as_ref().unwrap();
                println!(
                    "INSPECTOR TEST PASSED: attach, status, console and evaluation; Elements tree expansion and selection, detached OS window and pinned real tab switch; navigation clears selection and authority; detach, reattach and stalled-worker recovery"
                );
                println!(
                    "inspector-test: stalled worker reported NotResponding after {} ms with {} UI polls; recovered {} ms after SIGCONT",
                    test.unresponsive_after.map_or(0, |d| d.as_millis()),
                    test.frames_while_stalled,
                    test.resumed_at.map_or(0, |at| at.elapsed().as_millis())
                );
                self.harness.inspector_test = None;
                event_loop.exit();
            }
            _ => {}
        }
    }
}
