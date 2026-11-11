//! Native developer tools for the standalone Wega content worker.
use crate::{browser::Browser, ui::icon};
use egui::Ui;
pub(crate) fn panel_style(ui: &mut Ui) {
    ui.style_mut().override_font_id = None;
    for (style, font) in [
        (egui::TextStyle::Body, egui::FontId::proportional(12.0)),
        (egui::TextStyle::Button, egui::FontId::proportional(12.0)),
        (egui::TextStyle::Monospace, egui::FontId::monospace(12.0)),
    ] {
        ui.style_mut().text_styles.insert(style, font);
    }
    ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
    ui.spacing_mut().item_spacing = egui::vec2(6.0, 5.0);
}
impl Browser {
    pub fn devtools(&mut self, ui: &mut Ui) {
        use crate::tool_window::Dock;
        if self.profile().devtools_dock == Dock::Window {
            return;
        }
        let colors = crate::theme::palette(ui.ctx());
        let dock = if crate::ux::compact(ui.ctx().content_rect().width()) {
            Dock::Bottom
        } else {
            self.profile().devtools_dock
        };
        let (panel, size, maximum) = match dock {
            Dock::Left => (
                egui::Panel::left("tools-left"),
                560.0,
                (ui.available_width() - 200.0).max(220.0),
            ),
            Dock::Right => (
                egui::Panel::right("tools-right"),
                560.0,
                (ui.available_width() - 200.0).max(220.0),
            ),
            _ => (
                egui::Panel::bottom("console"),
                440.0,
                (ui.available_height() - 120.0).max(160.0),
            ),
        };
        // Side docks need room for the toolbar rows; the bottom dock can be shorter.
        let minimum = if dock == Dock::Bottom { 160.0 } else { 320.0 };
        let shown = panel
            .resizable(true)
            .default_size(size)
            .min_size(minimum)
            .max_size(maximum)
            .frame(
                egui::Frame::new()
                    .fill(colors.inset)
                    .corner_radius(12)
                    .outer_margin(8)
                    .inner_margin(10),
            )
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                self.devtools_contents(ui);
            });
        self.tools.panel_rect = shown.response.rect.shrink(8.0);
    }
    pub fn devtools_contents(&mut self, ui: &mut Ui) {
        panel_style(ui);
        ui.horizontal(|ui| {
            ui.heading("Wega Developer Tools");
            if icon(ui, "×", "Close developer tools (F12)").clicked() {
                self.close_developer_tools();
            }
        });
        ui.separator();
        if let Some(error) = &self.tools.error {
            ui.label(error);
        }
        let Some(page) = self.tab().wega.as_ref() else {
            ui.label("Open a web page to inspect its worker.");
            return;
        };
        let state = page
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ui.monospace(format!("URL: {}", state.url));
        ui.monospace(format!("Title: {}", state.title));
        ui.monospace(format!("PID: {}", page.pid()));
        ui.monospace(format!(
            "Worker RSS: {} KiB",
            page.resident_bytes().unwrap_or(0) / 1024
        ));
        ui.monospace(format!("JS Heap: {} KiB", state.js_heap / 1024));
        ui.monospace(format!("DOM Nodes: {}", state.nodes));
        ui.monospace(format!("Phase: {:?}", state.lifecycle.phase()));
        if let Some(reason) = &state.compatibility {
            ui.label(reason);
        }
        ui.weak("Worker inspection is available through WebDriver. The embedded-backend inspector and debugger have been removed.");
    }
}

#[cfg(test)]
#[path = "devtools_ui_tests.rs"]
mod tests;
