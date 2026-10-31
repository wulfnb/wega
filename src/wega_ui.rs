use crate::browser::Browser;
use wega::engine_choice::EngineKind;

/// A CSS cursor keyword as the closest egui cursor.
fn cursor_icon(name: &str) -> egui::CursorIcon {
    use egui::CursorIcon as C;
    match name {
        "pointer" => C::PointingHand,
        "text" | "vertical-text" => C::Text,
        "wait" => C::Wait,
        "progress" => C::Progress,
        "help" => C::Help,
        "crosshair" => C::Crosshair,
        "move" | "all-scroll" => C::Move,
        "grab" => C::Grab,
        "grabbing" => C::Grabbing,
        "not-allowed" | "no-drop" => C::NotAllowed,
        "copy" => C::Copy,
        "alias" => C::Alias,
        "cell" => C::Cell,
        "context-menu" => C::ContextMenu,
        "zoom-in" => C::ZoomIn,
        "zoom-out" => C::ZoomOut,
        "none" => C::None,
        "col-resize" | "ew-resize" | "e-resize" | "w-resize" => C::ResizeHorizontal,
        "row-resize" | "ns-resize" | "n-resize" | "s-resize" => C::ResizeVertical,
        "nesw-resize" | "ne-resize" | "sw-resize" => C::ResizeNeSw,
        "nwse-resize" | "nw-resize" | "se-resize" => C::ResizeNwSe,
        _ => C::Default,
    }
}
fn modifier_bits(modifiers: egui::Modifiers) -> u8 {
    u8::from(modifiers.shift)
        | u8::from(modifiers.ctrl) << 1
        | u8::from(modifiers.alt) << 2
        | u8::from(modifiers.mac_cmd || (modifiers.command && !modifiers.ctrl)) << 3
}
fn button_index(button: egui::PointerButton) -> u8 {
    match button {
        egui::PointerButton::Primary => 0,
        egui::PointerButton::Middle => 1,
        egui::PointerButton::Secondary => 2,
        egui::PointerButton::Extra1 => 3,
        egui::PointerButton::Extra2 => 4,
    }
}

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
        let mut native = vec![];
        let mut navigated = false;
        let mut visits = vec![];
        let mut checks = vec![];
        for (index, tab) in self.session().tabs().iter().enumerate() {
            let Some(page) = &tab.wega else { continue };
            let mut state = page
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let expired = state.expire_requests();
            checks.extend(
                state
                    .take_page_checks()
                    .into_iter()
                    .map(|(request, dirty)| (request, tab.key, dirty)),
            );
            if let Some(visit) = state.visited.take() {
                visits.push(visit);
            }
            if index == active {
                notices.extend(state.notices.drain(..));
            }
            native.extend(
                state
                    .take_native_events(index == active)
                    .into_iter()
                    .map(|(requester, event)| (tab.key, requester, event)),
            );
            drop(state);
            page.cancel_queued(expired);
        }
        for (request, key, dirty) in checks {
            self.page_state_result(request, key, dirty);
        }
        for (owner, requester, event) in native {
            use wega::engine_protocol::ContentEvent;
            let Some(index) = self.session().tabs().iter().position(|tab| {
                tab.key == owner
                    && tab.wega.as_ref().is_some_and(|page| {
                        page.state
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .accepts_requester(&requester)
                    })
            }) else {
                continue;
            };
            match event {
                ContentEvent::Notice(message) => notices.push(message),
                ContentEvent::Compatibility(reason) => {
                    let page = self.session().tabs()[index].wega.as_ref().unwrap();
                    let mut state = page
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if state.accepts_requester(&requester) && !state.fallback_reported {
                        state.compatibility.get_or_insert(reason);
                    }
                }
                ContentEvent::Cursor(cursor) => {
                    let page = self.session().tabs()[index].wega.as_ref().unwrap();
                    let mut state = page
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if state.accepts_requester(&requester) {
                        state.cursor = Some(cursor);
                    }
                }
                event => {
                    let selected = self.tab().key;
                    self.select_tab(index);
                    let mut opened = false;
                    match event {
                        ContentEvent::Download { url, .. } => {
                            let cookies = self.tab().wega.as_ref().unwrap().download_cookies(&url);
                            let connections = self.download_ui.connections;
                            let message = match cookies.and_then(|cookies| {
                                self.start_download(&url, "", connections, cookies)
                            }) {
                                Ok(()) => format!("Downloading {url}"),
                                Err(error) => error,
                            };
                            self.notify(crate::ux::Severity::Info, message, None);
                        }
                        ContentEvent::ActivatedOpenWindow { url, .. } => {
                            match url::Url::parse(&url) {
                                Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => {
                                    self.add_tab(&url);
                                    opened = true;
                                }
                                _ => self.notify(
                                    crate::ux::Severity::Warning,
                                    "The page tried to open an unsupported address.",
                                    None,
                                ),
                            }
                        }
                        ContentEvent::TraverseUnhandled(delta) => {
                            for _ in 0..delta.unsigned_abs().min(8) {
                                self.traverse(delta > 0);
                            }
                        }
                        ContentEvent::Navigate(address) => {
                            navigated = true;
                            let current = self.current_url();
                            match url::Url::parse(&address) {
                                Ok(url) if wega::navigation::content_navigation_allowed(&url, current.as_ref()) && matches!(url.scheme(), "http" | "https" | "file") => self.navigate(&address),
                                _ => self.notify(crate::ux::Severity::Warning, "This navigation is unavailable in Wega. Use Servo for this page.", None),
                            }
                        }
                        _ => {}
                    }
                    if !opened
                        && let Some(previous) = self
                            .session()
                            .tabs()
                            .iter()
                            .position(|tab| tab.key == selected)
                    {
                        self.select_tab(previous);
                    }
                }
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
        if !self.modal() && self.profile().policy.wega_fallback && !navigated {
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
    }
    pub(crate) fn wega_viewport(&mut self, ui: &mut egui::Ui, rect: egui::Rect, outer: egui::Rect) {
        let failed = self.tab().wega.as_ref().is_some_and(|p| {
            p.state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .lifecycle
                .failed()
        });
        if failed {
            let mut restart = false;
            let mut servo = false;
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Wega engine stopped");
                    ui.label("This tab can be restarted without closing the browser.");
                    ui.horizontal(|ui| {
                        restart = ui
                            .add(crate::components::primary_button(ui, "Restart Wega"))
                            .clicked();
                        servo = ui.button("Reload in Servo").clicked();
                    });
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
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Image,
                true,
                "Wega experimental page. Page accessibility, media and iframes require Servo.",
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
        if let Some(texture) = &page.texture {
            ui.painter().rect_filled(rect, 0.0, egui::Color32::WHITE);
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            // Theme surface while the engine starts, instead of a white flash.
            let colors = crate::theme::palette(ui.ctx());
            ui.painter().rect_filled(rect, 0.0, colors.inset);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Loading Wega engine…",
                egui::FontId::proportional(16.0),
                colors.muted,
            );
        }
        // Pointer input in viewport CSS pixels: moves, presses, releases and wheel.
        let zoom = page.zoom;
        let local = |position: egui::Pos2| {
            let point = position - rect.min;
            (point.x / zoom, point.y / zoom)
        };
        let (events, pressed_buttons, hovered) = ui.input(|input| {
            let buttons = [
                egui::PointerButton::Primary,
                egui::PointerButton::Secondary,
                egui::PointerButton::Middle,
            ]
            .iter()
            .filter(|b| input.pointer.button_down(**b))
            .fold(0u8, |bits, b| {
                bits | match b {
                    egui::PointerButton::Primary => 1,
                    egui::PointerButton::Secondary => 2,
                    _ => 4,
                }
            });
            (input.events.clone(), buttons, input.pointer.hover_pos())
        });
        let inside = |position: egui::Pos2| rect.contains(position);
        let mut pressed = false;
        for event in events {
            match event {
                egui::Event::PointerMoved(position) if inside(position) || pressed_buttons != 0 => {
                    page.pointer("move", local(position), (0, pressed_buttons), 0, (0.0, 0.0));
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: down,
                    modifiers,
                } if inside(pos) || !down => {
                    pressed |= down;
                    page.pointer(
                        if down { "down" } else { "up" },
                        local(pos),
                        (button_index(button), pressed_buttons),
                        modifier_bits(modifiers),
                        (0.0, 0.0),
                    );
                }
                egui::Event::PointerGone => {
                    page.pointer("leave", (0.0, 0.0), (0, 0), 0, (0.0, 0.0))
                }
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } if hovered.is_some_and(inside) => {
                    let position = local(hovered.unwrap_or(rect.min));
                    let scale = match unit {
                        egui::MouseWheelUnit::Point => 1.0 / zoom,
                        egui::MouseWheelUnit::Line => 40.0,
                        egui::MouseWheelUnit::Page => rect.height() / zoom,
                    };
                    // egui's wheel delta is positive upwards; DOM deltas are positive downwards.
                    page.pointer(
                        "wheel",
                        position,
                        (0, pressed_buttons),
                        modifier_bits(modifiers),
                        ((-delta.x * scale) as f64, (-delta.y * scale) as f64),
                    );
                }
                _ => {}
            }
        }
        if response.hovered() {
            let cursor = page
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .cursor
                .clone();
            if let Some(cursor) = cursor {
                context.set_cursor_icon(cursor_icon(&cursor));
            }
        }
        self.wega_dialog(&context);
        if pressed || response.clicked() {
            self.chrome.page_focused = true;
            context.memory_mut(|m| m.surrender_focus(egui::Id::unique("address")));
        }
        ui.allocate_rect(outer, egui::Sense::hover());
    }
    /// A page dialog, answered back to the worker that waits for it.
    fn wega_dialog(&mut self, context: &egui::Context) {
        let Some(page) = self.wega_mut() else { return };
        let dialog = page
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .dialog
            .clone();
        let Some(dialog) = dialog else { return };
        if page.dialog_input_for != Some(dialog.id) {
            page.dialog_input_for = Some(dialog.id);
            page.dialog_input = if dialog.kind == "prompt" {
                dialog.default.clone()
            } else {
                String::new()
            };
        }
        let mut answer = None;
        egui::Window::new("This page says")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.label(match &dialog.requester.principal.origin {
                    wega::engine_protocol::authority::Origin::Tuple(origin) => origin.as_str(),
                    wega::engine_protocol::authority::Origin::Opaque => {
                        "Page with an opaque or unverified origin"
                    }
                });
                ui.label(&dialog.message);
                if dialog.kind == "prompt" {
                    ui.text_edit_singleline(&mut page.dialog_input);
                }
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        answer = Some(Some(match dialog.kind.as_str() {
                            "confirm" => "true".to_owned(),
                            "prompt" => page.dialog_input.clone(),
                            _ => String::new(),
                        }));
                    }
                    if dialog.kind != "alert" && ui.button("Cancel").clicked() {
                        answer = Some(None);
                    }
                });
            });
        if let Some(value) = answer {
            page.answer_dialog(dialog.context, dialog.id, value);
        }
    }
}
