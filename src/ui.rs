use crate::{
    browser::Browser,
    profile::{self, TabLayout},
};
use egui::{Color32, Frame, Id, RichText, Stroke, Ui};

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;

pub fn style(ctx: &egui::Context) {
    let colors = crate::theme::palette(ctx);
    let mode = if colors.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    let mut style = (*ctx.style_of(mode)).clone();
    style.visuals = if colors.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    style.visuals.panel_fill = colors.chrome;
    style.visuals.window_fill = colors.inset;
    style.visuals.window_stroke = Stroke::new(1.0, colors.border);
    style.visuals.window_corner_radius = egui::CornerRadius::same(12);
    style.visuals.menu_corner_radius = egui::CornerRadius::same(10);
    style.visuals.extreme_bg_color = colors.inset;
    style.visuals.text_edit_bg_color = Some(colors.chrome);
    style.visuals.override_text_color = Some(colors.text);
    style.visuals.selection.bg_fill = colors.selected;
    style.visuals.selection.stroke = Stroke::new(1.0, colors.accent);
    for (widget, fill, foreground) in [
        (
            &mut style.visuals.widgets.noninteractive,
            colors.chrome,
            colors.muted,
        ),
        (
            &mut style.visuals.widgets.inactive,
            colors.surface,
            colors.text,
        ),
        (
            &mut style.visuals.widgets.hovered,
            colors.hover,
            colors.text,
        ),
        (
            &mut style.visuals.widgets.active,
            colors.selected,
            colors.accent,
        ),
        (
            &mut style.visuals.widgets.open,
            colors.selected,
            colors.text,
        ),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = Stroke::NONE;
        widget.fg_stroke = Stroke::new(1.0, foreground);
        widget.corner_radius = egui::CornerRadius::same(7);
        widget.expansion = 0.0;
    }
    style.visuals.faint_bg_color = colors.surface;
    style.visuals.code_bg_color = colors.inset;
    style.visuals.hyperlink_color = colors.accent;
    style.visuals.warn_fg_color = colors.amber;
    style.visuals.error_fg_color = colors.danger;
    style.spacing.button_padding = egui::vec2(10.0, 7.0);
    style.spacing.item_spacing = egui::vec2(7.0, 7.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
    ctx.set_theme(mode);
    ctx.set_style_of(mode, style);
}

fn paint_icon(painter: &egui::Painter, center: egui::Pos2, symbol: &str, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let point = |x: f32, y: f32| center + egui::vec2(x, y);
    let line = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            points.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };
    match symbol {
        "←" | "→" => {
            let direction = if symbol == "←" { -1.0 } else { 1.0 };
            line(&[(-7.0, 0.0), (7.0, 0.0)]);
            line(&[
                (direction * 2.0, -5.0),
                (direction * 7.0, 0.0),
                (direction * 2.0, 5.0),
            ]);
        }
        "↓" => {
            line(&[(0.0, -7.0), (0.0, 5.0)]);
            line(&[(-5.0, 0.0), (0.0, 5.0), (5.0, 0.0)]);
            line(&[(-7.0, 8.0), (7.0, 8.0)]);
        }
        "↻" => {
            let radius = 7.5;
            let end = 330.0_f32.to_radians();
            let points = (0..=32)
                .map(|i| {
                    let angle = (40.0 + i as f32 * 290.0 / 32.0).to_radians();
                    point(angle.cos() * radius, angle.sin() * radius)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            let tip = point(end.cos() * radius, end.sin() * radius);
            let tangent = egui::vec2(-end.sin(), end.cos());
            let normal = egui::vec2(end.cos(), end.sin());
            painter.add(egui::Shape::line(
                vec![
                    tip - tangent * 4.5 + normal * 3.0,
                    tip,
                    tip - tangent * 4.5 - normal * 3.0,
                ],
                stroke,
            ));
        }
        "×" => {
            line(&[(-5.0, -5.0), (5.0, 5.0)]);
            line(&[(-5.0, 5.0), (5.0, -5.0)]);
        }
        "☆" | "★" => {
            let points = (0..10)
                .map(|i| {
                    let angle = (i as f32 * 36.0 - 90.0).to_radians();
                    let radius = if i % 2 == 0 { 9.0 } else { 4.0 };
                    point(angle.cos() * radius, angle.sin() * radius)
                })
                .collect();
            painter.add(egui::Shape::Path(egui::epaint::PathShape {
                points,
                closed: true,
                fill: if symbol == "★" {
                    color
                } else {
                    Color32::TRANSPARENT
                },
                stroke: stroke.into(),
            }));
        }
        "☰" => {
            for y in [-5.0, 0.0, 5.0] {
                line(&[(-7.0, y), (7.0, y)]);
            }
        }
        "▤" | "▥" | "⌘" => {
            painter.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(18.0, 16.0)),
                2,
                stroke,
                egui::StrokeKind::Inside,
            );
            if symbol == "▥" {
                line(&[(-3.0, -7.0), (-3.0, 7.0)]);
            } else if symbol == "▤" {
                line(&[(-8.0, -2.0), (8.0, -2.0)]);
            } else {
                line(&[(-5.0, -3.0), (-2.0, 0.0), (-5.0, 3.0)]);
                line(&[(1.0, 4.0), (5.0, 4.0)]);
            }
        }
        "◷" => {
            painter.circle_stroke(center, 8.0, stroke);
            line(&[(0.0, -5.0), (0.0, 0.0), (4.0, 2.0)]);
        }
        "⋯" => {
            for x in [-6.0, 0.0, 6.0] {
                painter.circle_filled(point(x, 0.0), 1.8, color);
            }
        }
        "lock" => {
            line(&[
                (-4.5, -1.0),
                (-4.5, -5.0),
                (-2.5, -8.0),
                (2.5, -8.0),
                (4.5, -5.0),
                (4.5, -1.0),
            ]);
            painter.rect_stroke(
                egui::Rect::from_min_max(point(-7.0, -1.0), point(7.0, 9.0)),
                2,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(&[(0.0, 3.0), (0.0, 5.5)]);
        }
        "warning" => {
            line(&[(0.0, -8.0), (9.0, 8.0), (-9.0, 8.0), (0.0, -8.0)]);
            line(&[(0.0, -2.0), (0.0, 2.0)]);
            painter.circle_filled(point(0.0, 5.0), 0.9, color);
        }
        "containers" => {
            for (x, y) in [(-8.0, -7.0), (1.0, -7.0), (-8.0, 2.0), (1.0, 2.0)] {
                painter.rect_stroke(
                    egui::Rect::from_min_size(point(x, y), egui::vec2(7.0, 7.0)),
                    1,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
        "container-dot" => {
            painter.circle_filled(center, 4.0, color);
        }
        _ => {
            painter.circle_stroke(center, 8.0, stroke);
            painter.circle_filled(center, 3.0, color);
        }
    }
}

// Paint symbols directly instead of relying on emoji/system font fallback.
// Atoms keep the icon inside the button/menu's layout and accessible text label.
pub(crate) fn icon_atom(symbol: &'static str, color: Option<Color32>) -> egui::Atom<'static> {
    egui::Atom::paint(egui::vec2(20.0, 20.0), move |ui, args| {
        paint_icon(
            ui.painter(),
            args.rect.center(),
            symbol,
            color.unwrap_or(args.fallback_text_color),
        );
    })
}

fn site_info_button(ui: &mut Ui, protocol: &str) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let button = match protocol {
        "https" => egui::Button::new(icon_atom("lock", None)),
        "http" => egui::Button::new((
            icon_atom("warning", Some(colors.amber)),
            RichText::new("Not secure").color(colors.amber),
        )),
        "wega" => egui::Button::new("Wega"),
        "file" => egui::Button::new("File"),
        _ => egui::Button::new("Site"),
    };
    let response = ui.add(button).on_hover_text("Connection and site data");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            ui.is_enabled(),
            "Connection and site data",
        )
    });
    response
}

fn brand_icon(ui: &mut Ui) {
    let id = egui::Id::unique("browser-brand-icon");
    let cached = ui
        .ctx()
        .data(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = cached.or_else(|| {
        let image = image::load_from_memory(include_bytes!("../assets/icons/browser-icon-256.png"))
            .ok()?
            .into_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let texture = ui.ctx().load_texture(
            "browser-brand-icon",
            egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
            egui::TextureOptions::LINEAR,
        );
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, texture.clone()));
        Some(texture)
    });
    if let Some(texture) = texture {
        ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::vec2(28.0, 28.0)));
    }
}

pub(crate) fn icon(ui: &mut Ui, symbol: &str, help: &str) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    if response.hovered() && ui.is_enabled() {
        ui.painter().rect_filled(rect, 8, colors.hover);
    }
    paint_icon(
        ui.painter(),
        rect.center(),
        symbol,
        if ui.is_enabled() {
            colors.text
        } else {
            colors.muted.gamma_multiply(0.4)
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), help));
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            7,
            Stroke::new(2.0, colors.accent),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_text(help)
}

fn shorten(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let mut short: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        short.push('…');
    }
    short
}

// Mask only the corners after Servo's rectangular GPU blit.
pub(crate) fn page_corners(painter: &egui::Painter, rect: egui::Rect) {
    let colors = crate::theme::palette(painter.ctx());
    if colors.glass {
        use egui_glow::glow::{self, HasContext};
        let rgba = colors.chrome.to_array().map(|v| v as f32 / 255.0);
        painter.add(egui::PaintCallback {
            rect,
            callback: std::sync::Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                let gl = painter.gl();
                let viewport = info.viewport_in_pixels();
                let radius = (12.0 * info.pixels_per_point)
                    .round()
                    .min(viewport.width_px as f32 / 2.0)
                    .min(viewport.height_px as f32 / 2.0) as i32;
                // Clear rather than alpha-blend: the website is opaque, while the
                // rounded corner belongs to the translucent browser chrome.
                unsafe {
                    let enabled = gl.is_enabled(glow::SCISSOR_TEST);
                    let mut scissor = [0; 4];
                    let mut clear = [0.0; 4];
                    gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut scissor);
                    gl.get_parameter_f32_slice(glow::COLOR_CLEAR_VALUE, &mut clear);
                    gl.enable(glow::SCISSOR_TEST);
                    gl.clear_color(rgba[0], rgba[1], rgba[2], rgba[3]);
                    for row in 0..radius {
                        let distance = radius as f32 - row as f32 - 0.5;
                        let width = (radius as f32
                            - ((radius * radius) as f32 - distance * distance).sqrt())
                        .ceil() as i32;
                        for x in [
                            viewport.left_px,
                            viewport.left_px + viewport.width_px - width,
                        ] {
                            for y in [
                                viewport.from_bottom_px + row,
                                viewport.from_bottom_px + viewport.height_px - 1 - row,
                            ] {
                                gl.scissor(x, y, width, 1);
                                gl.clear(glow::COLOR_BUFFER_BIT);
                            }
                        }
                    }
                    gl.scissor(scissor[0], scissor[1], scissor[2], scissor[3]);
                    if !enabled {
                        gl.disable(glow::SCISSOR_TEST);
                    }
                    gl.clear_color(clear[0], clear[1], clear[2], clear[3]);
                }
            })),
        });
        return;
    }
    let radius = 12.0_f32.min(rect.width() / 2.0).min(rect.height() / 2.0);
    for (corner, sx, sy) in [
        (rect.left_top(), 1.0, 1.0),
        (rect.right_top(), -1.0, 1.0),
        (rect.left_bottom(), 1.0, -1.0),
        (rect.right_bottom(), -1.0, -1.0),
    ] {
        let mut points = vec![corner];
        for i in 0..=12 {
            let angle = i as f32 / 12.0 * std::f32::consts::FRAC_PI_2;
            points.push(
                corner
                    + egui::vec2(
                        sx * radius * (1.0 - angle.cos()),
                        sy * radius * (1.0 - angle.sin()),
                    ),
            );
        }
        painter.add(egui::Shape::convex_polygon(
            points,
            colors.chrome,
            Stroke::NONE,
        ));
    }
}

#[derive(Clone, Copy)]
enum TabAction {
    Select(usize),
    Close(usize),
    Discard(usize),
    Move(usize, usize),
    New,
    Layout,
}

impl Browser {
    pub fn ui(&mut self, ui: &mut Ui) {
        let colors = crate::theme::palette(ui.ctx());
        if self.status.is_some() {
            self.chrome.notice = None;
        }
        if !self.chrome.expanded_notice
            && self
                .chrome
                .notice
                .as_ref()
                .is_some_and(crate::ux::Notice::expired)
        {
            self.chrome.notice = None;
        }
        if !self.chrome.expanded_notice
            && let Some(notice) = &self.chrome.notice
            && matches!(
                notice.severity,
                crate::ux::Severity::Info | crate::ux::Severity::Success
            )
        {
            ui.ctx().request_repaint_after(
                std::time::Duration::from_secs(8).saturating_sub(notice.created.elapsed()),
            );
        }
        let compact = crate::ux::compact(ui.ctx().content_rect().width());
        if !self.fullscreen {
            egui::Panel::top("navigation")
                .frame(
                    Frame::new()
                        .fill(colors.chrome)
                        .inner_margin(egui::Margin::symmetric(10, 8)),
                )
                .show(ui, |ui| {
                    if self.profile().layout == TabLayout::Top || compact {
                        egui::ScrollArea::horizontal()
                            .id_salt("top_tabs")
                            .show(ui, |ui| {
                                ui.horizontal(|ui| self.tab_strip(ui, false));
                            });
                        ui.add_space(6.0);
                    }
                    self.navigation(ui);
                });
            if self.profile().layout == TabLayout::Side && !compact {
                let panel = egui::Panel::left("tab_sidebar")
                    .resizable(true)
                    .default_size(self.profile().sidebar_width)
                    .min_size(180.0)
                    .max_size(380.0)
                    .frame(Frame::new().fill(colors.chrome).inner_margin(12));
                let shown = panel.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        brand_icon(ui);
                        ui.label(RichText::new(crate::brand::NAME).strong().size(15.0));
                    });
                    ui.add_space(6.0);
                    let height = (ui.available_height() - 75.0).max(80.0);
                    egui::ScrollArea::vertical()
                        .max_height(height)
                        .id_salt("side_tabs")
                        .show(ui, |ui| self.tab_strip(ui, true));
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        ui.horizontal(|ui| {
                            if icon(ui, "☰", "Switch to tabs on top").clicked() {
                                self.toggle_layout();
                            }
                            if icon(ui, "◷", "History").clicked() {
                                self.chrome.show_history = !self.chrome.show_history;
                                self.chrome.show_bookmarks = false;
                                self.chrome.library_query.clear();
                            }
                            if icon(ui, "⌘", "Developer toolbox (F12)").clicked() {
                                self.toggle_developer_tools();
                            }
                        });
                        ui.label(
                            RichText::new(format!(
                                "{} {}",
                                self.session().tabs().len(),
                                if self.session().tabs().len() == 1 {
                                    "tab"
                                } else {
                                    "tabs"
                                }
                            ))
                            .size(11.0)
                            .color(colors.muted),
                        );
                    });
                });
                let width = shown.response.rect.width();
                if (self.profile().sidebar_width - width).abs() > 1.0 {
                    self.apply(wega::application::Command::SidebarWidth(width));
                    self.changed();
                }
            }
            egui::Panel::bottom("page_status")
                .frame(
                    Frame::new()
                        .fill(colors.chrome)
                        .inner_margin(egui::Margin::symmetric(12, 4)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let reserved=if self.profile().policy.show_performance {if self.is_wega() {740.0} else {530.0}} else {370.0};
                        let status_width = (ui.available_width() - reserved).max(0.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(status_width, 20.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                if let Some(status) = self.status.clone().or_else(|| {
                                    self.chrome.notice.as_ref().map(|n| n.message.clone())
                                }) {
                                    let color =
                                        self.chrome.notice.as_ref().map_or(colors.danger, |n| {
                                            match n.severity {
                                                crate::ux::Severity::Success => colors.accent,
                                                crate::ux::Severity::Info => colors.text,
                                                crate::ux::Severity::Warning => colors.amber,
                                                crate::ux::Severity::Error => colors.danger,
                                            }
                                        });
                                    ui.add_sized(
                                        egui::vec2((ui.available_width() - 100.0).max(0.0), 20.0),
                                        egui::Label::new(
                                            RichText::new(&status).size(12.0).color(color),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(status);
                                    if ui.small_button("Details").clicked() {
                                        self.chrome.expanded_notice = true;
                                    }
                                    if ui.small_button("×").clicked() {
                                        self.status = None;
                                        self.chrome.notice = None;
                                    }
                                } else {
                                    let status = self.tab().view().and_then(|view|view.status_text()).unwrap_or_else(|| {
                                        if self.tab().loading(){"Loading page…".into()}else if self.is_wega(){"Experimental: forms, media and page tools need Servo".into()}else{String::new()}
                                    });
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(status).size(11.0).color(colors.muted),
                                        )
                                        .truncate(),
                                    );
                                }
                            },
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            self.engine_picker(ui);
                            if ui.small_button("+").clicked() {
                                self.zoom(1.1);
                            }
                            if ui
                                .small_button(format!(
                                    "{}%",
                                    (self.page_zoom() * 100.0).round()
                                ))
                                .clicked()
                            {
                                self.reset_zoom();
                            }
                            if ui.small_button("−").clicked() {
                                self.zoom(1.0 / 1.1);
                            }
                            if self.profile().policy.show_performance {
                                ui.add_space(10.0);
                                ui.label(
                                    RichText::new(self.resources.label())
                                        .size(11.0)
                                        .color(colors.muted),
                                )
                                .on_hover_text(self.resources.description());
                                if let Some(page)=self.tab().wega.as_ref() {
                                    let state=page.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                                    let rss=state.rss_bytes.map_or_else(||"—".into(),|bytes|format!("{} MiB",bytes/1_048_576));
                                    ui.label(format!("Tab RAM {rss} · JS {:.1} MiB",state.js_heap as f64/1_048_576.0)).on_hover_text("Tab RAM is the worker's current resident memory. JS is its V8 heap, a subset of that memory. Shared pages may be counted by both browser and worker.");
                                }
                            }
                        });
                    });
                });
        }
        if self.chrome.show_console {
            self.devtools(ui);
        }
        self.collections(ui);
        egui::CentralPanel::default()
            .frame(Frame::new().fill(colors.chrome))
            .show(ui, |ui| self.viewport(ui));
        let ctx = ui.ctx().clone();
        self.dialogs(&ctx);
        self.containers_manager(&ctx);
        if !self.modal() {
            self.finish_dialog(&ctx);
        }
        self.downloads_window(&ctx);
        if self.chrome.expanded_notice {
            let mut open = true;
            egui::Window::new("Browser message")
                .open(&mut open)
                .default_width(500.0)
                .show(&ctx, |ui| {
                    let message = self
                        .status
                        .clone()
                        .or_else(|| self.chrome.notice.as_ref().map(|n| n.message.clone()))
                        .unwrap_or_else(|| "No current messages.".into());
                    ui.add(egui::Label::new(&message).wrap().selectable(true));
                    if let Some(path) = self.chrome.notice.as_ref().and_then(|n| n.path.clone())
                        && ui.button("Show in folder").clicked()
                        && let Err(error) = crate::ux::open_path(&path, true)
                    {
                        self.status = Some(error);
                    }
                    if ui.button("Copy message").clicked() {
                        ctx.copy_text(message);
                    }
                });
            self.chrome.expanded_notice = open;
        }
    }

    fn navigation(&mut self, ui: &mut Ui) {
        let colors = crate::theme::palette(ui.ctx());
        ui.horizontal(|ui| {
            if icon(
                ui,
                if self.profile().layout == TabLayout::Side {
                    "▤"
                } else {
                    "▥"
                },
                "Switch top / sidebar tabs (Cmd/Ctrl+Shift+L)",
            )
            .clicked()
            {
                self.toggle_layout();
            }
            if ui
                .add_enabled_ui(self.can_go_back(), |ui| {
                    icon(ui, "←", "Back (Alt+Left)")
                })
                .inner
                .clicked()
            {
                self.traverse(false);
            }
            if ui
                .add_enabled_ui(self.can_go_forward(), |ui| {
                    icon(ui, "→", "Forward (Alt+Right)")
                })
                .inner
                .clicked()
            {
                self.traverse(true);
            }
            let reload = icon(ui, "↻", "Reload (Cmd/Ctrl+R)");
            self.chrome.reload_rect = reload.rect;
            if reload.clicked() {
                self.reload();
            }
            let url = self.current_url();
            let protocol = url.as_ref().map(|u| u.scheme()).unwrap_or("");
            let security = site_info_button(ui, protocol);
            ui.ctx().data_mut(|data| data.insert_temp(Id::unique("site-panel-anchor"), security.rect.left_bottom()));
            if security.clicked() { self.refresh_site_data(); self.features.site_info = !self.features.site_info; }
            self.engine_badge(ui);
            self.containers_menu(ui);
            let width = (ui.available_width() - 126.0).max(100.0);
            let hint = match self.profile().policy.search_engine {
                wega::model::SearchEngine::Custom => "Search or enter address".to_owned(),
                engine => format!("Search {} or enter address", engine.name()),
            };
            let editing = self.chrome.focus_address || ui.memory(|memory| memory.has_focus(Id::unique("address")));
            let mut address_layout = |ui: &Ui, buffer: &dyn egui::TextBuffer, _: f32| {
                let text=buffer.as_str();
                let mut job=egui::text::LayoutJob::default();
                let font=egui::TextStyle::Body.resolve(ui.style());
                if let Some(range)=(!editing).then(|| wega::model::address_host_range(text)).flatten() {
                    for (part,color) in [(&text[..range.start],colors.muted),(&text[range.clone()],colors.text),(&text[range.end..],colors.muted)] {
                        job.append(part,0.0,egui::TextFormat{font_id:font.clone(),color,..Default::default()});
                    }
                } else {job.append(text,0.0,egui::TextFormat{font_id:font,color:colors.text,..Default::default()});}
                job.wrap.max_width=f32::INFINITY;
                ui.fonts_mut(|fonts| fonts.layout_job(job))
            };
            let response = ui.add_sized(
                [width, 34.0],
                egui::TextEdit::singleline(&mut self.chrome.address)
                    .id(Id::unique("address"))
                    .hint_text(hint)
                    .layouter(&mut address_layout)
                    .frame(
                        Frame::new()
                            .fill(colors.inset)
                            .stroke(Stroke::new(1.0, colors.border))
                            .corner_radius(17)
                            .inner_margin(egui::Margin::symmetric(13, 7)),
                    ),
            );
            if self.chrome.focus_address {
                response.request_focus();
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(self.chrome.address.chars().count()),
                        )));
                    state.store(ui.ctx(), response.id);
                }
                self.chrome.focus_address = false;
            }
            if response.has_focus() {
                self.chrome.page_focused = false;
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let address = self.chrome.address.clone();
                self.navigate(&address);
            }
            let starred = self.profile()
                .bookmarks
                .iter()
                .any(|b| b.url == self.tab().url());
            if icon(
                ui,
                if starred { "★" } else { "☆" },
                "Bookmark page (Cmd/Ctrl+D)",
            )
            .clicked()
            {
                self.bookmark();
            }
            if icon(ui,"↓","Downloads").clicked(){self.download_ui.open = !self.download_ui.open;}
            let menu = icon(ui, "⋯", "Menu");
            egui::Popup::menu(&menu).show(|ui| {
                if ui
                    .add(
                        egui::Button::new("New tab").shortcut_text(if cfg!(target_os = "macos") {
                            "Cmd+T"
                        } else {
                            "Ctrl+T"
                        }),
                    )
                    .clicked()
                {
                    self.new_tab();
                    ui.close();
                }
                if ui.button("Reopen closed tab").clicked() {
                    if let Some(url) = self.chrome.closed_tabs.pop() {
                        self.add_tab(&url);
                    }
                    ui.close();
                }
                ui.separator();
                if ui.button("Tabs on top").clicked() {
                    self.apply(wega::application::Command::Layout(TabLayout::Top));
                    self.changed();
                    ui.close();
                }
                if ui.button("Tabs in sidebar").clicked() {
                    self.apply(wega::application::Command::Layout(TabLayout::Side));
                    self.changed();
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Appearance", |ui| {
                    for theme in crate::theme::Theme::ALL {
                        if self.native_theme.is_none() && matches!(theme,crate::theme::Theme::GlassLight | crate::theme::Theme::GlassDark) { continue; }
                        if ui
                            .selectable_label(self.profile().theme == theme, theme.name())
                            .clicked()
                        {
                            self.set_theme(ui.ctx(), theme);
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.weak(if self.native_theme.is_some() {
                        "Glass uses macOS vibrancy. Auto follows system appearance."
                    } else {
                        "Glass uses an opaque fallback on this platform."
                    });
                });
                ui.separator();
                if ui.button("Bookmarks").clicked() {
                    self.chrome.show_bookmarks = true;
                    self.chrome.show_history = false;
                    self.chrome.library_query.clear();
                    ui.close();
                }
                if ui.button("History").clicked() {
                    self.chrome.show_history = true;
                    self.chrome.show_bookmarks = false;
                    self.chrome.library_query.clear();
                    ui.close();
                }
                if ui.button("Downloads").clicked(){self.download_ui.open=true;ui.close();}
                if ui.button("Download current URL…").clicked(){let url=self.tab().url();self.offer_download(&url,"");ui.close();}
                if ui
                    .add(egui::Button::new("Developer tools").shortcut_text("F12"))
                    .clicked()
                {
                    self.toggle_developer_tools();
                    ui.close();
                }
                if ui.button("Browser settings").clicked() {self.features.settings=true;ui.close();}
                if ui.button("Open browser logs").clicked() {if let Err(error)=crate::ux::open_path(&self.profile_dir.join("logs"),false) {self.status=Some(error);}ui.close();}
                if ui.button("About Wega").clicked(){self.navigate("wega://about");ui.close();}
                if ui.button("Site information").clicked() {self.refresh_site_data();self.features.site_info=true;ui.close();}
                if ui.button("Save page as image").clicked() {self.screenshot_page();ui.close();}
                if ui.add_enabled(!self.is_wega(),egui::Button::new("Reader view")).clicked() {self.view().evaluate_javascript(include_str!("reader.js"), |_|{});ui.close();}
                if ui.add_enabled(!self.is_wega(),egui::Button::new("Toggle page dark style")).clicked() {self.view().evaluate_javascript("(()=>{let s=document.getElementById('__wega_dark');if(s){s.remove();return} s=document.createElement('style');s.id='__wega_dark';s.textContent='html{color-scheme:dark!important;background:#171717!important;color:#eee!important}body,main,article,section,header,footer,nav{background-color:#171717!important;color:#eee!important}a{color:#a9caff!important}';document.head.appendChild(s)})()", |_|{});ui.close();}
                if ui.button("Find in page").clicked() {self.features.find_open=true;ui.close();}
                if ui.button("Reload without cache").clicked() {self.reload_without_cache();ui.close();}
                if ui.button("Home").clicked() {
                    self.navigate(profile::HOME);
                    ui.close();
                }
                ui.separator();
                ui.label(
                    RichText::new("Wega · Rust + Servo 0.7.0 + egui")
                        .small()
                        .color(colors.muted),
                );
            });
        });
    }

    fn tab_strip(&mut self, ui: &mut Ui, vertical: bool) {
        let colors = crate::theme::palette(ui.ctx());
        self.chrome.tab_rects.clear();
        ui.spacing_mut().button_padding = egui::vec2(6.0, 4.0);
        let mut action = None;
        for index in 0..self.session().tabs().len() {
            let selected = self.session().active() == index;
            let title = self.session().tabs()[index].title();
            let tab = &self.session().tabs()[index];
            let id = tab.key;
            let unloaded = !tab.loaded();
            let loading = tab.loading()
                || tab
                    .id()
                    .is_some_and(|view| self.features.traversals.contains_key(&view));
            let width = if vertical {
                ui.available_width().max(130.0)
            } else {
                195.0
            };
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(width, 36.0), egui::Sense::click_and_drag());
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::Role::Tab, ui.is_enabled(), selected, &title)
            });
            if response.has_focus() {
                ui.painter().rect_stroke(
                    rect,
                    18,
                    Stroke::new(2.0, colors.accent),
                    egui::StrokeKind::Inside,
                );
            }
            if selected || response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    18,
                    if selected { colors.inset } else { colors.hover },
                );
            }
            let label = format!("{}{}", if loading { "… " } else { "" }, title);
            if let Some(texture) = self.favicons.get(&self.session().tabs()[index].url()) {
                let icon_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 19.0, rect.center().y),
                    egui::vec2(16.0, 16.0),
                );
                ui.painter().image(
                    texture.id(),
                    icon_rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    if unloaded {
                        egui::Color32::WHITE.gamma_multiply(0.55)
                    } else {
                        egui::Color32::WHITE
                    },
                );
            }
            if !self
                .favicons
                .contains_key(&self.session().tabs()[index].url())
            {
                paint_icon(
                    ui.painter(),
                    egui::pos2(rect.left() + 19.0, rect.center().y),
                    "▤",
                    colors.muted,
                );
            }
            let title_rect = egui::Rect::from_min_max(
                rect.min + egui::vec2(34.0, 0.0),
                rect.max - egui::vec2(36.0, 0.0),
            );
            let mut job = egui::text::LayoutJob::simple_singleline(
                label,
                egui::FontId::proportional(14.0),
                if selected {
                    colors.text
                } else if unloaded {
                    colors.muted.gamma_multiply(0.7)
                } else {
                    colors.muted
                },
            );
            job.wrap.max_width = title_rect.width();
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            ui.painter()
                .with_clip_rect(title_rect.intersect(ui.clip_rect()))
                .galley(
                    egui::pos2(
                        title_rect.left(),
                        title_rect.center().y - galley.size().y / 2.0,
                    ),
                    galley,
                    colors.text,
                );
            let close_rect = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 18.0, rect.center().y),
                egui::vec2(26.0, 26.0),
            );
            let close = ui.interact(
                close_rect,
                ui.scope_id().with(("tab-close", id)),
                egui::Sense::click(),
            );
            close.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::Role::Button,
                    ui.is_enabled(),
                    format!("Close tab: {title}"),
                )
            });
            if close.hovered() || close.has_focus() {
                ui.painter().rect_filled(close_rect, 7, colors.hover);
            }
            if selected
                || response.hovered()
                || response.has_focus()
                || close.hovered()
                || close.has_focus()
            {
                paint_icon(
                    ui.painter(),
                    close_rect.center(),
                    "×",
                    if close.hovered() {
                        colors.text
                    } else {
                        colors.muted
                    },
                );
            }
            let closing = close.on_hover_text("Close tab").clicked();
            let response = response.on_hover_text(format!(
                "{}\n{}\n{}Drag to reorder · Right-click for tab options",
                title,
                self.session().tabs()[index].url(),
                if unloaded {
                    "Unloaded to save memory; select to reload.\n"
                } else {
                    ""
                }
            ));
            if response.clicked() && !closing {
                action = Some(TabAction::Select(index));
            }
            if closing || response.clicked_by(egui::PointerButton::Middle) {
                action = Some(TabAction::Close(index));
            }
            if response.drag_started() {
                response.dnd_set_drag_payload(id);
            }
            response.context_menu(|ui| {
                if ui.button("Close tab").clicked() {
                    action = Some(TabAction::Close(index));
                    ui.close();
                }
                if ui
                    .add_enabled(!selected && !unloaded, egui::Button::new("Unload tab"))
                    .on_hover_text("Free this tab's memory. It reloads when you select it.")
                    .clicked()
                {
                    action = Some(TabAction::Discard(index));
                    ui.close();
                }
                if ui
                    .add_enabled(index > 0, egui::Button::new("Move earlier"))
                    .clicked()
                {
                    action = Some(TabAction::Move(index, index - 1));
                    ui.close();
                }
                if ui
                    .add_enabled(
                        index + 1 < self.session().tabs().len(),
                        egui::Button::new("Move later"),
                    )
                    .clicked()
                {
                    action = Some(TabAction::Move(index, index + 1));
                    ui.close();
                }
                if ui.button("Switch top / sidebar").clicked() {
                    action = Some(TabAction::Layout);
                    ui.close();
                }
            });
            self.chrome.tab_rects.push((id, rect));
            if let Some(payload) = response.dnd_release_payload::<u64>()
                && let Some(from) = self.session().tabs().iter().position(|t| t.key == *payload)
            {
                action = Some(TabAction::Move(from, index));
            }
            if response.contains_pointer()
                && egui::DragAndDrop::has_payload_of_type::<u64>(ui.ctx())
            {
                ui.painter().rect_stroke(
                    rect,
                    18,
                    Stroke::new(2.0, colors.accent),
                    egui::StrokeKind::Inside,
                );
            }
            if vertical {
                ui.add_space(4.0);
            }
        }
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(if vertical { ui.available_width() } else { 36.0 }, 36.0),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), "New tab")
        });
        if response.hovered() {
            ui.painter().rect_filled(rect, 18, colors.hover);
        }
        // Left-aligned like the tab titles above it in the sidebar.
        let (position, align, label) = if vertical {
            (
                rect.left_center() + egui::vec2(12.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "+   New tab",
            )
        } else {
            (rect.center(), egui::Align2::CENTER_CENTER, "+")
        };
        ui.painter().text(
            position,
            align,
            label,
            egui::FontId::proportional(14.0),
            colors.text,
        );
        if response.on_hover_text("New tab (Cmd/Ctrl+T)").clicked() {
            action = Some(TabAction::New);
        }
        if let Some(action) = action {
            match action {
                TabAction::Select(i) => self.select_tab(i),
                TabAction::Close(i) => self.close_tab(i),
                TabAction::Discard(i) => {
                    self.discard_tab(i);
                }
                TabAction::New => {
                    self.new_tab();
                }
                TabAction::Layout => self.toggle_layout(),
                TabAction::Move(from, to) => {
                    self.move_tab(from, to);
                }
            }
        }
    }

    fn collections(&mut self, ui: &mut Ui) {
        let colors = crate::theme::palette(ui.ctx());
        self.chrome.library_rect = egui::Rect::NOTHING;
        if !self.chrome.show_bookmarks && !self.chrome.show_history {
            return;
        }
        let mut open_url = None;
        let mut remove = None;
        let library_content = |ui: &mut egui::Ui| {
            let bookmarks = self.chrome.show_bookmarks;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.chrome.show_bookmarks, true, "Bookmarks");
                ui.selectable_value(&mut self.chrome.show_bookmarks, false, "History");
                if self.chrome.show_bookmarks != bookmarks {
                    self.chrome.show_history = !self.chrome.show_bookmarks;
                    self.chrome.library_query.clear();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon(ui, "×", "Close library").clicked() {
                        self.chrome.show_bookmarks = false;
                        self.chrome.show_history = false;
                    }
                });
            });
            ui.add_space(6.0);
            ui.add_sized(
                [ui.available_width(), 32.0],
                egui::TextEdit::singleline(&mut self.chrome.library_query)
                    .hint_text("Search title or address"),
            );
            ui.add_space(6.0);
            let entry_count = if self.chrome.show_bookmarks {
                self.profile().bookmarks.len()
            } else {
                self.profile().history.len()
            };
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        entry_count,
                        if self.chrome.show_bookmarks {
                            "saved pages"
                        } else {
                            "recent pages"
                        }
                    ))
                    .size(11.0)
                    .color(colors.muted),
                );
                if !self.chrome.show_bookmarks {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                entry_count > 0,
                                egui::Button::new(
                                    RichText::new("Clear history…")
                                        .size(11.0)
                                        .color(colors.danger),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            self.chrome.confirm_clear_history = true;
                        }
                    });
                }
            });
            ui.add_space(4.0);
            let query = self.chrome.library_query.trim().to_lowercase();
            let entries = if self.chrome.show_bookmarks {
                &self.profile().bookmarks
            } else {
                &self.profile().history
            };
            let mut indices: Vec<_> = (0..entries.len()).collect();
            if !self.chrome.show_bookmarks {
                indices.sort_by_key(|i| std::cmp::Reverse(entries[*i].visited_at));
            }
            let mut visible = 0;
            let mut group = String::new();
            let now = chrono::Local::now();
            egui::ScrollArea::vertical()
                .id_salt("library_entries")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for index in indices {
                        let entry = &entries[index];
                        if !query.is_empty()
                            && !entry.title.to_lowercase().contains(&query)
                            && !entry.url.to_lowercase().contains(&query)
                        {
                            continue;
                        }
                        visible += 1;
                        let (date, time) = if self.chrome.show_bookmarks {
                            (String::new(), String::new())
                        } else {
                            profile::history_date(entry.visited_at, now)
                        };
                        if !self.chrome.show_bookmarks && group != date {
                            group = date;
                            ui.add_space(6.0);
                            ui.label(RichText::new(&group).size(11.0).color(colors.muted));
                        }
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 58.0),
                            egui::Sense::click(),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::Button,
                                ui.is_enabled(),
                                format!("{} {}", entry.title, entry.url),
                            )
                        });
                        if response.has_focus() {
                            ui.painter().rect_stroke(
                                rect,
                                8,
                                Stroke::new(2.0, colors.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        let remove_id = ui.scope_id().with(("remove-bookmark", index));
                        let hovered = ui.rect_contains_pointer(rect)
                            || response.has_focus()
                            || ui.memory(|m| m.has_focus(remove_id));
                        ui.painter().rect_filled(
                            rect,
                            8,
                            if hovered { colors.hover } else { colors.chrome },
                        );
                        if let Some(texture) = self.favicons.get(&entry.url) {
                            let icon_rect = egui::Rect::from_center_size(
                                egui::pos2(rect.left() + 18.0, rect.top() + 18.0),
                                egui::vec2(16.0, 16.0),
                            );
                            ui.painter().image(
                                texture.id(),
                                icon_rect,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                                egui::Color32::WHITE,
                            );
                        }
                        if !self.favicons.contains_key(&entry.url) {
                            paint_icon(
                                ui.painter(),
                                egui::pos2(rect.left() + 18.0, rect.top() + 18.0),
                                "▤",
                                colors.muted,
                            );
                        }
                        let text_rect = egui::Rect::from_min_max(
                            rect.min + egui::vec2(34.0, 8.0),
                            rect.max
                                - egui::vec2(
                                    if self.chrome.show_bookmarks {
                                        38.0
                                    } else {
                                        10.0
                                    },
                                    6.0,
                                ),
                        );
                        let painter = ui.painter().with_clip_rect(text_rect);
                        let title = if entry.title.is_empty() {
                            &entry.url
                        } else {
                            &entry.title
                        };
                        let title_job = egui::text::LayoutJob::simple_singleline(
                            title.clone(),
                            egui::FontId::proportional(13.0),
                            colors.text,
                        );
                        painter.galley(
                            text_rect.min,
                            ui.fonts_mut(|f| f.layout_job(title_job)),
                            colors.text,
                        );
                        let subtitle = if time.is_empty() {
                            entry.url.clone()
                        } else {
                            format!("{time} · {}", entry.url)
                        };
                        painter.text(
                            text_rect.min + egui::vec2(0.0, 23.0),
                            egui::Align2::LEFT_TOP,
                            subtitle,
                            egui::FontId::proportional(11.0),
                            colors.muted,
                        );
                        let mut removed = false;
                        if self.chrome.show_bookmarks {
                            response.context_menu(|ui| {
                                if ui.button("Remove bookmark").clicked() {
                                    remove = Some(index);
                                    removed = true;
                                    ui.close();
                                }
                            });
                        }
                        if self.chrome.show_bookmarks && hovered {
                            let close_rect = egui::Rect::from_center_size(
                                egui::pos2(rect.right() - 19.0, rect.center().y),
                                egui::vec2(28.0, 28.0),
                            );
                            let close = ui.interact(
                                close_rect,
                                ui.scope_id().with(("remove-bookmark", index)),
                                egui::Sense::click(),
                            );
                            close.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::Role::Button,
                                    ui.is_enabled(),
                                    format!("Remove bookmark: {title}"),
                                )
                            });
                            if close.hovered() || close.has_focus() {
                                ui.painter().rect_filled(close_rect, 6, colors.selected);
                            }
                            paint_icon(ui.painter(), close_rect.center(), "×", colors.text);
                            if close.on_hover_text("Remove bookmark").clicked() {
                                remove = Some(index);
                                removed = true;
                            }
                        }
                        if !removed
                            && response
                                .on_hover_text(format!("{title}\n{}", entry.url))
                                .clicked()
                        {
                            open_url = Some(entry.url.clone());
                        }
                        ui.add_space(5.0);
                    }
                    if visible == 0 {
                        ui.add_space(20.0);
                        ui.label(
                            RichText::new(if query.is_empty() {
                                "No pages here yet."
                            } else {
                                "No matching pages."
                            })
                            .color(colors.muted),
                        );
                    }
                });
        };
        let compact = crate::ux::compact(ui.ctx().content_rect().width());
        self.chrome.library_rect = if compact {
            egui::Window::new("Library")
                .id(egui::Id::unique("compact-library"))
                .default_width(320.0)
                .max_width((ui.ctx().content_rect().width() - 32.0).max(260.0))
                .max_height((ui.ctx().content_rect().height() - 130.0).max(200.0))
                .anchor(egui::Align2::RIGHT_TOP, [-16.0, 96.0])
                .show(ui.ctx(), library_content)
                .map(|s| s.response.rect)
                .unwrap_or(egui::Rect::NOTHING)
        } else {
            egui::Panel::right("library")
                .resizable(true)
                .default_size(320.0)
                .min_size(260.0)
                .max_size(440.0)
                .frame(
                    Frame::new()
                        .fill(colors.inset)
                        .corner_radius(12)
                        .outer_margin(8)
                        .inner_margin(12),
                )
                .show(ui, library_content)
                .response
                .rect
                .shrink(8.0)
        };
        if let Some(index) = remove {
            self.apply(wega::application::Command::RemoveBookmark(index));
            self.changed();
        }
        if let Some(url) = open_url {
            self.navigate(&url);
        }
        if self.chrome.confirm_clear_history {
            let fresh = self.prepare_dialog(ui.ctx(), "clear-history");
            if crate::ux::escape(ui.ctx()) {
                self.chrome.confirm_clear_history = false;
                self.finish_dialog(ui.ctx());
                return;
            }
            egui::Modal::new(Id::unique("confirm-clear-history")).show(ui.ctx(), |ui| {
                ui.heading("Clear browsing history?");
                ui.label("Remove the saved list of visited pages from this profile?");
                ui.horizontal(|ui| {
                    let cancel = ui.button("Cancel");
                    if fresh {
                        cancel.request_focus();
                    }
                    if cancel.clicked() {
                        self.chrome.confirm_clear_history = false;
                    }
                    if ui
                        .button(RichText::new("Clear history").color(colors.danger))
                        .clicked()
                    {
                        self.apply(wega::application::Command::ClearHistory);
                        self.chrome.confirm_clear_history = false;
                        self.changed();
                    }
                });
            });
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.destructive_action.is_some() {
            self.page_safety_ui(ctx);
            return;
        }
        self.feature_ui(ctx);
        if !self.dialogs.prompts.is_empty() || self.features.clear_data {
            return;
        }
        // Requests are answered once by consuming the corresponding Servo responder.
        if let Some((request_id, origin, permission)) = self
            .pending_permission()
            .map(|(id, origin, permission)| (id, origin.to_owned(), permission.to_owned()))
        {
            let fresh = self.prepare_dialog(ctx, format!("permission-{request_id:?}"));
            let mut answer = if crate::ux::escape(ctx) {
                Some(false)
            } else {
                None
            };
            egui::Modal::new(Id::unique("site_permission")).show(ctx, |ui| {
                ui.heading("Site permission");
                ui.label(origin);
                ui.label(format!("Allow access to {permission}?"));
                ui.horizontal(|ui| {
                    let deny = ui.button("Deny");
                    if fresh {
                        deny.request_focus();
                    }
                    if deny.clicked() {
                        answer = Some(false);
                    }
                    if ui.button("Allow for this page").clicked() {
                        answer = Some(true);
                    }
                });
            });
            if let Some(allow) = answer {
                self.answer_permission(request_id, allow);
                self.finish_dialog(ctx);
            }
            return;
        }
        if !self.dialogs.authentications.is_empty() {
            let fresh = self.prepare_dialog(ctx, "authentication");
            let submit = crate::ux::enter(ctx);
            let mut answer = if crate::ux::escape(ctx) {
                Some(false)
            } else {
                None
            };
            let request = self.dialogs.authentications.front().unwrap();
            egui::Modal::new(Id::unique("authentication")).show(ctx, |ui| {
                ui.heading("Website authentication");
                ui.label(request.url().origin().ascii_serialization());
                let username = ui.add(
                    egui::TextEdit::singleline(&mut self.chrome.username).hint_text("Username"),
                );
                if fresh {
                    username.request_focus();
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.chrome.password)
                        .password(true)
                        .hint_text("Password"),
                );
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                    if ui.button("Sign in").clicked() || submit {
                        answer = Some(true);
                    }
                });
            });
            if let Some(allow) = answer {
                let request = self.dialogs.authentications.pop_front().unwrap();
                if allow {
                    request.authenticate(
                        std::mem::take(&mut self.chrome.username),
                        std::mem::take(&mut self.chrome.password),
                    );
                }
                self.chrome.password.clear();
                self.chrome.username.clear();
                self.finish_dialog(ctx);
            }
            return;
        }
        let Some((owner, mut control)) = self.dialogs.controls.pop_front() else {
            return;
        };
        let fresh = self.prepare_dialog(ctx, format!("control-{:?}", control.id()));
        let cancel = crate::ux::escape(ctx);
        let submit = crate::ux::enter(ctx);
        let mut finished = false;
        let title = self
            .session()
            .tabs()
            .iter()
            .find(|t| t.id() == Some(owner))
            .map(|t| t.url())
            .unwrap_or_default();
        match &mut control {
            servo::EmbedderControl::SimpleDialog(dialog) => {
                if fresh && let servo::SimpleDialog::Prompt(prompt) = dialog {
                    self.chrome.prompt_text = prompt.current_value().to_owned();
                }
                let mut answer = if cancel {
                    Some(false)
                } else if submit {
                    Some(true)
                } else {
                    None
                };
                egui::Modal::new(Id::unique("site_dialog")).show(ctx, |ui| {
                    ui.heading("Message from website");
                    ui.label(shorten(&title, 80));
                    ui.separator();
                    ui.label(dialog.message());
                    if let servo::SimpleDialog::Prompt(_) = dialog {
                        let field =
                            ui.add(egui::TextEdit::singleline(&mut self.chrome.prompt_text));
                        if fresh {
                            field.request_focus();
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            answer = Some(false);
                        }
                        if ui.button("OK").clicked() {
                            answer = Some(true);
                        }
                    });
                });
                if let Some(allow) = answer {
                    if let servo::EmbedderControl::SimpleDialog(mut dialog) = control {
                        if let servo::SimpleDialog::Prompt(prompt) = &mut dialog {
                            prompt.set_current_value(&std::mem::take(&mut self.chrome.prompt_text));
                        }
                        if allow {
                            dialog.confirm();
                        } else {
                            dialog.dismiss();
                        }
                    }
                    return;
                }
            }
            servo::EmbedderControl::SelectElement(select) => {
                if fresh {
                    self.chrome.original_selection = select.selected_options();
                }
                if cancel {
                    select.select(self.chrome.original_selection.clone());
                    if let servo::EmbedderControl::SelectElement(select) = control {
                        drop(select);
                    }
                    self.finish_dialog(ctx);
                    return;
                }
                let mut selected = select.selected_options();
                egui::Modal::new(Id::unique("select_element")).show(ctx, |ui| {
                    ui.label("Choose an option");
                    egui::ScrollArea::vertical()
                        .max_height(350.0)
                        .show(ui, |ui| {
                            let mut option = |ui: &mut Ui, item: &servo::SelectElementOption| {
                                let checked = selected.contains(&item.id);
                                if ui
                                    .add_enabled(
                                        !item.is_disabled,
                                        egui::Button::new(&item.label).selected(checked),
                                    )
                                    .clicked()
                                {
                                    if select.allow_select_multiple() {
                                        if checked {
                                            selected.retain(|id| *id != item.id);
                                        } else {
                                            selected.push(item.id);
                                        }
                                    } else {
                                        selected = vec![item.id];
                                        finished = true;
                                    }
                                }
                            };
                            for item in select.options() {
                                match item {
                                    servo::SelectElementOptionOrOptgroup::Option(item) => {
                                        option(ui, item)
                                    }
                                    servo::SelectElementOptionOrOptgroup::Optgroup {
                                        label,
                                        options,
                                    } => {
                                        ui.strong(label);
                                        for item in options {
                                            option(ui, item);
                                        }
                                    }
                                }
                            }
                        });
                    if ui.button("Done").clicked() || submit {
                        finished = true;
                    }
                });
                select.select(selected);
                if finished {
                    if let servo::EmbedderControl::SelectElement(select) = control {
                        select.submit();
                    }
                    return;
                }
            }
            servo::EmbedderControl::FilePicker(picker) => {
                if fresh {
                    self.chrome.file_handoff = None;
                    if let Some(directory) = std::env::var_os("WEGA_FILE_HANDOFF_DIR") {
                        let uploads = std::env::var_os("BROWSER_UPLOAD_DIR")
                            .map(std::path::PathBuf::from)
                            .unwrap_or_else(|| "/config/uploads".into());
                        match crate::file_handoff::Request::new(
                            std::path::Path::new(&directory),
                            &uploads,
                            picker.allow_select_multiple(),
                            url::Url::parse(&title)
                                .map(|url| url.origin().ascii_serialization())
                                .unwrap_or_else(|_| "Page".into()),
                            picker
                                .filter_patterns()
                                .iter()
                                .filter(|pattern| pattern.0.len() <= 256)
                                .take(64)
                                .map(|pattern| pattern.0.clone())
                                .collect(),
                        ) {
                            Ok(request) => self.chrome.file_handoff = Some(request),
                            Err(error) => {
                                self.chrome.file_error = Some(format!(
                                    "Viewer handoff unavailable: {error}. Use path entry."
                                ))
                            }
                        }
                    }
                }
                let remote = self
                    .chrome
                    .file_handoff
                    .as_ref()
                    .map(|request| request.poll());
                if self.chrome.file_handoff.is_some() {
                    ctx.request_repaint_after(std::time::Duration::from_millis(250));
                }
                let mut remote_answer = None;
                match remote {
                    Some(Ok(Some(paths))) => {
                        self.chrome.file_paths = paths
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                            .join("\n");
                        remote_answer = Some(!paths.is_empty());
                    }
                    Some(Err(error)) => {
                        self.chrome.file_error = Some(error);
                        self.chrome.file_handoff = None;
                    }
                    _ => {}
                }
                let mut answer = if cancel {
                    Some(false)
                } else if submit {
                    Some(true)
                } else {
                    None
                };
                if remote_answer.is_some() {
                    answer = remote_answer;
                }
                egui::Modal::new(Id::unique("file_picker")).show(ctx, |ui| {
                    ui.set_max_width((ctx.content_rect().width() - 48.0).clamp(250.0, 520.0));
                    ui.heading("Choose file for website");
                    ui.label(shorten(&title, 80));
                    if std::env::var_os("BROWSER_DOWNLOAD_DIR").is_some() {
                        ui.label("Choose from this computer in the web viewer to upload directly. Upload and path entry remain available as a fallback.");
                    } else if ui.button("Browse files…").clicked() {
                        let mut dialog = rfd::FileDialog::new().set_parent(self.window.as_ref()).set_title("Choose file for website");
                        let extensions: Vec<_> = picker.filter_patterns().iter().map(|p| p.0.as_str()).collect();
                        if !extensions.is_empty() { dialog = dialog.add_filter("Accepted files", &extensions); }
                        let selected = if picker.allow_select_multiple() { dialog.pick_files() } else { dialog.pick_file().map(|p| vec![p]) };
                        if let Some(paths) = selected {
                            self.chrome.file_paths = paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n");
                            self.chrome.file_error = None;
                        }
                    }
                    ui.label(if picker.allow_select_multiple() { "Selected paths (one per line)" } else { "Selected file path" });
                    let field = ui.add(egui::TextEdit::multiline(&mut self.chrome.file_paths).desired_width(f32::INFINITY).desired_rows(3));
                    if fresh { field.request_focus(); }
                    if let Some(error) = &self.chrome.file_error { ui.colored_label(crate::theme::palette(ctx).danger, error); }
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() { answer = Some(false); }
                        if ui.button("Choose").clicked() { answer = Some(true); }
                    });
                });
                if answer == Some(true) {
                    match crate::ux::validate_files(
                        &self.chrome.file_paths,
                        picker.allow_select_multiple(),
                    ) {
                        Ok(paths) => {
                            if let servo::EmbedderControl::FilePicker(mut picker) = control {
                                picker.select(&paths);
                                picker.submit();
                            }
                            self.chrome.file_paths.clear();
                            self.finish_dialog(ctx);
                            return;
                        }
                        Err(error) => self.chrome.file_error = Some(error),
                    }
                } else if answer == Some(false) {
                    if let servo::EmbedderControl::FilePicker(picker) = control {
                        picker.dismiss();
                    }
                    self.chrome.file_paths.clear();
                    self.finish_dialog(ctx);
                    return;
                }
            }
            servo::EmbedderControl::ColorPicker(picker) => {
                if fresh {
                    self.chrome.original_color =
                        picker.current_color().map(|c| [c.red, c.green, c.blue]);
                }
                if cancel {
                    picker.select(self.chrome.original_color.map(|c| servo::RgbColor {
                        red: c[0],
                        green: c[1],
                        blue: c[2],
                    }));
                    if let servo::EmbedderControl::ColorPicker(picker) = control {
                        drop(picker);
                    }
                    self.finish_dialog(ctx);
                    return;
                }
                let color = picker.current_color();
                let mut rgb = color.map(|c| [c.red, c.green, c.blue]).unwrap_or([0, 0, 0]);
                egui::Modal::new(Id::unique("color_picker")).show(ctx, |ui| {
                    ui.heading("Choose a color");
                    ui.color_edit_button_srgb(&mut rgb);
                    if ui.button("Done").clicked() || submit {
                        finished = true;
                    }
                });
                picker.select(Some(servo::RgbColor {
                    red: rgb[0],
                    green: rgb[1],
                    blue: rgb[2],
                }));
                if finished {
                    if let servo::EmbedderControl::ColorPicker(picker) = control {
                        picker.submit();
                    }
                    return;
                }
            }
            servo::EmbedderControl::ContextMenu(menu) => {
                let mut action = None;
                let mut download = None;
                finished = cancel;
                egui::Modal::new(Id::unique("page_context")).show(ctx, |ui| {
                    if let Some(url) = &menu.element_info().link_url
                        && ui.button("Download link…").clicked()
                    {
                        download = Some(url.to_string());
                        finished = true;
                    }
                    if let Some(url) = &menu.element_info().image_url
                        && ui.button("Download image…").clicked()
                    {
                        download = Some(url.to_string());
                        finished = true;
                    }
                    for item in menu.items() {
                        match item {
                            servo::ContextMenuItem::Item {
                                label,
                                action: item_action,
                                enabled,
                            } => {
                                if ui.add_enabled(*enabled, egui::Button::new(label)).clicked() {
                                    action = Some(*item_action);
                                }
                            }
                            servo::ContextMenuItem::Separator => {
                                ui.separator();
                            }
                        }
                    }
                    if ui.button("Dismiss").clicked() {
                        finished = true;
                    }
                });
                if let Some(url) = download {
                    self.offer_download(&url, "");
                }
                if action.is_some() || finished {
                    if let servo::EmbedderControl::ContextMenu(menu) = control {
                        if let Some(action) = action {
                            menu.select(action);
                        } else {
                            menu.dismiss();
                        }
                    }
                    return;
                }
            }
            servo::EmbedderControl::InputMethod(_) => return,
        }
        self.dialogs.controls.push_front((owner, control));
    }
}
