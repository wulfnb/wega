//! Native read-only Elements panel. Renders escaped text from bounded protocol
//! values; never injects scripts or asks for layout, styles, edits or overlays.
use crate::{browser::Browser, devtools_elements::ElementsView};
use egui::{RichText, Ui};
use wega::engine_protocol::inspector::dom::MAX_DOM_DEPTH;

pub(crate) fn draw(ui: &mut Ui, model: &mut ElementsView) {
    ui.horizontal(|ui| {
        ui.label("Read-only DOM");
        if ui.button("Reset tree").clicked() {
            model.clear_cache();
        }
        if model.pending.is_some() {
            ui.spinner();
        }
    });
    if !model.notice.is_empty() {
        ui.label(&model.notice);
    }
    if let Some(epoch) = model.epoch {
        ui.weak(format!("Mutation epoch {epoch}"));
    }
    let visible = model.visible();
    egui::ScrollArea::both()
        .id_salt("elements-tree")
        .max_height((ui.available_height() * 0.6).max(60.0))
        .show_rows(ui, 22.0, visible.len(), |ui, range| {
            for index in range {
                let (key, depth) = visible[index];
                let Some(row) = model.rows.get(&key) else {
                    continue;
                };
                let id = row.node.id;
                let selected = model.selected == Some(id);
                let mut expand = row.expanded;
                let has_children = row.node.child_count > 0;
                let mut label = if row.node.node_type == 1 {
                    let attrs = row
                        .node
                        .attributes
                        .iter()
                        .filter(|a| a.name == "id" || a.name == "class")
                        .map(|a| format!(" {}={:?}", a.name, a.value))
                        .collect::<String>();
                    format!(
                        "<{}{attrs}>{}",
                        row.node.name,
                        if row.node.truncated { " …" } else { "" }
                    )
                } else {
                    format!(
                        "{} {:?}{}",
                        row.node.name,
                        row.node.text,
                        if row.node.truncated { " …" } else { "" }
                    )
                };
                if Some(row.observed_epoch) != model.epoch {
                    label.push_str(" · cached");
                }
                if has_children && depth >= MAX_DOM_DEPTH {
                    label.push_str(" · depth limit");
                }
                let next = row.next;
                let has_previous = row.offset > 0;
                let mut select = false;
                let mut more = false;
                let mut first = false;
                ui.push_id(key, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space((depth as f32 * 12.0).min(240.0));
                        if has_children && depth < MAX_DOM_DEPTH {
                            if ui.small_button(if expand { "▾" } else { "▸" }).clicked() {
                                expand = !expand;
                            }
                        } else {
                            ui.label("·");
                        }
                        select = ui
                            .selectable_label(selected, RichText::new(label).monospace())
                            .clicked();
                        if expand && next.is_some() {
                            more = ui.small_button("Next children").clicked();
                        }
                        if expand && has_previous {
                            first = ui.small_button("First children").clicked();
                        }
                    });
                });
                if let Some(row) = model.rows.get_mut(&key) {
                    row.expanded = expand;
                }
                if select {
                    model.select(id);
                }
                if more {
                    model.more(key);
                }
                if first {
                    model.first(key);
                }
            }
        });
    ui.separator();
    if let Some(id) = model.selected
        && let Some(row) = model.rows.get(&id.key)
    {
        if Some(row.observed_epoch) != model.epoch {
            ui.weak("Cached selection; refreshing…");
        }
        ui.monospace(format!("Selected: {} · node {}", row.node.name, id.key));
        ui.weak(format!(
            "Type {} · namespace {} · prefix {} · {} children",
            row.node.node_type, row.node.namespace, row.node.prefix, row.node.child_count
        ));
        egui::ScrollArea::vertical()
            .id_salt("element-details")
            .show(ui, |ui| {
                if !row.node.text.is_empty() {
                    ui.monospace(&row.node.text);
                }
                for attr in &row.node.attributes {
                    ui.monospace(format!("{} = {:?}", attr.name, attr.value))
                        .on_hover_text(format!("Namespace: {}", attr.namespace));
                }
                if row.node.truncated {
                    ui.weak("Preview truncated by inspection limits");
                }
            });
    } else {
        ui.weak("Select a node to inspect attributes and text.");
    }
}
impl Browser {
    pub(crate) fn elements_tab(&mut self, ui: &mut Ui) {
        // Includes paint-time process/document validation, shared by the detached window.
        let valid = self.devtools_snapshot().is_some()
            && self.devtools_document() == self.devtools.elements.document;
        if valid {
            let valid_style = self.devtools.styles.page.as_ref().is_none_or(|p| {
                self.devtools_snapshot().is_some_and(|s| {
                    p.scope.target == s.target
                        && p.scope.document == s.document
                        && Some(p.scope.session) == self.devtools.session_id()
                })
            });
            if !valid_style {
                self.devtools.styles.reset();
            }
            ui.columns(2, |columns| {
                draw(&mut columns[0], &mut self.devtools.elements);
                self.devtools.styles.select(self.devtools.elements.selected);
                crate::devtools_styles_ui::draw(&mut columns[1], &mut self.devtools.styles);
            });
        } else {
            ui.label("Waiting for the current document…");
        }
    }
}
