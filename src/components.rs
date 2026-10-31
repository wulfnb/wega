//! Shared chrome components built on the theme palette.
//!
//! Every surface uses these instead of composing raw egui widgets, so tabs,
//! filters, actions, messages and empty states look and behave the same in
//! the browser chrome, dialogs and developer tools.
use crate::ux::Severity;
use egui::{Color32, CornerRadius, Response, RichText, Sense, Stroke, Ui};

const TAB_HEIGHT: f32 = 30.0;

/// Exclusive tabs with an underline on the selected one. Returns true when
/// the selection changed. Labels are the accessible names, so they stay
/// stable for assistive tech and the UI tests.
pub fn tabs<T: PartialEq + Copy>(ui: &mut Ui, current: &mut T, items: &[(T, &str)]) -> bool {
    let colors = crate::theme::palette(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (value, label) in items {
            let selected = *current == *value;
            let font = egui::TextStyle::Button.resolve(ui.style());
            let color = if selected {
                colors.accent
            } else {
                colors.muted
            };
            let galley = ui
                .painter()
                .layout_no_wrap((*label).to_owned(), font, color);
            let size = egui::vec2(galley.size().x + 20.0, TAB_HEIGHT);
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Tab, ui.is_enabled(), *label)
            });
            // egui reports `selected` as a toggle state; tabs use accesskit's own
            // selected state so screen readers announce the active tab.
            ui.ctx()
                .accesskit_node_builder(response.id, |node| node.set_selected(selected));
            if ui.is_rect_visible(rect) {
                let painter = ui.painter();
                if response.hovered() || response.has_focus() {
                    painter.rect_filled(rect, 6, colors.hover);
                }
                let text = rect.center() - galley.size() / 2.0;
                painter.galley(text, galley, color);
                if selected {
                    let line = egui::Rect::from_min_max(
                        egui::pos2(rect.left() + 6.0, rect.bottom() - 2.5),
                        egui::pos2(rect.right() - 6.0, rect.bottom()),
                    );
                    painter.rect_filled(line, 1, colors.accent);
                }
            }
            if response.clicked() && !selected {
                *current = *value;
                changed = true;
            }
        }
    });
    changed
}

/// A pill that toggles a filter on or off.
pub fn chip(ui: &mut Ui, selected: bool, label: &str) -> Response {
    let colors = crate::theme::palette(ui.ctx());
    let text =
        RichText::new(label)
            .size(12.5)
            .color(if selected { colors.text } else { colors.muted });
    ui.add(
        egui::Button::new(text)
            .selected(selected)
            .fill(if selected {
                colors.selected
            } else {
                Color32::TRANSPARENT
            })
            .stroke(Stroke::new(
                1.0,
                if selected {
                    colors.accent
                } else {
                    colors.border
                },
            ))
            .corner_radius(CornerRadius::same(12))
            .min_size(egui::vec2(0.0, 24.0)),
    )
}

/// The main action of a dialog or panel.
pub fn primary_button(ui: &Ui, label: &str) -> egui::Button<'static> {
    let colors = crate::theme::palette(ui.ctx());
    egui::Button::new(RichText::new(label).color(colors.on_accent).strong())
        .fill(colors.accent)
        .min_size(egui::vec2(0.0, 30.0))
}

/// An action that deletes or clears data.
pub fn destructive_button(ui: &Ui, label: &str) -> egui::Button<'static> {
    let colors = crate::theme::palette(ui.ctx());
    egui::Button::new(RichText::new(label).color(colors.danger).strong())
        .fill(colors.danger_bg)
        .min_size(egui::vec2(0.0, 30.0))
}

/// A message shown in place, tinted by severity. Errors stay until the user
/// acts; callers decide when to clear it.
pub fn inline_message(ui: &mut Ui, severity: Severity, message: &str) -> Response {
    let colors = crate::theme::palette(ui.ctx());
    let (fill, fg) = match severity {
        Severity::Error => (colors.danger_bg, colors.danger),
        Severity::Warning => (colors.warning_bg, colors.amber),
        Severity::Success => (colors.surface, colors.success),
        Severity::Info => (colors.surface, colors.text),
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if matches!(severity, Severity::Error | Severity::Warning) {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 18.0), Sense::hover());
                    crate::ui::paint_icon(ui.painter(), rect.center(), "warning", fg);
                }
                ui.add(
                    egui::Label::new(RichText::new(message).color(fg))
                        .wrap()
                        .selectable(true),
                );
            });
        })
        .response
}

/// Placeholder for an area with nothing to show yet. Returns true when the
/// optional action button was clicked.
pub fn empty_state(ui: &mut Ui, title: &str, hint: &str, action: Option<&str>) -> bool {
    let colors = crate::theme::palette(ui.ctx());
    let mut clicked = false;
    ui.vertical_centered(|ui| {
        ui.add_space(16.0);
        ui.label(RichText::new(title).size(14.0).color(colors.text));
        if !hint.is_empty() {
            ui.label(RichText::new(hint).size(12.0).color(colors.muted));
        }
        if let Some(action) = action {
            ui.add_space(4.0);
            clicked = ui.button(action).clicked();
        }
        ui.add_space(16.0);
    });
    clicked
}

/// Dialog action row: Cancel on the left of the primary action, both on the
/// right. Returns Some(true) for the primary action, Some(false) for cancel.
/// Enter and Escape are handled by the caller through `ux::enter`/`ux::escape`.
pub fn dialog_buttons(
    ui: &mut Ui,
    cancel: Option<&str>,
    primary: &str,
    destructive: bool,
) -> Option<bool> {
    let mut answer = None;
    ui.add_space(4.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let button = if destructive {
            destructive_button(ui, primary)
        } else {
            primary_button(ui, primary)
        };
        if ui.add(button).clicked() {
            answer = Some(true);
        }
        if let Some(cancel) = cancel
            && ui
                .add(egui::Button::new(cancel).min_size(egui::vec2(0.0, 30.0)))
                .clicked()
        {
            answer = Some(false);
        }
    });
    answer
}

#[cfg(test)]
#[path = "components_tests.rs"]
mod tests;
