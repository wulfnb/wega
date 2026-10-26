use crate::browser::Browser;
use wega::{engine_choice::EngineKind, engine_protocol::Command};

impl Browser {
    pub(crate) fn engine_picker(&mut self, ui: &mut egui::Ui) {
        let response = egui::ComboBox::from_id_salt("page-engine")
            .selected_text(self.tab().engine.name())
            .width(145.0)
            .show_ui(ui, |ui| self.engine_options(ui))
            .response;
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::ComboBox,
                true,
                format!("Engine: {}", self.tab().engine.name()),
            )
        });
    }
    pub(crate) fn engine_badge(&mut self, ui: &mut egui::Ui) {
        let name = if self.is_wega() { "Wega" } else { "Servo" };
        ui.menu_button(name, |ui| self.engine_options(ui))
            .response
            .on_hover_text("Page engine and preference for this site");
    }
    fn engine_options(&mut self, ui: &mut egui::Ui) {
        let mut engine = self.tab().engine;
        ui.selectable_value(&mut engine, EngineKind::Servo, "Servo")
            .on_hover_text("Full browser engine for interactive apps, media and page tools.");
        ui.add_enabled_ui(crate::wega_page::executable().is_some(), |ui| {
            ui.selectable_value(&mut engine, EngineKind::Wega, "Wega (experimental)")
        });
        let mut policy = self.profile().policy.clone();
        let mut changed = false;
        if let Some(key) = self
            .current_url()
            .and_then(|url| wega::engine_choice::site_key(&url))
        {
            ui.separator();
            ui.label(&key);
            let mut remember = policy.site_engines.contains_key(&key);
            if ui
                .checkbox(&mut remember, "Remember engine for this site")
                .changed()
            {
                if remember {
                    policy.site_engines.insert(key, self.tab().engine);
                } else {
                    policy.site_engines.remove(&key);
                }
                changed = true;
            }
        }
        changed |= ui
            .checkbox(
                &mut policy.wega_rollout,
                "Use Wega for validated static sites",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut policy.wega_fallback,
                "Use Servo when Wega cannot run a page",
            )
            .on_hover_text("Edited pages require confirmation before their state is discarded.")
            .changed();
        if changed {
            self.apply(wega::application::Command::Policy(policy));
        }
        if engine != self.tab().engine {
            self.switch_engine(engine);
            ui.close();
        }
    }
    pub(crate) fn pump_wega(&mut self) {
        let active = self.session().active();
        let mut notices = vec![];
        let mut navigations = vec![];
        let mut visits = vec![];
        for (index, tab) in self.session().tabs().iter().enumerate() {
            let Some(page) = &tab.wega else { continue };
            let mut state = page
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(visit) = state.visited.take() {
                visits.push(visit);
            }
            if index == active {
                notices.extend(state.notices.drain(..));
                navigations.extend(state.navigations.drain(..));
            }
        }
        for message in notices {
            self.notify(crate::ux::Severity::Warning, message, None);
        }
        for (url, title) in visits {
            self.apply(wega::application::Command::Visit(
                crate::profile::Bookmark {
                    url,
                    title,
                    visited_at: crate::profile::visit_time(),
                    visit_count: 0,
                },
            ));
        }
        // Handle one compatibility report only after any existing prompt is resolved.
        // Initial unedited pages can reload automatically; user input always keeps
        // the existing conservative page-state confirmation path.
        if !self.modal() && self.profile().policy.wega_fallback && navigations.is_empty() {
            let fallback = self.tab().wega.as_ref().and_then(|page| {
                let mut state = page
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.compatibility.take().map(|reason| {
                    state.fallback_reported = true;
                    (reason, state.interacted)
                })
            });
            if let Some((reason, interacted)) = fallback {
                self.notify(
                    crate::ux::Severity::Warning,
                    format!("This page needs Servo: {reason}"),
                    None,
                );
                if interacted {
                    self.confirm_page_action(crate::page_safety::Action::Fallback(self.tab().key));
                } else if let Some(url) = self.current_url() {
                    let key = wega::engine_choice::site_key(&url);
                    if self.replace_engine_confirmed(EngineKind::Servo, url, false) {
                        self.mark_fallback(key);
                    }
                }
            }
        }
        if let Some(address) = navigations.last() {
            let current = self.current_url();
            match url::Url::parse(address) {
                Ok(url)
                    if wega::navigation::content_navigation_allowed(&url, current.as_ref())
                        && matches!(url.scheme(), "http" | "https" | "file") =>
                {
                    self.navigate(address)
                }
                _ => self.notify(
                    crate::ux::Severity::Warning,
                    "This navigation is unavailable in Wega. Use Servo for this page.",
                    None,
                ),
            }
        }
    }
    pub(crate) fn wega_viewport(&mut self, ui: &mut egui::Ui, rect: egui::Rect, outer: egui::Rect) {
        let failed = self.tab().wega.as_ref().is_some_and(|p| {
            p.state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .failed
        });
        if failed {
            let mut restart = false;
            let mut servo = false;
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Wega engine stopped");
                    ui.label("This tab can be restarted without closing the browser.");
                    restart = ui.button("Restart Wega").clicked();
                    servo = ui.button("Reload in Servo").clicked();
                })
            });
            if restart {
                self.switch_engine(EngineKind::Wega);
            } else if servo {
                self.switch_engine(EngineKind::Servo);
            }
            return;
        }
        let scale = ui.ctx().pixels_per_point();
        let mut width = (rect.width() * scale).round().max(1.0) as u32;
        let mut height = (rect.height() * scale).round().max(1.0) as u32;
        // Bound pixel storage; scale down very large displays instead of overflowing IPC.
        let factor = ((wega::engine_protocol::MAX_PIXELS as f64 / (width as f64 * height as f64))
            .min(1.0))
        .sqrt() as f32;
        width = (width as f32 * factor).floor().max(1.0) as u32;
        height = (height as f32 * factor).floor().max(1.0) as u32;
        let dark = self
            .profile()
            .theme
            .dark(self.window.theme() != Some(winit::window::Theme::Light));
        let context = ui.ctx().clone();
        let response = ui.allocate_rect(rect, egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Image,
                true,
                "Wega experimental page. Page accessibility, forms and media require Servo.",
            )
        });
        let Some(page) = self.wega_mut() else { return };
        let _ = page.resize(width, height, scale * factor, dark);
        let frame = page
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .frame
            .take();
        if let Some((width, height, rgba)) = frame {
            let image =
                egui::ColorImage::from_rgba_premultiplied([width as usize, height as usize], &rgba);
            if let Some(texture) = &mut page.texture {
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                page.texture =
                    Some(context.load_texture("wega-page", image, egui::TextureOptions::LINEAR));
            }
        }
        ui.painter().rect_filled(rect, 0.0, egui::Color32::WHITE);
        if let Some(texture) = &page.texture {
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Loading Wega engine…",
                egui::FontId::proportional(18.0),
                egui::Color32::DARK_GRAY,
            );
        }
        let clicked = response.clicked();
        if clicked && let Some(position) = response.interact_pointer_pos() {
            let local = position - rect.min;
            let _ = page.send(Command::Click {
                x: local.x / page.zoom,
                y: local.y / page.zoom,
            });
        }
        if response.hovered() {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            if delta != egui::Vec2::ZERO {
                let _ = page.send(Command::Scroll {
                    x: -(delta.x / page.zoom) as f64,
                    y: -(delta.y / page.zoom) as f64,
                    pointer_x: (response.hover_pos().unwrap_or(rect.center()).x - rect.min.x)
                        / page.zoom,
                    pointer_y: (response.hover_pos().unwrap_or(rect.center()).y - rect.min.y)
                        / page.zoom,
                });
            }
        }
        if clicked {
            self.chrome.page_focused = true;
            context.memory_mut(|m| m.surrender_focus(egui::Id::unique("address")));
        }
        ui.allocate_rect(outer, egui::Sense::hover());
    }
}
