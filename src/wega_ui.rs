use crate::browser::Browser;
use wega::{engine_choice::EngineKind, engine_protocol::Command};

impl Browser {
    pub(crate) fn engine_picker(&mut self, ui: &mut egui::Ui) {
        let mut engine = self.tab().engine;
        let available = crate::wega_page::executable().is_some();
        let response=egui::ComboBox::from_id_salt("page-engine")
            .selected_text(engine.name()).width(145.0)
            .show_ui(ui,|ui|{
                ui.selectable_value(&mut engine,EngineKind::Servo,"Servo").on_hover_text("Recommended for everyday browsing, interactive sites, media and developer tools.");
                ui.add_enabled_ui(available,|ui|ui.selectable_value(&mut engine,EngineKind::Wega,"Wega (experimental)")).inner
                    .on_hover_text(if available{"Uses a separate Blitz/V8 worker. Reloads this tab; cookies and sign-in state are separate. Forms, media and page tools currently need Servo."}else{"Build/install the wega-engine worker with python3 scripts/build-wega-engine.py --debug to enable this choice."});
            }).response.on_hover_text("Engine for this tab. Changing it reloads the page and loses unsaved page state.");
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::ComboBox,
                true,
                format!("Engine: {}", self.tab().engine.name()),
            )
        });
        if engine != self.tab().engine {
            self.switch_engine(engine);
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
