//! Native developer toolbox; every panel reads the active Servo webview.
use crate::ui::icon;
use crate::{
    browser::Browser,
    network,
    toolbox::{Tool, pretty, quote},
};
use egui::{RichText, Ui};
use serde_json::Value;
fn document(ui: &mut Ui, value: &Value) {
    ui.add(egui::Label::new(RichText::new(pretty(value)).monospace()).selectable(true));
}
fn code(ui: &mut Ui, text: &mut String, editable: bool) {
    let rows = (ui.available_height() / 17.0).max(4.0) as usize;
    ui.add(
        egui::TextEdit::multiline(text)
            .desired_rows(rows)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY)
            .interactive(editable)
            .code_editor(),
    );
}
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
        let colors = crate::theme::palette(ui.ctx());
        let view = self.view().clone();
        let owner = network::owner(view.id());
        self.tools.page(&view);
        let split = self.tools.split_console && self.tools.active != Tool::Console;
        let full_rect = ui.max_rect();
        let console_height = if split {
            (ui.available_height() * 0.28).clamp(90.0, 180.0)
        } else {
            0.0
        };
        self.tools.split_rect = egui::Rect::NOTHING;
        if split {
            let shown = egui::Panel::bottom("split-console")
                .default_size(console_height)
                .resizable(true)
                .min_size(70.0)
                .show(ui, |ui| {
                    panel_style(ui);
                    self.console_ui(ui, &view, &owner);
                });
            self.tools.split_rect = shown.response.rect;
        }
        let content_rect = ui.available_rect_before_wrap();
        self.tools.content_rect = content_rect;
        let mut content = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        content.set_clip_rect(content_rect.intersect(ui.clip_rect()));
        let ui = &mut content;
        self.tools.panel_rect = ui.max_rect();
        panel_style(ui);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon(ui, "×", "Close developer tools (F12)").clicked() {
                    self.close_developer_tools();
                }
                let compact = crate::ux::compact(ui.ctx().content_rect().width());
                let menu = icon(ui, "⋯", "Toolbox options");
                egui::Popup::menu(&menu).show(|ui| {
                    if ui.button("Rendering diagnostics…").clicked() {
                        self.features.diagnostics = true;
                        ui.close();
                    }
                    ui.separator();
                    for (dock, label) in crate::tool_window::Dock::ALL {
                        // Narrow windows always dock at the bottom; say so instead
                        // of accepting a choice that has no effect.
                        let side = matches!(dock, crate::tool_window::Dock::Left | crate::tool_window::Dock::Right);
                        let enabled = !(compact && side);
                        if ui
                            .add_enabled(enabled, egui::Button::selectable(self.profile().devtools_dock == dock, label))
                            .on_disabled_hover_text("Needs a wider window; the toolbox docks at the bottom below 1100 px.")
                            .clicked()
                        {
                            self.apply(wega::application::Command::Dock(dock));
                            self.tools.page(&view).inspector.reveal = true;
                            self.changed();
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.checkbox(&mut self.tools.split_console, "Show split console");
                });
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 32.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        egui::ScrollArea::horizontal()
                            .id_salt("tool-tabs")
                            .show(ui, |ui| {
                                let tabs: Vec<_> = Tool::ALL.iter().map(|tool| (*tool, tool.name())).collect();
                                if crate::components::tabs(ui, &mut self.tools.active, &tabs) {
                                    self.tools.refresh(&view, &self.servo);
                                }
                            });
                    },
                );
            });
        });
        ui.separator();
        if let Some(error) = self.tools.error.clone() {
            ui.horizontal(|ui| {
                crate::components::inline_message(ui, crate::ux::Severity::Error, &error);
            });
            if ui.small_button("Dismiss").clicked() {
                self.tools.error = None;
            }
        }
        if let Some((notice, remaining)) = self.tools.transient_notice() {
            egui::Frame::new()
                .fill(colors.surface)
                .corner_radius(6)
                .inner_margin(5)
                .show(ui, |ui| {
                    ui.label(RichText::new(notice).small().color(colors.muted));
                });
            ui.ctx().request_repaint_after(remaining);
        }
        match self.tools.active {
            Tool::Network => self.network_ui(ui, &owner),
            Tool::Debugger => self.debugger_ui(ui, &owner),
            Tool::Console => self.console_ui(ui, &view, &owner),
            tool => {
                ui.horizontal(|ui| {
                    if ui.button("Refresh").clicked() {
                        self.tools.refresh(&view, &self.servo);
                    }
                    ui.weak(match tool {
                        Tool::Memory => "Engine allocation report · all browser processes",
                        Tool::Performance => {
                            "Page timings, marks and measures · CPU sampling unavailable"
                        }
                        Tool::Accessibility => "Page semantics and Servo accessibility tree",
                        _ => "Active page",
                    });
                });
                let snapshot = self.tools.pages[&owner].snapshots.get(&tool);
                let pending = snapshot.is_some_and(|s| s.pending);
                let failed = snapshot.and_then(|s| s.error.clone());
                let updated = snapshot.and_then(|s| s.updated);
                let data = snapshot.map(|s| s.data.clone()).unwrap_or(Value::Null);
                if pending {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Reading the page…").color(colors.muted));
                    });
                } else if let Some(updated) = updated.filter(|_| !data.is_null()) {
                    let seconds = updated.elapsed().as_secs();
                    let age = if seconds < 2 {
                        "just now".to_owned()
                    } else if seconds < 120 {
                        format!("{seconds} s ago")
                    } else {
                        format!("{} min ago", seconds / 60)
                    };
                    ui.label(
                        RichText::new(format!("Updated {age}"))
                            .small()
                            .color(colors.muted),
                    );
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_secs(1));
                }
                if let Some(error) = &failed {
                    crate::components::inline_message(ui, crate::ux::Severity::Error, error);
                }
                if data.is_null()
                    && !pending
                    && failed.is_none()
                    && crate::components::empty_state(
                        ui,
                        "Nothing captured yet",
                        "Read the active page to see its data here.",
                        Some("Read page"),
                    )
                {
                    self.tools.refresh(&view, &self.servo);
                }
                if tool == Tool::Inspector {
                    self.inspector_ui(ui, &view, &owner, &data);
                } else if tool == Tool::StyleEditor {
                    self.styles_ui(ui, &view, &owner, &data);
                } else if tool == Tool::Performance {
                    self.performance_ui(ui, &view, &owner, &data);
                } else {
                    egui::ScrollArea::both()
                        .id_salt(tool.name())
                        .auto_shrink([false, false])
                        .show(ui, |ui| match tool {
                            Tool::Inspector => self.inspector_ui(ui, &view, &owner, &data),
                            Tool::StyleEditor => self.styles_ui(ui, &view, &owner, &data),
                            Tool::Storage => self.storage_ui(ui, &view, &owner, &data),
                            Tool::Accessibility => self.accessibility_ui(ui, &view, &owner, &data),
                            _ => document(ui, &data),
                        });
                }
            }
        }

        self.tools.panel_rect = full_rect;
        self.network_dialogs(ui.ctx(), &owner);
        if self.tools.export_open {
            egui::Window::new("Export network HAR")
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    ui.label("Contains captured URLs, headers and bounded body previews.");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.tools.export_path);
                        if ui.button("Choose…").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_file_name("network.har")
                                .save_file()
                        {
                            self.tools.export_path = path.display().to_string();
                        }
                    });
                    if let Some(error) = &self.tools.error {
                        crate::components::inline_message(ui, crate::ux::Severity::Error, error);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Save HAR").clicked() {
                            let result = std::fs::write(
                                &self.tools.export_path,
                                pretty(&self.tools.network.har(&owner)),
                            );
                            // On failure keep the dialog and the typed path.
                            match result {
                                Ok(()) => {
                                    self.tools.notice = Some("HAR saved".into());
                                    self.tools.error = None;
                                    self.tools.export_open = false;
                                }
                                Err(error) => {
                                    self.tools.error =
                                        Some(format!("Could not save the HAR file: {error}"));
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.tools.export_open = false;
                        }
                    });
                });
        }
    }
    fn styles_ui(&mut self, ui: &mut Ui, view: &servo::WebView, owner: &str, _data: &Value) {
        let sheets = self.tools.pages[owner].style_sheets.clone();
        let page = &self.tools.pages[owner];
        if !page.style_loaded
            && !page
                .snapshots
                .get(&Tool::StyleEditor)
                .is_some_and(|s| s.pending)
            && let Some(sheets) = sheets.as_array()
            && let Some(sheet) = sheets
                .iter()
                .find(|s| s["style_sheet_index"].as_i64() == Some(page.style_index as i64))
                .or_else(|| sheets.first())
        {
            self.tools.stylesheet(
                view,
                sheet["style_sheet_index"].as_i64().unwrap_or(0) as i32,
            );
        }
        egui::Panel::left("stylesheet-list")
            .resizable(true)
            .default_size(220.0)
            .min_size(100.0)
            .max_size((ui.available_width() * 0.55).max(100.0))
            .show(ui, |ui| {
                self.tools.style_rects[0] = ui.max_rect();
                ui.strong("Stylesheets");
                egui::ScrollArea::both()
                    .id_salt("stylesheet-list-scroll")
                    .show(ui, |ui| {
                        if let Some(sheets) = sheets.as_array() {
                            for sheet in sheets {
                                let index = sheet["style_sheet_index"].as_i64().unwrap_or(0) as i32;
                                let label = sheet["href"].as_str().unwrap_or("Inline stylesheet");
                                if ui
                                    .selectable_label(
                                        self.tools.pages[owner].style_index == index,
                                        label,
                                    )
                                    .clicked()
                                {
                                    self.tools.stylesheet(view, index);
                                }
                            }
                            if sheets.is_empty() {
                                ui.weak("No stylesheets in this document.");
                            }
                        }
                    });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                self.tools.style_rects[1] = ui.max_rect();
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Apply CSS override").clicked() {
                        let page = self.tools.page(view);
                        let action = crate::probes::edit_css(page.style_index, &page.css);
                        self.tools.run(view, Tool::StyleEditor, action);
                    }
                    if ui.button("Reset overrides").clicked() {
                        self.tools
                            .run(view, Tool::StyleEditor, crate::probes::RESET_CSS.into());
                    }
                });
                ui.separator();
                egui::ScrollArea::both()
                    .id_salt("stylesheet-code")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        code(ui, &mut self.tools.page(view).css, true);
                    });
            });
    }
    fn storage_ui(&mut self, ui: &mut Ui, view: &servo::WebView, owner: &str, data: &Value) {
        if let Some(origin) = data["origin"].as_str() {
            ui.strong(format!("Origin: {origin}"));
        }
        if data["opaque"] == true {
            ui.weak("This document has an opaque origin. Web storage is unavailable here; open an HTTP or HTTPS page to inspect its storage.");
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tools.page(view).storage_area, 0, "Local storage");
            ui.selectable_value(
                &mut self.tools.page(view).storage_area,
                1,
                "Session storage",
            );
            ui.selectable_value(&mut self.tools.page(view).storage_area, 2, "Cookies");
        });
        if self.tools.pages[owner].storage_area == 2 {
            self.tools.page(view).cookies.ui(ui);
            return;
        }
        let area = if self.tools.pages[owner].storage_area == 0 {
            "local"
        } else {
            "session"
        };
        let available = data[area].is_array();
        if let Some(error) = data[area]["error"].as_str() {
            ui.colored_label(crate::theme::palette(ui.ctx()).amber, error);
        }
        if data[area].as_array().is_some_and(Vec::is_empty) {
            ui.weak("No items stored for this origin.");
        }
        ui.add_enabled_ui(available, |ui| {
            egui::Grid::new("storage-rows")
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Key");
                    ui.strong("Value");
                    ui.end_row();
                    if let Some(rows) = data[area].as_array() {
                        for row in rows {
                            if ui
                                .selectable_label(
                                    self.tools.pages[owner].storage_key
                                        == row["key"].as_str().unwrap_or(""),
                                    row["key"].as_str().unwrap_or(""),
                                )
                                .clicked()
                            {
                                let page = self.tools.page(view);
                                page.storage_key = row["key"].as_str().unwrap_or("").into();
                                page.storage_value = row["value"].as_str().unwrap_or("").into();
                            }
                            ui.label(row["value"].as_str().unwrap_or(""));
                            ui.end_row();
                        }
                    }
                });
            ui.horizontal(|ui| {
                ui.label("Key");
                ui.text_edit_singleline(&mut self.tools.page(view).storage_key);
                ui.label("Value");
                ui.text_edit_singleline(&mut self.tools.page(view).storage_value);
            });
            ui.horizontal(|ui| {
                let save = ui.button("Set item").clicked();
                let delete = ui.button("Delete selected item").clicked();
                if save || delete {
                    let page = self.tools.page(view);
                    let store = if page.storage_area == 0 {
                        "localStorage"
                    } else {
                        "sessionStorage"
                    };
                    let command = if save {
                        format!(
                            "{store}.setItem({},{});",
                            quote(&page.storage_key),
                            quote(&page.storage_value)
                        )
                    } else {
                        format!("{store}.removeItem({});", quote(&page.storage_key))
                    };
                    self.tools.run(
                        view,
                        Tool::Storage,
                        format!("{command}{}", crate::probes::STORAGE),
                    );
                }
            });
        });
        ui.strong("Page-visible cookies");
        if let Some(error) = data["cookieError"].as_str() {
            ui.weak(error);
        } else if let Some(cookies) = data["cookies"].as_str() {
            ui.monospace(if cookies.is_empty() {
                "No page-visible cookies."
            } else {
                cookies
            });
        }
        ui.weak("The Cookies tab includes HttpOnly cookies from Servo. IndexedDB editing is unavailable.");
    }
    fn accessibility_ui(&mut self, ui: &mut Ui, _view: &servo::WebView, owner: &str, data: &Value) {
        document(ui, data);
        ui.separator();
        ui.strong("Engine accessibility nodes");
        for (id, node) in &self.tools.pages[owner].ax {
            ui.collapsing(id, |ui| {
                document(ui, node);
            });
        }
    }
    fn network_ui(&mut self, ui: &mut Ui, owner: &str) {
        let colors = crate::theme::palette(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if self.tools.network.paused {
                    "Resume recording"
                } else {
                    "Pause recording"
                })
                .clicked()
            {
                self.tools.network.paused = !self.tools.network.paused;
            }
            if ui.button("Clear").clicked() {
                self.tools.network.clear(owner);
            }
            if ui
                .checkbox(
                    &mut self.tools.network.disable_cache,
                    "Disable cache (all tabs)",
                )
                .changed()
            {
                self.servo.set_preference(
                    "network_http_cache_disabled",
                    servo::PrefValue::Bool(self.tools.network.disable_cache),
                );
                if self.tools.network.disable_cache {
                    self.servo.network_manager().clear_cache();
                }
            }
            if ui.button("Reload").clicked() {
                self.reload();
            }
            self.network_options(ui, owner);
            if ui.button("New request…").clicked() {
                self.tools.network_ui.composer = Some(network::Composer {
                    url: self.tab().url(),
                    ..Default::default()
                });
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.tools.network.query)
                    .hint_text("Filter URL or domain:example.com")
                    .desired_width(210.0),
            ).on_hover_text("Filters: domain:example.com method:POST status:404 type:XHR is:cached is:imported · prefix - to exclude");
        });
        ui.horizontal_wrapped(|ui| {
            for (kind, label) in [
                ("", "All"),
                ("HTML", "HTML"),
                ("CSS", "CSS"),
                ("JS", "JS"),
                ("XHR", "XHR"),
                ("Fetch", "Fetch"),
                ("Font", "Fonts"),
                ("Img", "Images"),
                ("Media", "Media"),
                ("WS", "WS"),
                ("Other", "Other"),
            ] {
                let selected = if kind.is_empty() {
                    self.tools.network.types.is_empty()
                } else {
                    self.tools.network.types.contains(kind)
                };
                if ui.selectable_label(selected, label).clicked() {
                    self.tools.network.filter.clear();
                    if kind.is_empty() {
                        self.tools.network.types.clear();
                    } else if !self.tools.network.types.remove(kind) {
                        self.tools.network.types.insert(kind.into());
                    }
                }
            }
        });

        if self
            .tools
            .network
            .rows
            .iter()
            .any(|r| Some(r.uid) == self.tools.network.selected && r.owner == owner)
        {
            let details = if ui.available_width() < 900.0 {
                egui::Panel::bottom("network-detail-pane-compact")
            } else {
                egui::Panel::right("network-detail-pane")
            };
            details
                .resizable(true)
                .default_size(420.0)
                .min_size(150.0)
                .max_size(if ui.available_width() < 900.0 {
                    (ui.available_height() * 0.55).max(150.0)
                } else {
                    (ui.available_width() * 0.5).max(150.0)
                })
                .show(ui, |ui| {
                    self.tools.network_rects[1] = ui.max_rect();
                    self.network_details(ui, owner);
                });
        }
        let mut clear_filters = false;
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
        self.tools.network_rects[0] = ui.max_rect();
        let rows = self.tools.network.visible(owner);
        let count = rows.len();
        let bytes: usize = rows
            .iter()
            .fold(0usize, |sum, r| sum.saturating_add(r.response_body.size));
        let pending = rows.iter().filter(|r| r.status.is_none()).count();
        let transferred: u64 = rows
            .iter()
            .filter_map(|r| r.transfer())
            .fold(0u64, u64::saturating_add);
        let unknown = rows.iter().filter(|r| r.transfer().is_none()).count();
        let summary = format!(
            "{count} requests · {} response data · {} reported transferred{} · {pending} pending · 500 rows / 64 KiB previews max",
            network::size_label(bytes as u64),
            network::size_label(transferred),
            if unknown > 0 {
                format!(" ({unknown} sizes unknown)")
            } else {
                String::new()
            }
        );
        if count == 0 {
            let recorded = self.tools.network.rows.iter().any(|r| r.owner == owner);
            if !recorded {
                crate::components::empty_state(ui, "No requests yet", "Reload or navigate to record this tab's network traffic.", None);
            } else if crate::components::empty_state(ui, "No requests match the filters", "", Some("Clear filters")) {
                clear_filters = true;
            }
        }
        let selected = self.tools.network.selected;
        let mut clicked = None;
        let mut action = None;
        let mut sorting = None;
        let first = rows.iter().map(|r| r.started_ms).min();
        let span = rows
            .iter()
            .map(|r| {
                first
                    .map(|f| r.started_ms.saturating_sub(f) as f64 + r.duration_ms())
                    .unwrap_or(1.0)
            })
            .fold(1.0, f64::max);
        use network::Sort;
        let mut columns = vec![
            (Sort::Status, 65.0, "Status"),
            (Sort::Method, 65.0, "Method"),
            (Sort::Domain, 135.0, "Domain"),
            (Sort::File, 200.0, "File"),
        ];
        if self.tools.network.show_initiator {
            columns.push((Sort::Initiator, 100.0, "Initiator"));
        }
        columns.push((Sort::Type, 65.0, "Type"));
        if self.tools.network.show_transferred {
            columns.push((Sort::Transferred, 100.0, "Transferred"));
        }
        columns.extend([(Sort::Size, 85.0, "Size"), (Sort::Duration, 85.0, "Time")]);
        let used: f32 = columns.iter().map(|(_, w, _)| w).sum();
        columns.push((
            Sort::Started,
            (ui.available_width() - used).max(150.0),
            "Waterfall",
        ));
        for (column, width, _) in &mut columns {
            let configured = self.tools.network_ui.widths[*column as usize];
            if configured > 0.0 {
                *width = configured;
            }
        }
        let total: f32 = columns.iter().map(|(_, width, _)| *width).sum();
        if ui.available_width() >= 600.0 && total > ui.available_width()
            && self.tools.network_ui.widths.iter().all(|width| *width <= 0.0) {
            let scale = ui.available_width() / total;
            for (_, width, _) in &mut columns { *width *= scale; }
        }
        let widths: Vec<_> = columns.iter().map(|(_, w, _)| *w).collect();
        let table_width: f32 = widths.iter().sum();
        let headings: Vec<_> = columns
            .iter()
            .map(|(sort, _, label)| {
                format!(
                    "{label}{}",
                    if self.tools.network.sort == *sort {
                        if self.tools.network.descending {
                            " ↓"
                        } else {
                            " ↑"
                        }
                    } else {
                        ""
                    }
                )
            })
            .collect();
        let summary_galley = ui.painter().layout(summary.clone(), egui::TextStyle::Small.resolve(ui.style()), colors.muted, ui.available_width());
        let footer_height = summary_galley.size().y + ui.spacing().item_spacing.y + ui.spacing().scroll.bar_width + 8.0;
        let mut resized = None;
        egui::ScrollArea::both()
            .id_salt("network-table")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .max_height((ui.available_height() - footer_height).max(0.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (header, _) =
                    ui.allocate_exact_size(egui::vec2(table_width, 24.0), egui::Sense::hover());
                ui.painter().rect_filled(header, 4, colors.surface);
                table_cells(
                    ui,
                    header,
                    &widths,
                    &headings.iter().map(String::as_str).collect::<Vec<_>>(),
                    colors.muted,
                );
                let mut column_x = header.left();
                for (column, width, _) in &columns {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(column_x, header.top()),
                        egui::vec2(*width - 6.0, header.height()),
                    );
                    if ui
                        .interact(
                            rect,
                            ui.scope_id().with(("network-sort", *column as usize)),
                            egui::Sense::click(),
                        )
                        .on_hover_text("Click to sort; click again to reverse")
                        .clicked()
                    {
                        sorting = Some(*column);
                    }
                    let grip = egui::Rect::from_min_size(
                        egui::pos2(column_x + width - 6.0, header.top()),
                        egui::vec2(6.0, header.height()),
                    );
                    let id = ui.scope_id().with(("column-resize", *column as usize));
                    let drag = ui
                        .interact(grip, id, egui::Sense::drag())
                        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                    if drag.drag_started() {
                        ui.ctx().data_mut(|d| d.insert_temp(id, *width));
                    }
                    if drag.dragged() {
                        let initial = ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or(*width);
                        resized =
                            Some((*column, (initial + drag.drag_delta().x).clamp(50.0, 700.0)));
                    }
                    ui.painter().vline(
                        column_x + width,
                        header.y_range(),
                        egui::Stroke::new(1.0, colors.border),
                    );
                    column_x += width;
                }
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, row) in rows.iter().enumerate() {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(table_width, 25.0), egui::Sense::click());
                    let fill = if selected == Some(row.uid) {
                        colors.selected
                    } else if response.hovered() {
                        colors.hover
                    } else if index % 2 == 0 {
                        colors.chrome
                    } else {
                        colors.inset
                    };
                    ui.painter().rect_filled(rect, 0, fill);
                    let values: Vec<String> = columns
                        .iter()
                        .map(|(column, _, _)| match column {
                            Sort::Status => row.status_label(),
                            Sort::Method => row.method.clone(),
                            Sort::Domain => row.domain(),
                            Sort::File => format!(
                                "{}{}",
                                if row.imported { "[HAR] " } else { "" },
                                row.filename()
                            ),
                            Sort::Initiator => {
                                if row.initiator.is_empty() {
                                    "—".into()
                                } else {
                                    row.initiator.clone()
                                }
                            }
                            Sort::Type => row.kind.clone(),
                            Sort::Transferred => {
                                if row.cached {
                                    "cached".into()
                                } else {
                                    row.transfer()
                                        .map(network::size_label)
                                        .unwrap_or_else(|| "unknown".into())
                                }
                            }
                            Sort::Size => network::size_label(row.response_body.size as u64),
                            Sort::Duration => network::duration_label(row.duration_ms()),
                            Sort::Started => String::new(),
                        })
                        .collect();
                    let cells = values.iter().map(String::as_str).collect::<Vec<_>>();
                    table_cells(ui, rect, &widths, &cells, colors.text);
                    let status = match row.status {
                        Some(300..400) => colors.amber,
                        Some(400..=599) | Some(0) => colors.danger,
                        _ if selected == Some(row.uid) => colors.text,
                        _ => colors.muted,
                    };
                    let status_rect =
                        egui::Rect::from_min_size(rect.min, egui::vec2(widths[0], rect.height()));
                    ui.painter().rect_filled(status_rect, 0, fill);
                    paint_cell(ui, status_rect, &values[0], status);
                    let x = rect.left() + widths[..widths.len() - 1].iter().sum::<f32>() + 6.0;
                    let width = widths[widths.len() - 1] - 12.0;
                    let offset = first
                        .map(|f| row.started_ms.saturating_sub(f) as f64)
                        .unwrap_or(0.0);
                    let left = x + width * (offset / span) as f32;
                    let bar_width = (width * (row.duration_ms() / span) as f32).max(2.0);
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(left, rect.center().y - 3.0),
                            egui::vec2(bar_width.min(x + width - left), 6.0),
                        ),
                        2,
                        colors.accent,
                    );
                    response.context_menu(|ui| {
                        if let Some(kind) = Self::network_row_menu(ui, row) {
                            action = Some((kind, row.uid));
                        }
                    });
                    if response.on_hover_text(&row.url).clicked() {
                        clicked = Some(row.uid);
                    }
                }
            });
        ui.weak(summary).on_hover_text("Transferred uses reported response payload sizes; this engine does not report complete wire bytes including headers.");
        if let Some(id) = clicked {
            self.tools.network.selected = Some(id);
        }
        if let Some((column, width)) = resized {
            self.tools.network_ui.widths[column as usize] = width;
        }
        if let Some(column) = sorting {
            if self.tools.network.sort == column {
                self.tools.network.descending = !self.tools.network.descending;
            } else {
                self.tools.network.sort = column;
                self.tools.network.descending = false;
            }
        }
        if let Some((kind, id)) = action
            && let Some(row) = self.tools.network.rows.iter().find(|r| r.uid == id)
        {
            match kind {
                "compose" => {
                    self.tools.network_ui.composer = Some(network::Composer::from_request(row))
                }
                "open" => {
                    let url = row.url.clone();
                    self.add_tab(&url);
                }
                "download"=>{let url=row.url.clone();self.offer_download(&url,"");}
                "block" => {
                    let domain = row.domain();
                    if !self.profile()
                        .policy
                        .blocked_hosts
                        .split_whitespace()
                        .any(|h| h == domain)
                    {
                        let mut policy = self.profile().policy.clone();
                        policy.blocked_hosts.push_str(&format!("\n{domain}"));
                        self.apply(wega::application::Command::Policy(policy));
                        self.changed();
                    }
                    self.tools.notice = Some(format!("Blocked {domain}; reload to apply."));
                }
                _ => {}
            }
        }

        });
        if clear_filters {
            let network = &mut self.tools.network;
            network.query.clear();
            network.filter.clear();
            network.types.clear();
        }
        let dropped = self
            .tools
            .bridge
            .dropped
            .load(std::sync::atomic::Ordering::Relaxed);
        if dropped > 0 {
            ui.colored_label(
                colors.amber,
                format!(
                    "{dropped} developer events dropped under load; capture may be incomplete."
                ),
            );
        }
    }
    fn network_details(&mut self, ui: &mut Ui, owner: &str) {
        let mut close = false;
        if let Some(row) = self
            .tools
            .network
            .rows
            .iter()
            .find(|r| Some(r.uid) == self.tools.network.selected && r.owner == owner)
        {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon(ui, "×", "Close request details").clicked() {
                        close = true;
                    }
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), 32.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for (index, label) in [
                                    "Headers", "Request", "Response", "Timings", "Security",
                                    "Cookies",
                                ]
                                .iter()
                                .enumerate()
                                {
                                    ui.selectable_value(&mut self.tools.detail_tab, index, *label);
                                }
                                if ui.small_button("Copy URL").clicked() {
                                    ui.ctx().copy_text(row.url.clone());
                                }
                            });
                        },
                    );
                });
            });
            egui::ScrollArea::both().id_salt("request-details").show(ui,|ui|{
                ui.monospace(format!("{} {}",row.method,row.url));
match self.tools.detail_tab{
                    0=>{
ui.strong(format!("{} {}",row.status_label(),row.reason));
for(title,headers)in [("Response headers",&row.response_headers),("Request headers",&row.request_headers)]{
ui.strong(title);
for(name,value)in headers{
ui.monospace(format!("{name}: {value}"));
}
}
}
                    1=>{
if let Ok(url)=url::Url::parse(&row.url){
for(k,v)in url.query_pairs(){
ui.monospace(format!("{k}: {v}"));
}
}
body_ui(ui,&row.request_body);
}
                    2=>{
ui.horizontal(|ui|{ui.label("Find in preview");ui.text_edit_singleline(&mut self.tools.network_ui.body_search);});
if !self.tools.network_ui.body_search.is_empty(){
    let query=self.tools.network_ui.body_search.to_lowercase();
    if let Some(text)=row.response_body.text(){let lines:Vec<_>=text.lines().enumerate().filter(|(_,line)|line.to_lowercase().contains(&query)).collect();ui.label(format!("{} matching lines in captured preview",lines.len()));for (index,line) in lines.iter().take(500){ui.monospace(format!("{}: {line}",index+1));}}
}else{body_ui(ui,&row.response_body);}
}
                    3=>{
ui.label(format!("Observed total: {:.2} ms\nConnection: {:.2} ms\nSend: {:.2} ms",row.duration_ms(),row.connect_ms,row.send_ms));
ui.weak("DNS, TLS, server wait and receive phases are not separately reported by this Servo version. Pending means no response event has arrived.");
}
                    5=>{
ui.strong("Request cookies");
for (_,value) in row.request_headers.iter().filter(|(k,_)|k.eq_ignore_ascii_case("cookie")){for cookie in value.split(';'){ui.monospace(cookie.trim());}}
ui.strong("Response Set-Cookie");
for (_,value) in row.response_headers.iter().filter(|(k,_)|k.eq_ignore_ascii_case("set-cookie")){ui.monospace(value);}
ui.weak("Shows cookies captured in HTTP headers. Imported HAR cookie arrays are not substituted for missing headers.");
}
                    _=>{
ui.monospace(if row.security.is_empty(){
"No TLS details reported for this request"}
else{
&row.security}
);
}
                }
            }
);
        }
        if close {
            self.tools.network.selected = None;
        }
    }
}
fn body_ui(ui: &mut Ui, body: &network::Body) {
    ui.weak(format!(
        "{} bytes{}",
        body.size,
        if body.truncated() {
            " · preview truncated at 64 KiB"
        } else {
            ""
        }
    ));
    if let Some(text) = body.text() {
        let formatted = serde_json::from_str::<Value>(text)
            .map(|v| pretty(&v))
            .unwrap_or_else(|_| text.into());
        ui.add(egui::Label::new(RichText::new(formatted).monospace()).selectable(true));
    } else {
        ui.label("Binary response · text preview unavailable");
    }
}

fn paint_cell(ui: &Ui, rect: egui::Rect, text: &str, color: egui::Color32) {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    ui.painter()
        .with_clip_rect(rect.shrink2(egui::vec2(6.0, 0.0)).intersect(ui.clip_rect()))
        .text(
            rect.min + egui::vec2(6.0, 5.0),
            egui::Align2::LEFT_TOP,
            text,
            egui::FontId::monospace(12.0),
            color,
        );
}
fn table_cells(ui: &Ui, rect: egui::Rect, widths: &[f32], values: &[&str], color: egui::Color32) {
    let mut x = rect.left();
    for (width, value) in widths.iter().zip(values) {
        let cell =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*width, rect.height()));
        paint_cell(ui, cell, value, color);
        x += width;
    }
}

#[cfg(test)]
#[path = "devtools_ui_tests.rs"]
mod tests;
