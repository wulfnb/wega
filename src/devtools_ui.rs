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
        self.devtools_tick(ui.ctx());
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
        let pinned = self.profile().devtools_dock == crate::tool_window::Dock::Window;
        if let Some((tab, _)) = self.devtools_target() {
            let title = tab.title();
            ui.monospace(format!(
                "Target: {} (tab {}){}",
                if title.is_empty() { tab.url() } else { title },
                tab.key,
                if pinned { " · pinned" } else { "" }
            ));
        }
        if !self.devtools.frames_cache.is_empty() {
            let frames = self.devtools.frames_cache.clone();
            ui.horizontal(|ui| {
                ui.label("Frame:");
                let current_frame = self.devtools.target_frame;
                let current_label = match current_frame {
                    None => "Top-level frame".to_string(),
                    Some(id) => frames.iter().find(|f| f.key == id.0).map_or_else(
                        || format!("Frame {}", id.0),
                        |f| {
                            let origin = if f.sandboxed {
                                "opaque (sandboxed)"
                            } else {
                                &f.origin
                            };
                            if f.name.is_empty() {
                                format!("Frame {} ({})", f.key, origin)
                            } else {
                                format!("\"{}\" ({})", f.name, origin)
                            }
                        },
                    ),
                };
                egui::ComboBox::from_id_salt("devtools_frame_selector")
                    .selected_text(current_label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(current_frame.is_none(), "Top-level frame")
                            .clicked()
                        {
                            self.devtools_select_frame(None);
                        }
                        for frame in &frames {
                            let is_selected = current_frame
                                == Some(wega::engine_protocol::identity::FrameId(frame.key));
                            let origin = if frame.sandboxed {
                                "opaque"
                            } else {
                                &frame.origin
                            };
                            let label = if frame.name.is_empty() {
                                format!("Frame {} [{}]", frame.key, origin)
                            } else {
                                format!("\"{}\" [{}]", frame.name, origin)
                            };
                            if ui.selectable_label(is_selected, label).clicked() {
                                self.devtools_select_frame(Some(
                                    wega::engine_protocol::identity::FrameId(frame.key),
                                ));
                            }
                        }
                    });

                if let Some(current_id) = current_frame {
                    let parent = frames
                        .iter()
                        .find(|f| f.key == current_id.0)
                        .and_then(|f| f.parent);
                    if ui.button("↑ Parent Frame").clicked() {
                        self.devtools_select_frame(
                            parent.map(wega::engine_protocol::identity::FrameId),
                        );
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.tools.elements_tab, "Elements")
                .clicked()
            {
                self.tools.elements_tab = true;
                self.tools.console_tab = false;
            }
            if ui
                .selectable_label(
                    !self.tools.elements_tab && !self.tools.console_tab,
                    "Status",
                )
                .clicked()
            {
                self.tools.elements_tab = false;
                self.tools.console_tab = false;
            }
            if ui
                .selectable_label(self.tools.console_tab, "Console")
                .clicked()
            {
                self.tools.elements_tab = false;
                self.tools.console_tab = true;
            }
        });
        self.devtools.elements_active = self.tools.elements_tab;
        self.devtools_state_line(ui);
        if self.tools.elements_tab {
            self.elements_tab(ui);
            return;
        }
        if self.tools.console_tab {
            self.console_tab(ui);
            return;
        }
        // Everything below the line is either engine data that passed the session's target
        // and document-stamp checks, or browser-side supervisor facts labelled as such.
        if let Some(snapshot) = self.devtools_snapshot() {
            ui.monospace(format!("URL: {}", snapshot.url));
            ui.monospace(format!("Title: {}", snapshot.title));
            ui.monospace(format!(
                "Loading: {}",
                if snapshot.loading { "yes" } else { "no" }
            ));
            ui.monospace(format!(
                "Document: {} (revision {})",
                snapshot.document.document.0, snapshot.document.revision.0
            ));
            if let Some(viewport) = snapshot.viewport {
                ui.monospace(format!(
                    "Viewport: {:.0} × {:.0} CSS px @ {:.2}x",
                    viewport.width, viewport.height, viewport.device_pixel_ratio
                ));
            }
            ui.monospace(format!("DOM Nodes: {}", snapshot.nodes));
            ui.monospace(format!(
                "Layout: {} · DOM change counter: {}",
                if snapshot.layout_pending {
                    "pending"
                } else {
                    "current"
                },
                snapshot.mutation_epoch
            ));
            ui.monospace(format!("JS Heap: {} KiB", snapshot.js_heap_bytes / 1024));
            ui.monospace(format!(
                "History: back {} · forward {}",
                if snapshot.can_go_back { "yes" } else { "no" },
                if snapshot.can_go_forward { "yes" } else { "no" }
            ));
            ui.weak(format!(
                "{} · protocol {} · inspector {}",
                snapshot.engine, snapshot.protocol_version, snapshot.inspector_version
            ));
        }
        if self.devtools.dropped_events() > 0 {
            ui.weak(format!(
                "{} inspector events were dropped (slow consumer).",
                self.devtools.dropped_events()
            ));
        }
        if let Some((_, Some(page))) = self.devtools_target() {
            let state = page
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            ui.add_space(4.0);
            ui.weak("Browser-side process facts");
            ui.monospace(format!("PID: {}", page.pid()));
            ui.monospace(format!(
                "Worker RSS: {} KiB",
                page.resident_bytes().unwrap_or(0) / 1024
            ));
            ui.monospace(format!("Phase: {:?}", state.lifecycle.phase()));
            if let Some(reason) = &state.compatibility {
                ui.label(reason);
            }
        }
    }
}

impl Browser {
    fn devtools_state_line(&self, ui: &mut Ui) {
        use crate::devtools_session::ViewState;
        match &self.devtools.state {
            ViewState::Idle => {}
            ViewState::Connecting => {
                ui.weak("Connecting to the page…");
            }
            ViewState::Updating => {
                ui.weak("Updating…");
            }
            ViewState::NotResponding => {
                ui.label("The page is not responding to the inspector. The browser is unaffected.");
            }
            ViewState::Unavailable(reason) => {
                ui.label(format!("Disconnected: {reason}"));
            }
            ViewState::TargetClosed => {
                ui.label("The inspected tab was closed.");
            }
            ViewState::NoPage => {
                ui.label("Open a web page to inspect its worker.");
            }
            ViewState::Attached => {}
        }
    }
}

#[cfg(test)]
#[path = "devtools_ui_tests.rs"]
mod tests;
