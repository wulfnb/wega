//! Inspector views backed by DOM/CSSOM snapshots from the active Servo page.
use crate::{
    browser::Browser,
    inspector_probes,
    toolbox::{Tool, pretty},
};
use egui::{RichText, Ui};
use serde_json::Value;
#[derive(Default)]
pub struct State {
    pub query: String,
    pub reveal: bool,
    pub picking: bool,
    pub filter: String,
    pub tab: usize,
    pub collapsed: std::collections::BTreeSet<String>,
    pub inline_css: String,
    pub classes: String,
    pub attribute: String,
    pub value: String,
}
fn editor(ui: &mut Ui, text: &mut String, rows: usize) {
    ui.add(
        egui::TextEdit::multiline(text)
            .code_editor()
            .desired_width(f32::INFINITY)
            .desired_rows(rows),
    );
}
fn properties(ui: &mut Ui, value: &Value, filter: &str) {
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            let line = format!("{key}: {}", value.as_str().unwrap_or(""));
            if line.to_lowercase().contains(&filter.to_lowercase()) {
                ui.add(egui::Label::new(RichText::new(line).monospace()).selectable(true));
            }
        }
    }
}
impl Browser {
    pub fn inspector_ui(&mut self, ui: &mut Ui, view: &servo::WebView, owner: &str, data: &Value) {
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            ui.label("Selector");
            ui.add(
                egui::TextEdit::singleline(&mut self.tools.page(view).selector)
                    .desired_width(220.0),
            );
            if ui.button("Inspect").clicked() {
                self.tools.refresh(view, &self.servo);
            }
            if ui
                .button("Pick element")
                .on_hover_text(
                    "Click an element on the page; Escape cancels. Times out after 9 seconds.",
                )
                .clicked()
            {
                action = Some(inspector_probes::picker());
                self.tools.page(view).inspector.picking = true;
                self.chrome.page_focused = true;
            }
            if ui.button("Highlight").clicked() {
                action = Some(crate::probes::highlight(&self.tools.page(view).selector));
            }
        });
        if self.tools.pages[owner].inspector.picking {
            ui.weak("Click an element on the page · Escape cancels");
        }
        if let Some(ancestors) = data["selected"]["ancestors"].as_array() {
            egui::ScrollArea::horizontal()
                .id_salt("inspector-breadcrumbs")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for node in ancestors {
                            if ui
                                .small_button(node["tag"].as_str().unwrap_or("?"))
                                .on_hover_text(node["selector"].as_str().unwrap_or(""))
                                .clicked()
                            {
                                self.tools.page(view).selector =
                                    node["selector"].as_str().unwrap_or("body").into();
                                self.tools.refresh(view, &self.servo);
                            }
                            ui.weak("›");
                        }
                    });
                });
        }
        if ui.available_width() >= 760.0 {
            ui.columns(2, |columns| {
                let left = columns[0].max_rect().intersect(columns[0].clip_rect());
                let right = columns[1].max_rect().intersect(columns[1].clip_rect());
                columns[0].set_clip_rect(left);
                columns[1].set_clip_rect(right);
                self.inspector_tree(&mut columns[0], view, owner, data);
                self.inspector_details(&mut columns[1], view, owner, data, &mut action);
            });
        } else {
            ui.allocate_ui(
                egui::vec2(
                    ui.available_width(),
                    (ui.available_height() * 0.35).clamp(100.0, 240.0),
                ),
                |ui| self.inspector_tree(ui, view, owner, data),
            );
            ui.separator();
            self.inspector_details(ui, view, owner, data, &mut action);
        }
        if let Some(action) = action {
            self.tools.run(view, Tool::Inspector, action);
        }
    }
    fn inspector_tree(&mut self, ui: &mut Ui, view: &servo::WebView, owner: &str, data: &Value) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.tools.page(view).inspector.query)
                    .hint_text("Search DOM tags, text, IDs, classes")
                    .desired_width((ui.available_width() - 110.0).max(50.0)),
            );
            if ui.small_button("Expand all").clicked() {
                self.tools.page(view).inspector.collapsed.clear();
            }
        });
        let colors = crate::theme::palette(ui.ctx());
        let query = self.tools.pages[owner].inspector.query.to_lowercase();
        let reveal = self.tools.pages[owner].inspector.reveal;
        self.tools.page(view).inspector.reveal = false;
        egui::ScrollArea::both()
            .id_salt("inspector-dom")
            .max_width(ui.available_width())
            .max_height((ui.available_height() - 10.0).max(40.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let mut hidden_depth = None;
                if let Some(nodes) = data["nodes"].as_array() {
                    for node in nodes {
                        let depth = node["depth"].as_u64().unwrap_or(0);
                        if let Some(hidden) = hidden_depth {
                            if depth > hidden && query.is_empty() {
                                continue;
                            }
                            hidden_depth = None;
                        }
                        let selector = node["selector"].as_str().unwrap_or("body");
                        let tag = node["tag"].as_str().unwrap_or("");
                        let id = node["id"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(|s| format!(" id=\"{s}\""))
                            .unwrap_or_default();
                        let classes = node["classes"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(|s| format!(" class=\"{s}\""))
                            .unwrap_or_default();
                        let text = node["text"]
                            .as_str()
                            .unwrap_or("")
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ");
                        if !format!("{tag}{id}{classes} {text} {selector}")
                            .to_lowercase()
                            .contains(&query)
                        {
                            continue;
                        }
                        let collapsed = self.tools.pages[owner]
                            .inspector
                            .collapsed
                            .contains(selector);
                        if collapsed && query.is_empty() {
                            hidden_depth = Some(depth);
                        }
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            ui.add_space((depth as f32 * 10.0).min(180.0));
                            if node["children"].as_u64().unwrap_or(0) > 0 {
                                if ui.small_button(if collapsed { ">" } else { "v" }).clicked() {
                                    let state = &mut self.tools.page(view).inspector;
                                    if !state.collapsed.remove(selector) {
                                        state.collapsed.insert(selector.into());
                                    }
                                }
                            } else {
                                ui.add_space(18.0);
                            }
                            let selected = self.tools.pages[owner].selector == selector;
                            let response = ui.selectable_label(
                                selected,
                                RichText::new(format!("<{tag}{id}{classes}> {text}"))
                                    .monospace()
                                    .color(if selected { colors.text } else { colors.accent }),
                            );
                            if selected && reveal {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                            response.context_menu(|ui| {
                                if ui.button("Copy selector").clicked() {
                                    ui.ctx().copy_text(selector.into());
                                    ui.close();
                                }
                                if ui.button("Copy element label").clicked() {
                                    ui.ctx().copy_text(format!("<{tag}{id}{classes}> {text}"));
                                    ui.close();
                                }
                            });
                            if response.on_hover_text(selector).clicked() {
                                self.tools.page(view).selector = selector.into();
                                self.tools.refresh(view, &self.servo);
                            }
                        });
                    }
                }
                if data["truncated"] == true {
                    ui.weak("DOM preview limited to 500 elements.");
                }
            });
    }
    fn inspector_details(
        &mut self,
        ui: &mut Ui,
        view: &servo::WebView,
        owner: &str,
        data: &Value,
        action: &mut Option<String>,
    ) {
        ui.horizontal_wrapped(|ui| {
            for (index, label) in [
                "Rules",
                "Layout",
                "Computed",
                "Attributes",
                "HTML",
                "Changes",
            ]
            .iter()
            .enumerate()
            {
                ui.selectable_value(&mut self.tools.page(view).inspector.tab, index, *label);
            }
        });
        let selected = &data["selected"];
        if selected.is_null() {
            ui.weak("No element matches the selector.");
            return;
        }
        let selector = self.tools.pages[owner].selector.clone();
        let tab = self.tools.pages[owner].inspector.tab;
        egui::ScrollArea::both().id_salt("inspector-details").max_height((ui.available_height()-8.0).max(40.0)).auto_shrink([false,false]).show(ui,|ui| {
            match tab {
                0=>{
                    ui.strong("element.style");
                    editor(ui,&mut self.tools.page(view).inspector.inline_css,4);
                    if ui.button("Apply inline CSS").clicked(){*action=Some(inspector_probes::attribute(&selector,"style",Some(&self.tools.pages[owner].inspector.inline_css)));}
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Classes");ui.text_edit_singleline(&mut self.tools.page(view).inspector.classes);
                        if ui.button("Apply classes").clicked(){*action=Some(inspector_probes::attribute(&selector,"class",Some(&self.tools.pages[owner].inspector.classes)));}
                    });
                    ui.add(egui::TextEdit::singleline(&mut self.tools.page(view).inspector.filter).hint_text("Filter rules / styles"));
                    let filter=self.tools.pages[owner].inspector.filter.to_lowercase();
                    if let Some(rules)=selected["rules"].as_array(){
                        for (index,rule) in rules.iter().enumerate().rev() {
                            if !pretty(rule).to_lowercase().contains(&filter){continue;}
                            ui.push_id(index,|ui| {
                                ui.separator();
                                ui.label(RichText::new(rule["selector"].as_str().unwrap_or("")).monospace().color(crate::theme::palette(ui.ctx()).accent));
                                ui.weak(rule["source"].as_str().unwrap_or(""));
                                if rule["context"].as_array().is_some_and(|c|!c.is_empty()){ui.weak(pretty(&rule["context"]));}
                                ui.add(egui::Label::new(RichText::new(rule["css"].as_str().unwrap_or("")).monospace()).selectable(true));
                            });
                        }
                    }
                    if selected["rulesTruncated"]==true {ui.weak("Matched rules limited to 100; stylesheet traversal limited to 3,000 rules.");}
                    if selected["inaccessible"].as_array().is_some_and(|a|!a.is_empty()){ui.weak("Some stylesheet rules cannot be read because of cross-origin restrictions.");}
                    ui.weak("Matching author rules from CSSOM. Computed values reflect the final cascade; overridden declarations and browser default rules are not identified here.");
                }
                1=>{
                    ui.strong("Box model");
                    box_model(ui,&selected["box"]);
                    ui.separator();ui.strong("Flexbox / Grid and positioning");
                    properties(ui,&selected["layout"],"");
                    if let Some(items)=selected["layoutItems"].as_array() {
                        for item in items {
                            let target=item["selector"].as_str().unwrap_or("body");
                            if ui.small_button(format!("Inspect {target}")).clicked(){self.tools.page(view).selector=target.into();self.tools.refresh(view,&self.servo);}
                        }
                    }
                }
                2=>{
                    ui.add(egui::TextEdit::singleline(&mut self.tools.page(view).inspector.filter).hint_text("Filter computed properties"));
                    let filter=self.tools.pages[owner].inspector.filter.to_lowercase();
                    if let Some(items)=selected["computed"].as_array() {
                        for item in items {
                            let line=format!("{}: {}",item["name"].as_str().unwrap_or(""),item["value"].as_str().unwrap_or(""));
                            if line.to_lowercase().contains(&filter){ui.add(egui::Label::new(RichText::new(line).monospace()).selectable(true));}
                        }
                    }
                }
                3=>{
                    if let Some(items)=selected["attributes"].as_array(){for (index,item) in items.iter().enumerate(){ui.push_id(index,|ui| {ui.horizontal_wrapped(|ui| {
                        let name=item["name"].as_str().unwrap_or("");let value=item["value"].as_str().unwrap_or("");
                        if ui.small_button("Edit").clicked(){let state=&mut self.tools.page(view).inspector;state.attribute=name.into();state.value=value.into();}
                        if ui.small_button("Remove").clicked(){*action=Some(inspector_probes::attribute(&selector,name,None));}
                        ui.monospace(format!("{name}=\"{value}\""));
                    });});}}
                    ui.separator();ui.label("Attribute name");ui.text_edit_singleline(&mut self.tools.page(view).inspector.attribute);
                    ui.label("Value");editor(ui,&mut self.tools.page(view).inspector.value,2);
                    if ui.add_enabled(!self.tools.pages[owner].inspector.attribute.trim().is_empty(),egui::Button::new("Set attribute")).clicked(){
                        let state=&self.tools.pages[owner].inspector;
                        *action=Some(inspector_probes::attribute(&selector,state.attribute.trim(),Some(&state.value)));
                    }
                }
                4=>{
                    if ui.button("Copy outer HTML").clicked(){ui.ctx().copy_text(self.tools.pages[owner].html.clone());}
                    editor(ui,&mut self.tools.page(view).html,8);
                    let truncated=selected["htmlTruncated"]==true;
                    if truncated {ui.weak("HTML preview truncated; select a smaller element to edit.");}
                    if ui.add_enabled(!truncated,egui::Button::new("Apply outer HTML")).clicked(){*action=Some(crate::probes::edit_html(&selector,&self.tools.pages[owner].html));}
                }
                _=>{
                    let changes=data["changes"].as_array();
                    ui.weak("Attribute, class and inline CSS edits from this Inspector. Last 100 edits; reset on navigation. HTML replacement is not undoable here.");
                    if ui.add_enabled(changes.is_some_and(|a|!a.is_empty()),egui::Button::new("Undo last edit")).clicked(){*action=Some(inspector_probes::undo(&selector));}
                    if let Some(changes)=changes{for change in changes.iter().rev(){ui.monospace(change.as_str().unwrap_or(""));}}
                }
            }
        });
    }
}
fn box_model(ui: &mut Ui, box_value: &Value) {
    // Nested like the CSS box model: margin around border around padding
    // around content, in theme colours so it reads on light and dark themes.
    let colors = crate::theme::palette(ui.ctx());
    let line = |label: &str, key: &str| {
        let sides = &box_value[key];
        egui::RichText::new(format!(
            "{label}: {}  {}  {}  {}",
            sides["top"].as_str().unwrap_or("0"),
            sides["right"].as_str().unwrap_or("0"),
            sides["bottom"].as_str().unwrap_or("0"),
            sides["left"].as_str().unwrap_or("0")
        ))
        .small()
        .color(colors.text)
    };
    egui::Frame::new()
        .fill(colors.warning_bg)
        .corner_radius(6)
        .inner_margin(6)
        .show(ui, |ui| {
            ui.label(line("margin", "margin"));
            egui::Frame::new()
                .stroke(egui::Stroke::new(2.0, colors.amber))
                .corner_radius(4)
                .inner_margin(6)
                .show(ui, |ui| {
                    ui.label(line("border", "border"));
                    egui::Frame::new()
                        .fill(colors.selected)
                        .corner_radius(4)
                        .inner_margin(6)
                        .show(ui, |ui| {
                            ui.label(line("padding", "padding"));
                            egui::Frame::new()
                                .fill(colors.surface)
                                .corner_radius(4)
                                .inner_margin(6)
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("content").small().color(colors.muted),
                                    );
                                });
                        });
                });
        });
    ui.monospace(format!(
        "{:.2} × {:.2} px · x {:.2}, y {:.2}",
        box_value["width"].as_f64().unwrap_or(0.0),
        box_value["height"].as_f64().unwrap_or(0.0),
        box_value["x"].as_f64().unwrap_or(0.0),
        box_value["y"].as_f64().unwrap_or(0.0)
    ));
    ui.weak("Dimensions above are the border box in viewport CSS pixels. Insets are top / right / bottom / left.");
}
