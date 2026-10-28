//! Confirm destructive actions when the document reports edits, or cannot be queried.
use crate::browser::Browser;
use wega::engine_choice::EngineKind;

#[derive(Clone)]
pub(crate) enum Action {
    Close(u64),
    Discard(u64),
    Switch(u64, EngineKind),
    Fallback(u64),
    Navigate(u64, wega::navigation::Command),
    Quit,
}

pub(crate) struct Check {
    action: Action,
    request: u64,
    waiting: std::collections::HashSet<u64>,
    dirty: bool,
    pub deadline: std::time::Instant,
}

impl Browser {
    pub(crate) fn confirm_page_action(&mut self, action: Action) -> bool {
        // Repeated close requests must not replace a pending decision.
        if self.destructive_action.is_some() || self.page_check.is_some() {
            return true;
        }
        let key = match &action {
            Action::Close(key)
            | Action::Discard(key)
            | Action::Fallback(key)
            | Action::Switch(key, _)
            | Action::Navigate(key, _) => Some(*key),
            Action::Quit => None,
        };
        let tabs: Vec<_> = self
            .session()
            .tabs()
            .iter()
            .filter(|tab| {
                key.is_none_or(|key| tab.key == key)
                    && wega::lifecycle::requires_state_check(tab.loaded(), &tab.url())
            })
            .map(|tab| tab.key)
            .collect();
        if tabs.is_empty() {
            return false;
        }
        self.page_check_id = self.page_check_id.wrapping_add(1);
        let request = self.page_check_id;
        self.page_check = Some(Check {
            action,
            request,
            waiting: tabs.iter().copied().collect(),
            dirty: false,
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(2),
        });
        for key in tabs {
            let tab = self
                .session()
                .tabs()
                .iter()
                .find(|tab| tab.key == key)
                .unwrap();
            if let Some(page) = &tab.wega {
                if page
                    .send(wega::engine_protocol::Command::CheckPageState(request))
                    .is_err()
                {
                    self.page_state_result(request, key, true);
                }
            } else if let Some(view) = tab.view() {
                let delegate = self.delegate.clone();
                let window = self.window.clone();
                view.evaluate_javascript(
                    "typeof __wega_page_has_edits==='function' ? __wega_page_has_edits() : true",
                    move |result| {
                        let dirty = !matches!(result, Ok(servo::JSValue::Boolean(false)));
                        delegate
                            .events
                            .borrow_mut()
                            .push_back(crate::engine::Event::PageState(request, key, dirty));
                        window.request_redraw();
                    },
                );
            } else {
                self.page_state_result(request, key, true);
            }
        }
        true
    }

    pub(crate) fn page_state_result(&mut self, request: u64, key: u64, dirty: bool) {
        let Some(check) = &mut self.page_check else {
            return;
        };
        if request != check.request || !check.waiting.remove(&key) {
            return;
        }
        check.dirty |= dirty;
        if !check.waiting.is_empty() {
            return;
        }
        self.finish_page_check();
    }

    fn finish_page_check(&mut self) {
        let Some(check) = self.page_check.take() else {
            return;
        };
        self.destructive_action = Some(check.action);
        if !check.dirty {
            self.answer_page_action(true);
        }
        self.window.request_redraw();
    }

    pub(crate) fn expire_page_check(&mut self) {
        if let Some(check) = &mut self.page_check
            && check.deadline <= std::time::Instant::now()
        {
            check.dirty = true;
            self.finish_page_check();
        }
    }

    pub(crate) fn request_quit(&mut self) {
        if !self.confirm_page_action(Action::Quit) {
            self.quit_requested = true;
        }
    }

    pub(crate) fn answer_page_action(&mut self, proceed: bool) {
        let Some(action) = self.destructive_action.take() else {
            return;
        };
        if !proceed {
            if matches!(action, Action::Navigate(key, _) if key == self.tab().key) {
                self.chrome.address = self.tab().url();
                self.chrome.last_url = self.chrome.address.clone();
            }
            return;
        }
        let key = match action {
            Action::Quit => {
                self.quit_requested = true;
                return;
            }
            Action::Close(key)
            | Action::Discard(key)
            | Action::Fallback(key)
            | Action::Switch(key, _)
            | Action::Navigate(key, _) => key,
        };
        // Resolve identity at decision time; indices may change while a prompt is open.
        let Some(index) = self.session().tabs().iter().position(|tab| tab.key == key) else {
            return;
        };
        match action {
            Action::Close(_) => self.close_tab_confirmed(index),
            Action::Discard(_) => {
                self.discard_tab_confirmed(index);
            }
            Action::Fallback(_) => {
                self.select_tab(index);
                if let Some(url) = self.current_url() {
                    let key = wega::engine_choice::site_key(&url);
                    if self.replace_engine_confirmed(EngineKind::Servo, url, false) {
                        self.mark_fallback(key);
                    }
                }
            }
            Action::Switch(_, engine) => {
                self.select_tab(index);
                self.switch_engine_confirmed(engine);
            }
            Action::Navigate(_, command) => {
                self.select_tab(index);
                if let Err(error) = self.dispatch_navigation_confirmed(command) {
                    self.status = Some(error);
                }
                self.changed();
            }
            Action::Quit => unreachable!(),
        }
    }

    pub(crate) fn page_safety_ui(&mut self, ctx: &egui::Context) {
        let fresh = self.prepare_dialog(ctx, "page-state-protection");
        let label = match self.destructive_action {
            Some(Action::Close(_)) => "Close tab",
            Some(Action::Discard(_)) => "Unload tab",
            Some(Action::Switch(_, _)) | Some(Action::Fallback(_)) => "Switch engine",
            Some(Action::Navigate(_, _)) => "Leave page",
            _ => "Quit browser",
        };
        let mut answer = crate::ux::escape(ctx).then_some(false);
        egui::Modal::new(egui::Id::unique("page-state-protection")).show(ctx, |ui| {
            ui.set_max_width(440.0);
            ui.heading("Unsaved changes may be lost");
            ui.label("This action discards the live page state. Save your work before continuing. Restoring a tab only reloads its address.");
            ui.horizontal(|ui| {
                let cancel = ui.button("Keep working");
                if fresh { cancel.request_focus(); }
                if cancel.clicked() { answer = Some(false); }
                if ui.button(label).clicked() { answer = Some(true); }
            });
        });
        if let Some(proceed) = answer {
            self.answer_page_action(proceed);
            self.finish_dialog(ctx);
        }
    }
}
