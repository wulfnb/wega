//! Conservative protection until both engines can acknowledge a safe close.
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

impl Browser {
    pub(crate) fn confirm_page_action(&mut self, action: Action) -> bool {
        // Repeated close requests must not replace a pending decision.
        if self.destructive_action.is_some() {
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
        let needs_confirmation = self.session().tabs().iter().any(|tab| {
            key.is_none_or(|key| tab.key == key)
                && wega::lifecycle::needs_confirmation(tab.loaded(), &tab.url())
        });
        if needs_confirmation {
            self.destructive_action = Some(action);
            self.window.request_redraw();
        }
        needs_confirmation
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
