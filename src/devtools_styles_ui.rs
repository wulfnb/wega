use crate::devtools_styles::{Mode, StyleView};
use egui::Ui;
use wega::engine_protocol::inspector::styles::*;
pub(crate) fn draw(ui: &mut Ui, view: &mut StyleView) {
    ui.separator();
    ui.horizontal(|ui| {
        ui.checkbox(&mut view.enabled, "Read-only Styles / Computed");
        if view.pending() {
            ui.spinner();
        }
    });
    if !view.enabled {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for (mode, label) in [
            (Mode::Inline, "Inline"),
            (Mode::Computed, "Computed"),
            (Mode::Sheets, "Stylesheets"),
            (Mode::Matched, "Matched (unsupported)"),
        ] {
            if ui.selectable_label(view.mode == mode, label).clicked() {
                view.change_mode(mode);
            }
        }
        if ui.button("Refresh cached data").clicked() {
            view.refresh();
        }
    });
    if !view.notice.is_empty() {
        ui.label(&view.notice);
    }
    let Some(page) = view.page.as_ref() else {
        ui.weak("Select an element or open Stylesheets. Waiting for bounded cached data…");
        return;
    };
    ui.weak(&page.note);
    match page.availability {
        StyleAvailability::Unsupported => {
            ui.label("Unsupported for this query/node; this is not an empty match result.");
        }
        StyleAvailability::NotComputed => {
            ui.label("Not yet computed. Inspection does not trigger style or layout work.");
        }
        StyleAvailability::Cached => {
            ui.label(if page.layout_pending {
                "Cached computed values · layout pending"
            } else {
                "Cached computed values"
            });
        }
        _ => {}
    }
    if page.truncated {
        ui.weak("Inspection limit reached; text or enumeration is truncated.");
    }
    if page.items.is_empty() && page.availability == StyleAvailability::Available {
        ui.weak("No entries in this supported page.");
    }
    let mut action = None;
    egui::ScrollArea::vertical()
        .id_salt("css-style-data")
        .max_height(220.)
        .show_rows(ui, 28., page.items.len(), |ui, range| {
            for index in range {
                ui.push_id(index, |ui| match &page.items[index] {
                    StyleItem::Sheet {
                        reference,
                        url,
                        disabled,
                        rules,
                        ..
                    } => {
                        if ui
                            .button(format!(
                                "Author sheet · {} rules{} · {}",
                                rules,
                                if *disabled { " · disabled" } else { "" },
                                url
                            ))
                            .clicked()
                        {
                            action = Some((reference.clone(), false));
                        }
                    }
                    StyleItem::Rule {
                        reference,
                        kind,
                        selector,
                        children,
                        declarations,
                        source,
                        ..
                    } => {
                        ui.horizontal_wrapped(|ui| {
                            ui.monospace(format!(
                                "{} {}",
                                kind,
                                selector
                                    .as_deref()
                                    .unwrap_or("(selector/condition unavailable)")
                            ));
                            if *children > 0
                                && ui.button(format!("{children} child rules")).clicked()
                            {
                                action = Some((reference.clone(), false));
                            }
                            if *declarations > 0
                                && ui.button(format!("{declarations} declarations")).clicked()
                            {
                                action = Some((reference.clone(), true));
                            }
                            if let Some(source) = source {
                                ui.weak(format!(
                                    "{}:{}:{}",
                                    source.url, source.line, source.column
                                ));
                            }
                        });
                    }
                    StyleItem::Declaration {
                        name,
                        value,
                        important,
                        truncated,
                    } => {
                        ui.monospace(format!(
                            "{name}: {value}{}{}",
                            if *important == Some(true) {
                                " !important"
                            } else {
                                ""
                            },
                            if *truncated { " …" } else { "" }
                        ));
                    }
                });
            }
        });
    let next = page.next;
    let offset = page.query.offset();
    ui.horizontal(|ui| {
        if offset > 0 && ui.button("First page").clicked() {
            view.first();
        }
        if next.is_some() && ui.button("Next page").clicked() {
            view.more();
        }
        if matches!(view.mode, Mode::Rules | Mode::Declarations)
            && ui.button("Parent rules").clicked()
        {
            if let Some(mut reference) = view.focus.clone() {
                if !reference.path.is_empty() {
                    reference.path.pop();
                    view.show_rules(reference, false);
                } else {
                    view.change_mode(Mode::Sheets);
                }
            }
        }
    });
    if let Some((reference, decls)) = action {
        view.show_rules(reference, decls);
    }
}
