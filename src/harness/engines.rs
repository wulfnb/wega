//! Real native chrome, worker rendering, click routing and engine switching.
use crate::browser::Browser;
use std::time::{Duration, Instant};
use wega::{engine_choice::EngineKind, engine_protocol::Command};
pub struct Test {
    stage: u8,
    start: Instant,
    first_pid: u32,
    fixture_url: String,
}
impl Test {
    pub fn new() -> Self {
        Self {
            stage: 0,
            start: Instant::now(),
            first_pid: 0,
            fixture_url: String::new(),
        }
    }
}
impl Browser {
    fn check_new_tab_render_contract(&mut self) {
        let original = self.session().active();
        let original_key = self.tab().key;
        let worker = self.tab().wega.as_ref().unwrap().pid();
        let layout = self.profile().layout;
        for layout in [
            crate::profile::TabLayout::Side,
            crate::profile::TabLayout::Top,
        ] {
            self.apply(wega::application::Command::Layout(layout));
            let count = self.session().tabs().len();
            // redraw() owns the GUI outside Browser while widgets run. The old
            // focus_content unwrap crashed only on real UI clicks, not shortcuts
            // or AccessKit contracts which left the GUI inside Browser.
            let gui = self.gui.take().unwrap();
            self.coverage_click(&["New tab"], |browser, ui| browser.ui(ui));
            self.gui = Some(gui);
            assert_eq!(self.session().tabs().len(), count + 1);
            assert_eq!(self.tab().url(), "wega://newtab");
            assert!(!self.is_wega());
            self.handle_event(winit::event::WindowEvent::RedrawRequested);
            assert!(!self.chrome.clear_address_focus);
            assert!(self.chrome.page_focused);
            self.close_tab(self.session().active());
            self.select_tab(original);
            assert_eq!(self.tab().key, original_key);
            assert_eq!(self.tab().wega.as_ref().unwrap().pid(), worker);
        }
        self.apply(wega::application::Command::Layout(layout));
        println!(
            "NEW TAB RENDER TEST PASSED: sidebar/top click during GUI ownership, lazy Servo and preserved Wega worker"
        );
    }
    pub fn check_engine_test(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Some(test) = &self.harness.engine_test else {
            return;
        };
        assert!(
            test.start.elapsed() < Duration::from_secs(65),
            "Engine UI test timed out at stage {}: {}",
            test.stage,
            self.tab().title()
        );
        match test.stage {
            0 => {
                if self.tab().title() != "Wega engine ready"
                    || self.tab().loading()
                    || self.tab().wega.as_ref().is_none_or(|p| p.texture.is_none())
                {
                    return;
                }
                assert!(self.is_wega());
                assert!(
                    !self.servo.initialized(),
                    "A Wega-only launch must not initialize Servo"
                );
                self.check_new_tab_render_contract();
                let page = self.tab().wega.as_ref().unwrap();
                self.harness.engine_test.as_mut().unwrap().first_pid = page.pid();
                self.harness.engine_test.as_mut().unwrap().fixture_url = self.tab().url();
                self.harness.engine_test.as_mut().unwrap().stage = 17;
                // The AccessKit contract renders at a synthetic size and switches
                // layouts/tabs. Restore the actual window viewport before asking
                // the worker for coordinates; a queued resize is not a layout ack.
                self.handle_event(winit::event::WindowEvent::RedrawRequested);
            }
            17 => {
                let page = self.tab().wega.as_ref().unwrap();
                let Some((width, height, ..)) = page.viewport else {
                    return;
                };
                if page
                    .texture
                    .as_ref()
                    .is_none_or(|texture| texture.size() != [width as usize, height as usize])
                {
                    return;
                }
                // Pixels at the restored dimensions prove the worker has laid
                // out this viewport, rather than the preceding synthetic one.
                page.send(Command::Evaluate(
                    "document.getElementById('counter').getBoundingClientRect()".into(),
                ))
                .unwrap();
                self.harness.engine_test.as_mut().unwrap().stage = 1;
            }
            1 => {
                let result = self
                    .tab()
                    .wega
                    .as_ref()
                    .unwrap()
                    .state
                    .lock()
                    .unwrap()
                    .evaluated
                    .take();
                let Some(result) = result else { return };
                let rect: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
                let x = rect["x"].as_f64().unwrap() + rect["width"].as_f64().unwrap() / 2.0;
                let y = rect["y"].as_f64().unwrap() + rect["height"].as_f64().unwrap() / 2.0;
                println!(
                    "ENGINE CLICK: button={rect}, page={:?}, viewport={:?}",
                    self.chrome.page_rect,
                    self.tab().wega.as_ref().unwrap().viewport
                );
                self.smoke_click(x, y);
                // Deliver the queued move/down/up in this frame, before any
                // later native events or layout changes can alter the target.
                self.handle_event(winit::event::WindowEvent::RedrawRequested);
                self.harness.engine_test.as_mut().unwrap().stage = 2;
            }
            2 => {
                if self.tab().title() != "Counter clicked: 1" {
                    return;
                }
                self.tab().wega.as_ref().unwrap().send(Command::Evaluate("globalThis.nativeEdit=document.createElement('input');nativeEdit.id='native-edit';document.body.append(nativeEdit);nativeEdit.focus();nativeEdit.addEventListener('input',()=>document.title='Native input: '+nativeEdit.value);'focused'".into())).unwrap();
                self.harness.engine_test.as_mut().unwrap().stage = 12;
            }
            12 => {
                let result = self
                    .tab()
                    .wega
                    .as_ref()
                    .unwrap()
                    .state
                    .lock()
                    .unwrap()
                    .evaluated
                    .take();
                let Some(result) = result else {
                    return;
                };
                assert_eq!(result.unwrap(), "\"focused\"");
                self.harness.engine_test.as_mut().unwrap().stage = 13;
                self.chrome.page_focused = true;
                self.handle_event(winit::event::WindowEvent::Ime(winit::event::Ime::Commit(
                    "Hello 文".into(),
                )));
            }
            13 => {
                if self.tab().title() != "Native input: Hello 文" {
                    return;
                }
                self.tab()
                    .wega
                    .as_ref()
                    .unwrap()
                    .send(Command::Evaluate(
                        "nativeEdit.remove();document.title='Counter clicked: 1'".into(),
                    ))
                    .unwrap();
                self.harness.engine_test.as_mut().unwrap().stage = 14;
            }
            14 => {
                if self.tab().title() != "Counter clicked: 1" {
                    return;
                }
                let address = self.tab().url();
                self.navigate("about:blank");
                self.coverage_click(&["Keep working"], |b, ui| b.ui(ui));
                assert_eq!(self.tab().url(), address);
                assert_eq!(self.chrome.address, address);
                assert_eq!(self.tab().title(), "Counter clicked: 1");
                self.coverage_click(&["Engine: Wega (experimental)", "Servo"], |b, ui| b.ui(ui));
                assert!(self.is_wega(), "Switch waits for explicit consent");
                self.coverage_click(&["Keep working"], |b, ui| b.ui(ui));
                assert!(self.is_wega());
                assert_eq!(
                    self.tab().title(),
                    "Counter clicked: 1",
                    "Cancellation preserves live JavaScript state"
                );
                // Preferences and live network policy must preserve the existing DOM.
                let mut policy = self.profile().policy.clone();
                policy.wega_rollout = false;
                self.apply(wega::application::Command::Policy(policy));
                assert_eq!(
                    self.tab().wega.as_ref().unwrap().pid(),
                    self.harness.engine_test.as_ref().unwrap().first_pid
                );
                assert_eq!(self.tab().title(), "Counter clicked: 1");
                // The real missing-API reporter must offer fallback without silently
                // replacing an edited page. Cancelling it must not prompt repeatedly.
                self.tab()
                    .wega
                    .as_ref()
                    .unwrap()
                    .send(Command::Evaluate(
                        "setTimeout(()=>document.body.unimplementedFallbackFixtureAPI(),0);'scheduled'".into(),
                    ))
                    .unwrap();
                self.harness.engine_test.as_mut().unwrap().stage = 15;
            }
            15 => {
                if !matches!(
                    self.destructive_action,
                    Some(crate::page_safety::Action::Fallback(_))
                ) {
                    return;
                }
                self.coverage_click(&["Keep working"], |b, ui| b.ui(ui));
                assert!(self.is_wega());
                assert_eq!(self.tab().title(), "Counter clicked: 1");
                self.pump_wega();
                assert!(self.destructive_action.is_none());
                self.switch_engine(EngineKind::Servo);
                self.coverage_click(&["Switch engine"], |b, ui| b.ui(ui));
                assert_eq!(self.tab().engine, EngineKind::Servo);
                assert!(self.servo.initialized());
                self.harness.engine_test.as_mut().unwrap().stage = 3;
            }
            3 => {
                if self.tab().loading() || self.tab().title() != "Wega engine ready" {
                    return;
                }
                self.coverage_click(&["Engine: Servo", "Wega (experimental)"], |b, ui| b.ui(ui));
                self.coverage_click(&["Switch engine"], |b, ui| b.ui(ui));
                assert!(self.is_wega());
                self.harness.engine_test.as_mut().unwrap().stage = 4;
            }
            4 => {
                if self.tab().loading() || self.tab().title() != "Wega engine ready" {
                    return;
                }
                let pid = self.tab().wega.as_ref().unwrap().pid();
                assert_ne!(pid, self.harness.engine_test.as_ref().unwrap().first_pid);
                self.save();
                assert_eq!(self.profile().engines[0], EngineKind::Wega);
                self.add_tab("about:blank");
                assert!(
                    self.session().tabs()[0]
                        .wega
                        .as_ref()
                        .unwrap()
                        .texture
                        .is_none(),
                    "Hidden Wega tabs release frontend textures"
                );
                assert!(!self.discard_tab(0));
                assert!(self.session().tabs()[0].loaded());
                self.coverage_click(&["Keep working"], |b, ui| b.ui(ui));
                assert!(self.session().tabs()[0].loaded());
                let protected_key = self.session().tabs()[0].key;
                self.discard_tab(0);
                self.move_tab(0, 1);
                self.coverage_click(&["Unload tab"], |b, ui| b.ui(ui));
                assert_eq!(self.session().tabs()[1].key, protected_key);
                assert!(
                    !self.session().tabs()[1].loaded(),
                    "Consent follows tab identity after reordering"
                );
                self.move_tab(1, 0);
                assert!(!self.session().tabs()[0].loaded());
                self.select_tab(0);
                assert!(self.is_wega());
                assert_ne!(self.tab().wega.as_ref().unwrap().pid(), pid);
                self.harness.engine_test.as_mut().unwrap().stage = 5;
            }
            5 => {
                if self.tab().loading() || self.tab().title() != "Wega engine ready" {
                    return;
                }
                self.close_tab(0);
                assert_eq!(self.session().tabs().len(), 2);
                self.coverage_click(&["Keep working"], |b, ui| b.ui(ui));
                assert_eq!(self.session().tabs().len(), 2);
                self.request_quit();
                self.coverage_click(&["Keep working"], |b, ui| b.ui(ui));
                assert!(!self.quit_requested);
                self.close_tab(0);
                self.coverage_click(&["Close tab"], |b, ui| b.ui(ui));
                assert_eq!(self.tab().engine, EngineKind::Servo);
                self.save();
                assert_eq!(self.profile().engines, vec![EngineKind::Servo]);
                let url = url::Url::parse(&self.harness.engine_test.as_ref().unwrap().fixture_url)
                    .unwrap()
                    .join("wega-fallback.html")
                    .unwrap();
                let mut policy = self.profile().policy.clone();
                policy.site_engines.insert(
                    wega::engine_choice::site_key(&url).unwrap(),
                    EngineKind::Wega,
                );
                self.apply(wega::application::Command::Policy(policy));
                assert!(self.replace_engine_confirmed(EngineKind::Wega, url, false));
                self.harness.engine_test.as_mut().unwrap().stage = 16;
            }
            16 => {
                if self.is_wega()
                    || self.tab().loading()
                    || self.tab().title() != "Wega fallback fixture"
                {
                    return;
                }
                assert!(
                    self.destructive_action.is_none(),
                    "An unedited failed initial page can fall back automatically"
                );
                let url = self.current_url().unwrap();
                let key = wega::engine_choice::site_key(&url).unwrap();
                assert_eq!(self.tab().fallback_origin.as_deref(), Some(key.as_str()));
                assert_eq!(
                    self.profile().policy.site_engines.get(&key),
                    Some(&EngineKind::Wega),
                    "Fallback preserves the explicit preference"
                );
                self.dispatch_navigation(wega::navigation::Command::Load(url.to_string()))
                    .unwrap();
                assert!(
                    !self.is_wega(),
                    "Same-site navigation must not create a fallback loop"
                );
                println!(
                    "DUAL ENGINE TEST PASSED: lazy Servo, real Wega pixels, native JS click/IME input, footer selection both directions, persisted choice, policy updates, freezing, discard/reload, edited fallback cancellation, automatic fallback and loop prevention"
                );
                self.harness.engine_test = None;
                event_loop.exit();
            }
            _ => unreachable!(),
        }
    }
}
