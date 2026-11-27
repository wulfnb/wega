//! Elements editing controls: the picker button, the editing permission indicator and the edit
//! forms. Every action only *queues* a validated `MutationOp`; authority, ordering and the
//! epoch check live in the session and the engine. Nothing here is shown or enabled unless the
//! browser's own record says editing is on for the current document.
use crate::{browser::Browser, devtools_geometry::EditView};
use egui::{Color32, RichText, Ui};
use wega::engine_protocol::{
    DocumentContext,
    authority::Origin,
    inspector::{
        dom::{DomNode, InspectorNodeId},
        edit::{MutationOp, is_code_attribute},
    },
};

/// An element whose preview was truncated cannot be edited safely: writing the shortened value
/// back would destroy data.
fn editable(node: &DomNode) -> bool {
    !node.truncated
}
fn protected(node: &DomNode) -> bool {
    node.node_type == 9 || matches!(node.name.as_str(), "html" | "head" | "body")
}

impl Browser {
    /// Picker button and the editing permission indicator. Always visible, never implicit.
    pub(crate) fn elements_toolbar(&mut self, ui: &mut Ui) {
        let document: Option<DocumentContext> = self.devtools_document();
        let principal = self.devtools_principal();
        let established = principal.is_some();
        let opaque = principal
            .as_ref()
            .is_none_or(|p| matches!(p.origin, Origin::Opaque));
        let enabled = document.is_some_and(|d| self.devtools.edit.enabled == Some(d));
        let mut toggle_picker = false;
        let mut enable = None;
        let mut disable = false;
        ui.horizontal_wrapped(|ui| {
            let picking = self.devtools.geometry.picker;
            if ui
                .selectable_label(picking, "⌖ Inspect element")
                .on_hover_text(
                    "Pick an element on the page. Page input is paused while picking; Esc or this button cancels.",
                )
                .clicked()
            {
                toggle_picker = true;
            }
            if picking {
                ui.label(RichText::new("Picking… click an element, Esc to cancel").color(Color32::from_rgb(111, 168, 220)));
            }
            ui.separator();
            if enabled {
                ui.label(
                    RichText::new("Editing enabled for this document")
                        .color(Color32::from_rgb(235, 190, 70)),
                );
                if ui
                    .button("Disable editing")
                    .on_hover_text("Revoke now. Also ends on navigation, closing the tools or switching tabs.")
                    .clicked()
                {
                    disable = true;
                }
            } else {
                ui.weak("Read-only");
                let label = if !established {
                    "Editing unavailable"
                } else if opaque {
                    "Enable editing anyway (opaque origin)…"
                } else {
                    "Enable editing"
                };
                if ui
                    .add_enabled(
                        document.is_some() && established,
                        egui::Button::new(label),
                    )
                    .on_hover_text("Allow DOM and inline-style edits in this document only. This is separate from enabling evaluation.")
                    .clicked()
                {
                    enable = Some(opaque);
                }
            }
        });
        if !self.devtools.edit.notice.is_empty() {
            ui.label(&self.devtools.edit.notice);
        } else if let Some(last) = &self.devtools.edit.last {
            ui.weak(format!("Last edit: {last}"));
        }
        if !self.devtools.geometry.notice.is_empty() {
            ui.weak(&self.devtools.geometry.notice);
        }
        if toggle_picker {
            let on = !self.devtools.geometry.picker;
            self.devtools.geometry.set_picker(on);
        }
        if let Some(opaque) = enable {
            self.devtools_enable_editing(opaque);
        }
        if disable {
            self.devtools_revoke_editing();
        }
    }
}

/// Edit forms for the selected node. Returns the edits the user committed this frame.
pub(crate) fn controls(
    ui: &mut Ui,
    edit: &mut EditView,
    id: InspectorNodeId,
    node: &DomNode,
) -> Vec<(InspectorNodeId, MutationOp)> {
    let mut out = Vec::new();
    if !edit.is_enabled() {
        return out;
    }
    ui.separator();
    ui.strong("Edit");
    if node.node_type == 1 {
        if !editable(node) {
            ui.weak("This element's attributes were truncated by inspection limits; editing is disabled to avoid losing data.");
        } else {
            let mut remove = None;
            for attr in &node.attributes {
                ui.horizontal(|ui| {
                    ui.monospace(&attr.name);
                    let editing = edit
                        .buffers
                        .attribute
                        .as_ref()
                        .is_some_and(|(key, name, _)| *key == id.key && *name == attr.name);
                    if editing {
                        if let Some((_, _, value)) = edit.buffers.attribute.as_mut() {
                            let response =
                                ui.add(egui::TextEdit::singleline(value).desired_width(180.0));
                            let commit = response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if ui.small_button("Apply").clicked() || commit {
                                out.push((
                                    id,
                                    MutationOp::SetAttribute {
                                        name: attr.name.clone(),
                                        value: value.clone(),
                                    },
                                ));
                            }
                        }
                        if ui.small_button("Cancel").clicked() {
                            edit.buffers.attribute = None;
                        }
                    } else {
                        ui.monospace(format!("= {:?}", attr.value));
                        if ui.small_button("Edit").clicked() {
                            edit.buffers.attribute =
                                Some((id.key, attr.name.clone(), attr.value.clone()));
                        }
                        if ui.small_button("Remove").clicked() {
                            remove = Some(attr.name.clone());
                        }
                    }
                });
            }
            if let Some(name) = remove {
                out.push((id, MutationOp::RemoveAttribute { name }));
            }
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut edit.buffers.new_name)
                        .hint_text("attribute")
                        .desired_width(100.0),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut edit.buffers.new_value)
                        .hint_text("value")
                        .desired_width(140.0),
                );
                if ui.button("Add / set attribute").clicked() {
                    out.push((
                        id,
                        MutationOp::SetAttribute {
                            name: std::mem::take(&mut edit.buffers.new_name),
                            value: std::mem::take(&mut edit.buffers.new_value),
                        },
                    ));
                }
            });
            if is_code_attribute(&edit.buffers.new_name) {
                ui.weak("Event-handler and srcdoc attributes run code: they also need evaluation to be enabled.");
            }
            // Inline style: set or remove one declaration.
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut edit.buffers.css_property)
                        .hint_text("css property")
                        .desired_width(110.0),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut edit.buffers.css_value)
                        .hint_text("value")
                        .desired_width(130.0),
                );
                ui.checkbox(&mut edit.buffers.css_important, "!important");
            });
            ui.horizontal(|ui| {
                if ui.button("Set inline style").clicked() {
                    out.push((
                        id,
                        MutationOp::SetInlineProperty {
                            property: edit.buffers.css_property.trim().to_owned(),
                            value: edit.buffers.css_value.trim().to_owned(),
                            important: edit.buffers.css_important,
                        },
                    ));
                }
                if ui.button("Remove inline declaration").clicked() {
                    out.push((
                        id,
                        MutationOp::SetInlineProperty {
                            property: edit.buffers.css_property.trim().to_owned(),
                            value: String::new(),
                            important: false,
                        },
                    ));
                }
            });
            ui.weak("Inline edits apply to the style attribute. Cached computed values refresh after the page lays out again.");
        }
    } else if matches!(node.node_type, 3 | 8) {
        if !editable(node) {
            ui.weak("This text was truncated by inspection limits; editing is disabled to avoid losing data.");
        } else {
            let buffer = match &mut edit.buffers.text {
                Some((key, text)) if *key == id.key => text,
                slot => {
                    *slot = Some((id.key, node.text.clone()));
                    &mut slot.as_mut().unwrap().1
                }
            };
            ui.add(
                egui::TextEdit::multiline(buffer)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            if ui.button("Apply text").clicked() {
                out.push((
                    id,
                    MutationOp::SetText {
                        text: buffer.clone(),
                    },
                ));
            }
        }
    }
    if !protected(node) && ui.button("Remove node").clicked() {
        out.push((id, MutationOp::RemoveNode));
    }
    out
}
