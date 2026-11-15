//! The Console tab of the native developer tools. It renders `ConsoleView` (already validated,
//! bounded engine data) and forwards the user's actions; it holds no engine handle and has no
//! authority of its own: evaluation still needs a live grant, checked again by the page and by
//! the content process.
use crate::{
    browser::Browser,
    devtools_console::{ConsoleLine, ConsoleView},
    devtools_session::ViewState,
};
use egui::{Color32, RichText, Ui};
use wega::engine_protocol::{
    DocumentContext,
    inspector::{ConsoleLevel, ConsoleMessage, ExceptionDetails, Preview, PreviewKind},
};

fn level_style(level: ConsoleLevel) -> (&'static str, Color32) {
    match level {
        ConsoleLevel::Debug => ("·", Color32::from_gray(150)),
        ConsoleLevel::Log => ("›", Color32::from_gray(210)),
        ConsoleLevel::Info => ("i", Color32::from_rgb(110, 170, 240)),
        ConsoleLevel::Warn => ("⚠", Color32::from_rgb(235, 190, 70)),
        ConsoleLevel::Error => ("✖", Color32::from_rgb(235, 100, 100)),
    }
}

/// One-line rendering of a bounded preview. Top-level strings are shown raw, nested ones quoted.
pub(crate) fn preview_line(preview: &Preview, nested: bool) -> String {
    match preview.kind {
        PreviewKind::String if nested => format!("{:?}", preview.text),
        PreviewKind::Array | PreviewKind::Object if !preview.props.is_empty() => {
            let items: Vec<String> = preview
                .props
                .iter()
                .map(|prop| {
                    if preview.kind == PreviewKind::Array {
                        preview_line(&prop.value, true)
                    } else {
                        format!("{}: {}", prop.key, preview_line(&prop.value, true))
                    }
                })
                .collect();
            let more = if preview.more > 0 {
                format!(", … {} more", preview.more)
            } else {
                String::new()
            };
            if preview.kind == PreviewKind::Array {
                // "Array(3)" is shown as "(3) [a, b, c]".
                let count = preview.text.strip_prefix("Array").unwrap_or(&preview.text);
                format!("{count} [{}{more}]", items.join(", "))
            } else {
                format!("{} {{{}{more}}}", preview.text, items.join(", "))
            }
        }
        _ if preview.truncated => format!("{}…", preview.text),
        _ => preview.text.clone(),
    }
}

fn clock(timestamp_ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms as i64)
        .map(|utc| {
            utc.with_timezone(&chrono::Local)
                .format("%H:%M:%S%.3f")
                .to_string()
        })
        .unwrap_or_default()
}

fn source_label(message: &ConsoleMessage) -> Option<String> {
    message.source.as_ref().map(|source| {
        let file = source.url.rsplit('/').next().unwrap_or(&source.url);
        format!("{file}:{}:{}", source.line, source.column)
    })
}

fn exception_block(ui: &mut Ui, details: &ExceptionDetails) {
    if let Some(stack) = &details.stack {
        egui::CollapsingHeader::new("stack")
            .id_salt(stack.as_ptr() as usize)
            .show(ui, |ui| {
                ui.label(RichText::new(stack).monospace().weak());
            });
    }
}

fn draw_line(ui: &mut Ui, line: &ConsoleLine) {
    match line {
        ConsoleLine::Message(message) => {
            let (glyph, color) = level_style(message.level);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(glyph).color(color).monospace());
                ui.label(
                    RichText::new(clock(message.timestamp_ms))
                        .monospace()
                        .weak(),
                );
                let text = if message.args.is_empty() {
                    message.text.clone()
                } else {
                    message
                        .args
                        .iter()
                        .map(|arg| preview_line(arg, false))
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                ui.label(RichText::new(text).monospace().color(color));
                if let Some(source) = source_label(message) {
                    ui.label(RichText::new(source).monospace().weak());
                }
            });
            if let Some(details) = &message.exception {
                exception_block(ui, details);
            }
        }
        ConsoleLine::Input(text) => {
            ui.label(
                RichText::new(format!("» {text}"))
                    .monospace()
                    .color(Color32::from_rgb(150, 190, 255)),
            );
        }
        ConsoleLine::Result(preview) => {
            ui.label(RichText::new(format!("← {}", preview_line(preview, false))).monospace());
        }
        ConsoleLine::Failure(details) => {
            ui.label(
                RichText::new(format!("✖ {}: {}", details.name, details.message))
                    .monospace()
                    .color(Color32::from_rgb(235, 100, 100)),
            );
            if let Some(source) = &details.source {
                ui.label(
                    RichText::new(format!(
                        "   at {}:{}:{}",
                        source.url, source.line, source.column
                    ))
                    .monospace()
                    .weak(),
                );
            }
            exception_block(ui, details);
        }
        ConsoleLine::Notice(text) => {
            ui.label(RichText::new(text).italics().weak());
        }
    }
}

fn level_toggles(ui: &mut Ui, console: &mut ConsoleView) {
    for (index, (label, level)) in [
        ("Debug", ConsoleLevel::Debug),
        ("Log", ConsoleLevel::Log),
        ("Info", ConsoleLevel::Info),
        ("Warn", ConsoleLevel::Warn),
        ("Error", ConsoleLevel::Error),
    ]
    .into_iter()
    .enumerate()
    {
        let (_, color) = level_style(level);
        ui.toggle_value(
            &mut console.levels[index],
            RichText::new(label).color(color),
        );
    }
}

impl Browser {
    pub(crate) fn console_tab(&mut self, ui: &mut Ui) {
        let connected = matches!(
            self.devtools.state,
            ViewState::Attached | ViewState::Updating | ViewState::NotResponding
        );
        let document: Option<DocumentContext> = self.devtools_document();
        // The decision comes from the committed principal, not from the tab's URL.
        let principal = self.devtools_principal();
        let established = principal.is_some();
        let opaque = principal
            .as_ref()
            .is_none_or(|p| matches!(p.origin, wega::engine_protocol::authority::Origin::Opaque));
        let enabled =
            document.and_then(|document| self.devtools.console.evaluation_enabled(document));
        let mut enable = None;
        let mut disable = false;
        let mut run = None;

        // Evaluation authority: always visible, never implicit.
        ui.horizontal_wrapped(|ui| match enabled {
            Some(overrode) => {
                ui.label(
                    RichText::new(if overrode {
                        "Evaluation enabled for this document (opaque origin allowed)"
                    } else {
                        "Evaluation enabled for this document"
                    })
                    .color(Color32::from_rgb(120, 200, 140)),
                );
                if ui
                    .button("Disable")
                    .on_hover_text(
                        "Revoke now. Also ends on navigation, closing the tools or switching tabs.",
                    )
                    .clicked()
                {
                    disable = true;
                }
            }
            None => {
                ui.weak("Commands run with full page authority.");
                let label = if !established {
                    "Evaluation unavailable"
                } else if opaque {
                    "Enable anyway (opaque origin)…"
                } else {
                    "Enable evaluation"
                };
                if ui
                    .add_enabled(
                        connected && document.is_some() && established,
                        egui::Button::new(label),
                    )
                    .on_hover_text(
                        "Allow JavaScript commands in this page's current document only.",
                    )
                    .clicked()
                {
                    enable = Some(opaque);
                }
            }
        });

        let console = &mut self.devtools.console;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut console.filter)
                    .hint_text("Filter")
                    .desired_width(160.0),
            );
            level_toggles(ui, console);
            if ui.button("Clear").clicked() {
                console.clear();
            }
        });
        if console.dropped > 0 {
            ui.label(
                RichText::new(format!(
                    "{} messages were dropped (more output than the tools keep).",
                    console.dropped
                ))
                .color(Color32::from_rgb(235, 190, 70)),
            );
        }
        let input_height = 30.0;
        egui::ScrollArea::vertical()
            .id_salt("console-lines")
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .max_height((ui.available_height() - input_height).max(60.0))
            .show(ui, |ui| {
                for line in console.lines().filter(|line| console.visible(line)) {
                    draw_line(ui, line);
                }
                if console.len() == 0 && connected {
                    ui.weak("No console output yet.");
                }
            });
        ui.horizontal(|ui| {
            let editable = enabled.is_some() && connected && console.pending.is_none();
            let response = ui.add_enabled(
                editable,
                egui::TextEdit::singleline(&mut console.input)
                    .hint_text(if enabled.is_some() {
                        "JavaScript — Enter to run"
                    } else {
                        "Enable evaluation to run JavaScript"
                    })
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
            if response.has_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    if let Some(text) = console.recall_older() {
                        console.input = text.to_string();
                    }
                } else if ui.input(|i| i.key_pressed(egui::Key::ArrowDown))
                    && let Some(text) = console.recall_newer()
                {
                    console.input = text.to_string();
                }
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let text = std::mem::take(&mut console.input);
                if !text.trim().is_empty() {
                    run = Some(text);
                }
                response.request_focus();
            }
            if console.pending.is_some() {
                ui.spinner();
            }
        });
        if let Some(opaque) = enable {
            self.devtools_enable_evaluation(opaque);
        }
        if disable {
            self.devtools_revoke_evaluation();
        }
        if let Some(text) = run {
            self.devtools_run_command(&text);
        }
    }
}

#[cfg(test)]
#[path = "devtools_console_ui_tests.rs"]
mod tests;
