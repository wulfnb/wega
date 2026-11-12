//! Native internal pages for Wega (`wega://newtab`, `history`, `bookmarks`, `settings`, `about`).
//!
//! Internal pages are rendered natively via egui directly inside the tab viewport.
//! They do not require starting external engine worker processes.

use crate::{
    browser::{Browser, Tab},
    components, theme,
};
use egui::{CornerRadius, RichText, Sense, Stroke};
use wega::application::Command as AppCommand;

#[derive(Clone, Debug, Default)]
pub struct InternalPagesState {
    pub search_query: String,
    pub history_filter: String,
    pub bookmarks_filter: String,
}

impl Tab {
    /// True if this tab represents a native internal browser page.
    pub(crate) fn is_internal(&self) -> bool {
        if let Some(page) = &self.wega {
            return page.url().starts_with("wega://");
        }
        self.initial_url.starts_with("wega://")
    }
}

impl Browser {
    pub(crate) fn is_internal(&self) -> bool {
        self.tab().is_internal()
    }

    pub(crate) fn internal_viewport(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        _outer: egui::Rect,
    ) {
        let colors = theme::palette(ui.ctx());
        ui.painter().rect_filled(rect, 0.0, colors.inset);

        let url_str = self.tab().url();
        let page = url_str
            .strip_prefix("wega://")
            .unwrap_or("newtab")
            .split('?')
            .next()
            .unwrap_or("newtab");

        let mut next_nav = None;

        let margin = egui::Margin::symmetric(24, 20);
        let content_width = (rect.width() - 48.0).clamp(300.0, 760.0);

        ui.allocate_ui(rect.size(), |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.set_width(content_width);
                        ui.add_space(margin.top as f32);

                        // Top navigation pill bar
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let nav_items = [
                                ("newtab", "New tab", "wega://newtab"),
                                ("history", "History", "wega://history"),
                                ("bookmarks", "Bookmarks", "wega://bookmarks"),
                                ("settings", "Settings", "wega://settings"),
                                ("about", "About", "wega://about"),
                            ];
                            for (id, label, dest) in nav_items {
                                let selected = page == id;
                                if components::chip(ui, selected, label)
                                    .on_hover_text(format!("Navigate to {dest}"))
                                    .clicked()
                                    && !selected
                                {
                                    next_nav = Some(dest.to_string());
                                }
                            }
                        });

                        ui.add_space(20.0);

                        // Render active page
                        match page {
                            "newtab" => self.render_native_newtab(ui, &mut next_nav),
                            "history" => self.render_native_history(ui, &mut next_nav),
                            "bookmarks" => self.render_native_bookmarks(ui, &mut next_nav),
                            "settings" => self.render_native_settings(ui, &mut next_nav),
                            "about" => self.render_native_about(ui),
                            other => self.render_native_not_found(ui, other, &mut next_nav),
                        }

                        ui.add_space(margin.bottom as f32);
                    });
                });
        });

        if let Some(dest) = next_nav {
            self.navigate(&dest);
        }
    }

    fn render_native_newtab(&mut self, ui: &mut egui::Ui, next_nav: &mut Option<String>) {
        let colors = theme::palette(ui.ctx());

        ui.heading(
            RichText::new("Welcome to Wega")
                .size(28.0)
                .color(colors.text)
                .strong(),
        );
        ui.add_space(16.0);

        // Search engine details
        let policy = &self.profile().policy;
        let engine_name = match policy.search_engine {
            wega::model::SearchEngine::Custom => "the web".to_string(),
            engine => engine.name().to_string(),
        };

        // Search controls
        let mut do_search = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let width = ui.available_width() - 110.0;
            let edit = ui.add_sized(
                [width, 38.0],
                egui::TextEdit::singleline(&mut self.internal_state.search_query)
                    .hint_text(format!("Search {engine_name} or type an address"))
                    .frame(
                        egui::Frame::new()
                            .fill(colors.surface)
                            .stroke(Stroke::new(1.0, colors.border))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(egui::Margin::symmetric(12, 8)),
                    ),
            );
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                do_search = true;
            }
            if ui
                .add_sized([100.0, 38.0], components::primary_button(ui, "Search"))
                .clicked()
            {
                do_search = true;
            }
        });

        if do_search && !self.internal_state.search_query.trim().is_empty() {
            let query = self.internal_state.search_query.trim().to_string();
            self.internal_state.search_query.clear();
            *next_nav = Some(query);
        }

        let shortcut = if cfg!(target_os = "macos") {
            "⌘L"
        } else {
            "Ctrl+L"
        };
        ui.label(
            RichText::new(format!("Press {shortcut} to type an address."))
                .size(12.0)
                .color(colors.muted),
        );

        ui.add_space(28.0);

        // Shortcuts tiles ("Your pages")
        let shortcuts = self.profile().new_tab_shortcuts();
        if !shortcuts.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Your pages")
                        .size(16.0)
                        .color(colors.muted)
                        .strong(),
                );
            });
            ui.add_space(8.0);

            let cols = 3;
            egui::Grid::new("newtab-shortcuts")
                .spacing(egui::vec2(12.0, 12.0))
                .min_col_width((ui.available_width() - 24.0) / (cols as f32))
                .show(ui, |ui| {
                    for (i, entry) in shortcuts.into_iter().enumerate() {
                        let tile_resp = egui::Frame::new()
                            .fill(colors.surface)
                            .stroke(Stroke::new(1.0, colors.border))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(egui::Margin::symmetric(14, 12))
                            .show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    if let Some(texture) = self.favicons.get(&entry.url) {
                                        ui.image((texture.id(), egui::vec2(24.0, 24.0)));
                                    } else {
                                        ui.label(RichText::new("▤").size(20.0).color(colors.muted));
                                    }
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(&entry.title)
                                            .size(13.0)
                                            .color(colors.text)
                                            .strong(),
                                    );
                                });
                            });
                        let click_resp = ui.interact(
                            tile_resp.response.rect,
                            ui.scope_id().with(("tile", i)),
                            Sense::click(),
                        );
                        if click_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if click_resp.clicked() {
                            *next_nav = Some(entry.url.clone());
                        }

                        if (i + 1) % cols == 0 {
                            ui.end_row();
                        }
                    }
                });
            ui.add_space(24.0);
        }

        // Recent history
        let recent: Vec<_> = self.profile().history.iter().take(6).cloned().collect();
        if !recent.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Recent")
                        .size(16.0)
                        .color(colors.muted)
                        .strong(),
                );
            });
            ui.add_space(8.0);

            for entry in recent {
                let row = egui::Frame::new()
                    .fill(colors.surface)
                    .stroke(Stroke::new(0.5, colors.border))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if let Some(texture) = self.favicons.get(&entry.url) {
                                ui.image((texture.id(), egui::vec2(16.0, 16.0)));
                            } else {
                                ui.label(RichText::new("▤").size(14.0).color(colors.muted));
                            }
                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(&entry.title)
                                        .size(13.0)
                                        .color(colors.text)
                                        .strong(),
                                );
                                ui.label(RichText::new(&entry.url).size(11.0).color(colors.muted));
                            });
                        });
                    });
                let click_resp = ui.interact(
                    row.response.rect,
                    ui.scope_id().with(("recent", &entry.url)),
                    Sense::click(),
                );
                if click_resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if click_resp.clicked() {
                    *next_nav = Some(entry.url);
                }
                ui.add_space(4.0);
            }
        }
    }

    fn render_native_history(&mut self, ui: &mut egui::Ui, next_nav: &mut Option<String>) {
        let colors = theme::palette(ui.ctx());

        ui.horizontal(|ui| {
            ui.heading(
                RichText::new("History")
                    .size(24.0)
                    .color(colors.text)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.profile().history.is_empty()
                    && ui
                        .add(components::destructive_button(ui, "Clear history…"))
                        .on_hover_text("Clear browsing history across all sessions")
                        .clicked()
                {
                    self.chrome.confirm_clear_history = true;
                }
            });
        });
        ui.add_space(12.0);

        // Search filter
        ui.add_sized(
            [ui.available_width(), 34.0],
            egui::TextEdit::singleline(&mut self.internal_state.history_filter)
                .hint_text("Filter history…")
                .frame(
                    egui::Frame::new()
                        .fill(colors.surface)
                        .stroke(Stroke::new(1.0, colors.border))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(10, 6)),
                ),
        );
        ui.add_space(16.0);

        let query = self.internal_state.history_filter.trim().to_lowercase();
        let items: Vec<_> = self
            .profile()
            .history
            .iter()
            .filter(|entry| {
                if query.is_empty() {
                    true
                } else {
                    entry.title.to_lowercase().contains(&query)
                        || entry.url.to_lowercase().contains(&query)
                }
            })
            .cloned()
            .collect();

        if items.is_empty() {
            if query.is_empty() {
                components::empty_state(
                    ui,
                    "No history yet",
                    "Web pages you visit will appear here.",
                    None,
                );
            } else {
                components::empty_state(
                    ui,
                    "No matches",
                    "Try a different search term or address.",
                    None,
                );
            }
            return;
        }

        for entry in items {
            let row = egui::Frame::new()
                .fill(colors.surface)
                .stroke(Stroke::new(0.5, colors.border))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(texture) = self.favicons.get(&entry.url) {
                            ui.image((texture.id(), egui::vec2(16.0, 16.0)));
                        } else {
                            ui.label(RichText::new("▤").size(14.0).color(colors.muted));
                        }
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(&entry.title)
                                    .size(13.0)
                                    .color(colors.text)
                                    .strong(),
                            );
                            ui.label(RichText::new(&entry.url).size(11.0).color(colors.muted));
                        });
                    });
                });
            let click_resp = ui.interact(
                row.response.rect,
                ui.scope_id()
                    .with(("history-item", &entry.url, entry.visited_at)),
                Sense::click(),
            );
            if click_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if click_resp.clicked() {
                *next_nav = Some(entry.url);
            }
            ui.add_space(4.0);
        }
    }

    fn render_native_bookmarks(&mut self, ui: &mut egui::Ui, next_nav: &mut Option<String>) {
        let colors = theme::palette(ui.ctx());

        ui.horizontal(|ui| {
            ui.heading(
                RichText::new("Bookmarks")
                    .size(24.0)
                    .color(colors.text)
                    .strong(),
            );
        });
        ui.add_space(12.0);

        // Filter
        ui.add_sized(
            [ui.available_width(), 34.0],
            egui::TextEdit::singleline(&mut self.internal_state.bookmarks_filter)
                .hint_text("Filter bookmarks…")
                .frame(
                    egui::Frame::new()
                        .fill(colors.surface)
                        .stroke(Stroke::new(1.0, colors.border))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(10, 6)),
                ),
        );
        ui.add_space(16.0);

        let query = self.internal_state.bookmarks_filter.trim().to_lowercase();
        let items: Vec<_> = self
            .profile()
            .bookmarks
            .iter()
            .filter(|entry| {
                if query.is_empty() {
                    true
                } else {
                    entry.title.to_lowercase().contains(&query)
                        || entry.url.to_lowercase().contains(&query)
                }
            })
            .cloned()
            .collect();

        if items.is_empty() {
            if query.is_empty() {
                let shortcut = if cfg!(target_os = "macos") {
                    "⌘D"
                } else {
                    "Ctrl+D"
                };
                components::empty_state(
                    ui,
                    "No bookmarks yet",
                    &format!("Bookmark web pages with {shortcut} to find them here quickly."),
                    None,
                );
            } else {
                components::empty_state(
                    ui,
                    "No matches",
                    "Try a different bookmark title or URL.",
                    None,
                );
            }
            return;
        }

        let mut to_remove = None;
        for entry in items {
            let row = egui::Frame::new()
                .fill(colors.surface)
                .stroke(Stroke::new(0.5, colors.border))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(texture) = self.favicons.get(&entry.url) {
                            ui.image((texture.id(), egui::vec2(16.0, 16.0)));
                        } else {
                            ui.label(RichText::new("★").size(14.0).color(colors.accent));
                        }
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(&entry.title)
                                    .size(13.0)
                                    .color(colors.text)
                                    .strong(),
                            );
                            ui.label(RichText::new(&entry.url).size(11.0).color(colors.muted));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(RichText::new("Remove").size(11.0).color(colors.danger))
                                .clicked()
                            {
                                to_remove = Some(entry.url.clone());
                            }
                        });
                    });
                });

            let click_resp = ui.interact(
                row.response.rect,
                ui.scope_id().with(("bookmark-item", &entry.url)),
                Sense::click(),
            );
            if click_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if click_resp.clicked() && to_remove.is_none() {
                *next_nav = Some(entry.url);
            }
            ui.add_space(4.0);
        }

        if let Some(url) = to_remove
            && let Some(idx) = self.profile().bookmarks.iter().position(|b| b.url == url)
        {
            self.apply(AppCommand::RemoveBookmark(idx));
        }
    }

    fn render_native_settings(&mut self, ui: &mut egui::Ui, _next_nav: &mut Option<String>) {
        let colors = theme::palette(ui.ctx());

        ui.heading(
            RichText::new("Settings")
                .size(24.0)
                .color(colors.text)
                .strong(),
        );
        ui.add_space(8.0);

        ui.label(
            RichText::new("Configure appearance, privacy, downloads, and engine behavior.")
                .size(13.0)
                .color(colors.muted),
        );
        ui.add_space(16.0);

        if ui
            .add(components::primary_button(
                ui,
                "Open browser settings panel",
            ))
            .clicked()
        {
            self.features.settings = true;
        }

        ui.add_space(24.0);

        // Quick settings overview cards
        egui::Frame::new()
            .fill(colors.surface)
            .stroke(Stroke::new(1.0, colors.border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(16, 14))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Appearance & Theme")
                            .size(15.0)
                            .color(colors.text)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!("Active theme: {}", self.profile().theme.name()))
                            .size(12.0)
                            .color(colors.muted),
                    );
                    ui.add_space(8.0);

                    ui.label(
                        RichText::new("Search Engine")
                            .size(15.0)
                            .color(colors.text)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!(
                            "Default search engine: {}",
                            self.profile().policy.search_engine.name()
                        ))
                        .size(12.0)
                        .color(colors.muted),
                    );
                    ui.add_space(8.0);

                    ui.label(
                        RichText::new("Engine Configuration")
                            .size(15.0)
                            .color(colors.text)
                            .strong(),
                    );
                    let mode = "Wega standalone engine (V8 + Blitz + Stylo + Skia)";
                    ui.label(RichText::new(mode).size(12.0).color(colors.muted));
                });
            });
    }

    fn render_native_about(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());

        ui.heading(
            RichText::new("About Wega")
                .size(24.0)
                .color(colors.text)
                .strong(),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(concat!("Version ", env!("CARGO_PKG_VERSION")))
                .size(14.0)
                .color(colors.accent)
                .strong(),
        );
        ui.add_space(16.0);

        egui::Frame::new()
            .fill(colors.surface)
            .stroke(Stroke::new(1.0, colors.border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(18, 16))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(
                            "Wega is a modern, privacy-focused desktop browser built in Rust.",
                        )
                        .size(13.0)
                        .color(colors.text),
                    );
                    ui.add_space(10.0);

                    egui::Grid::new("about-details-grid")
                        .spacing(egui::vec2(16.0, 8.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("Engine backend").strong());
                            let engine_label = "Wega standalone engine (V8 + Blitz + Stylo + Skia)";
                            ui.label(engine_label);
                            ui.end_row();

                            ui.label(RichText::new("Platform").strong());
                            ui.label(format!(
                                "{}-{}",
                                std::env::consts::OS,
                                std::env::consts::ARCH
                            ));
                            ui.end_row();

                            ui.label(RichText::new("UI toolkit").strong());
                            ui.label("egui 0.31 with winit & OpenGL");
                            ui.end_row();

                            ui.label(RichText::new("Build profile").strong());
                            ui.label(if cfg!(debug_assertions) {
                                "Development"
                            } else {
                                "Release"
                            });
                            ui.end_row();
                        });
                });
            });
    }

    fn render_native_not_found(
        &mut self,
        ui: &mut egui::Ui,
        page: &str,
        next_nav: &mut Option<String>,
    ) {
        let colors = theme::palette(ui.ctx());

        ui.heading(
            RichText::new("Page not found")
                .size(24.0)
                .color(colors.danger)
                .strong(),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!(
                "The internal address 'wega://{page}' is not recognized."
            ))
            .size(13.0)
            .color(colors.muted),
        );
        ui.add_space(16.0);
        if ui
            .add(components::primary_button(ui, "Go to New tab"))
            .clicked()
        {
            *next_nav = Some("wega://newtab".to_string());
        }
    }
}
