//! Native console controls. Structured trees are bounded snapshots, never automatic getter calls.
use crate::{browser::Browser, console_model, toolbox::Tool};
use egui::{RichText, Ui};
use serde_json::Value;
use std::collections::HashMap;
pub fn value_ui(
    ui: &mut Ui,
    label: &str,
    value: &Value,
    objects: &HashMap<String, Value>,
    depth: usize,
) {
    if depth >= 8 {
        ui.monospace(format!("{label}: [preview depth limit]"));
        return;
    }
    if let Some(object) = value.get("ObjectValue") {
        let stored = object["actor"].as_str().and_then(|id| objects.get(id));
        let object = if object["preview"].is_null() {
            stored.and_then(|v| v.get("ObjectValue")).unwrap_or(object)
        } else {
            object
        };
        egui::CollapsingHeader::new(RichText::new(format!("{label}: {}",object["class"].as_str().unwrap_or("Object"))).monospace()).show(ui,|ui| {
            let preview=&object["preview"];
            if let Some(props)=preview["ownProperties"].as_array(){for (index,p) in props.iter().take(100).enumerate(){ui.push_id(index,|ui|{
                let name=p["name"].as_str().unwrap_or("?");
                if p["isAccessor"]==true {ui.monospace(format!("{name}: [accessor — not invoked]"));}else{value_ui(ui,name,&p["value"],objects,depth+1);}
            });}}
            if let Some(items)=preview["items"].as_array(){for(index,item)in items.iter().take(100).enumerate(){ui.push_id(index,|ui|value_ui(ui,&index.to_string(),item,objects,depth+1));}}
            if let Some(items)=preview["entries"].as_array(){for(index,item)in items.iter().take(100).enumerate(){ui.push_id(index,|ui|value_ui(ui,&format!("entry {index}"),item,objects,depth+1));}}
            ui.weak("Engine-captured preview; up to 100 properties/items per expansion. Getters and additional live properties are not requested.");
        });
    } else if value.get("StringValue").is_some()
        || value.get("NumberValue").is_some()
        || value.get("BooleanValue").is_some()
        || value.get("NullValue").is_some()
        || value == "VoidValue"
    {
        ui.add(
            egui::Label::new(
                RichText::new(format!("{label}: {}", console_model::value_text(value))).monospace(),
            )
            .selectable(true),
        );
    } else if value.is_array() || value.is_object() {
        let count = value
            .as_array()
            .map(|v| v.len())
            .or_else(|| value.as_object().map(|v| v.len()))
            .unwrap_or(0);
        egui::CollapsingHeader::new(RichText::new(format!("{label} ({count})")).monospace()).show(
            ui,
            |ui| {
                if let Some(items) = value.as_array() {
                    for (index, item) in items.iter().take(100).enumerate() {
                        ui.push_id(index, |ui| {
                            value_ui(ui, &index.to_string(), item, objects, depth + 1)
                        });
                    }
                }
                if let Some(items) = value.as_object() {
                    for (name, item) in items.iter().take(100) {
                        ui.push_id(name, |ui| value_ui(ui, name, item, objects, depth + 1));
                    }
                }
            },
        );
    } else {
        ui.add(
            egui::Label::new(
                RichText::new(format!(
                    "{label}: {}",
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string())
                ))
                .monospace(),
            )
            .selectable(true),
        );
    }
}
fn timestamp(time: u64) -> String {
    chrono::DateTime::from_timestamp_millis(time.min(i64::MAX as u64) as i64)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%H:%M:%S%.3f")
                .to_string()
        })
        .unwrap_or_default()
}
impl Browser {
    pub fn console_ui(&mut self, ui: &mut Ui, view: &servo::WebView, owner: &str) {
        let colors = crate::theme::palette(ui.ctx());
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::K)) {
            self.tools.log.clear(owner);
            self.console.retain(|(id, _)| *id != view.id());
            self.tools.page(view).console_results.clear();
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Clear").clicked(){self.tools.log.clear(owner);self.console.retain(|(id,_)|*id!=view.id());self.tools.page(view).console_results.clear();}
            ui.add(egui::TextEdit::singleline(&mut self.tools.page(view).console_state.query).hint_text("Filter output · -term excludes").desired_width(210.0));
            for level in ["Error","Warn","Info","Log","Debug"] {
                let count=self.tools.log.entries.iter().filter(|e|e.owner==owner&&e.level==level).count();
                let selected=self.tools.pages[owner].console_state.levels.contains(level);
                if ui.selectable_label(selected,format!("{level} ({count})")).clicked(){let levels=&mut self.tools.page(view).console_state.levels;if !levels.remove(level){levels.insert(level.into());}}
            }
            ui.menu_button("Options",|ui|{
                ui.checkbox(&mut self.tools.log.preserve,"Persist logs across navigation (all tabs)");
                let native_available=self.tools.debug.contains_key(owner);
                let state=&mut self.tools.page(view).console_state;
                ui.checkbox(&mut state.timestamps,"Timestamps");ui.checkbox(&mut state.group_repeats,"Group consecutive identical messages");ui.checkbox(&mut state.follow,"Follow latest output");ui.checkbox(&mut state.multiline,"Multiline editor");ui.add_enabled(native_available,egui::Checkbox::new(&mut state.native_evaluation,"Native evaluator (no promise awaiting)"));
                ui.separator();ui.label("Categories");
                for category in ["JS","CSS","Input","Result"] {let mut enabled=state.categories.contains(category);if ui.checkbox(&mut enabled,category).changed(){if enabled{state.categories.insert(category.into());}else{state.categories.remove(category);}}}
                ui.separator();
                if ui.button("Copy visible output").clicked(){let text=self.visible_console_text(owner);ui.ctx().copy_text(text);ui.close();}
                if ui.button("Save visible output…").clicked(){self.tools.page(view).console_state.export_open=true;self.tools.page(view).console_state.export_path=self.profile_dir.join("console.log").display().to_string();ui.close();}
                ui.weak("Worker-specific evaluation, CSS/XHR interception breakpoints and eager evaluation are unavailable.");
            });
        });
        let paused = self.tools.debug.get(owner).is_some_and(|d| d.paused);
        ui.horizontal_wrapped(|ui| {
            ui.weak(if paused {
                "Evaluate in selected paused frame · native debugger"
            } else if self.tools.pages[owner].console_state.native_evaluation {
                "Native main-page evaluation · synchronous results · Up/Down history"
            } else {
                "Evaluate in main page · promises / await expressions · Up/Down history"
            });
            let multiline = self.tools.pages[owner].console_state.multiline;
            let edit = if multiline {
                egui::TextEdit::multiline(&mut self.tools.page(view).console_input).desired_rows(4)
            } else {
                egui::TextEdit::singleline(&mut self.tools.page(view).console_input)
            };
            let input = ui.add(
                edit.font(egui::TextStyle::Monospace)
                    .desired_width((ui.available_width() - 65.0).max(60.0)),
            );
            if input.has_focus() && !multiline {
                let back = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
                let forward = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
                if back || forward {
                    let page = self.tools.page(view);
                    page.console_state
                        .history_move(&mut page.console_input, back);
                }
            }
            let enter = if multiline {
                input.has_focus()
                    && ui.input(|i| {
                        i.key_pressed(egui::Key::Enter) && (i.modifiers.command || i.modifiers.ctrl)
                    })
            } else {
                input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
            };
            if enter || ui.button("Run").clicked() {
                self.tools.console(view);
                input.request_focus();
            }
        });
        ui.horizontal_wrapped(|ui|{
            let input=&self.tools.pages[owner].console_input;
            let identifier=input.chars().all(|c|c.is_alphanumeric()||c=='_'||c=='$');
            if ui.add_enabled(paused&&identifier,egui::Button::new("Suggest scope identifiers")).on_hover_text("Matches identifiers in captured paused scopes; does not evaluate getters or expressions.").clicked(){
                let names=self.tools.scope_identifiers(owner,input);self.tools.page(view).console_state.completions=names;
            }
            let names=self.tools.pages[owner].console_state.completions.clone();
            for name in names.iter().take(20){if ui.small_button(name).clicked(){let page=self.tools.page(view);page.console_input=name.clone();page.console_state.completions.clear();}}
        });
        let state = &self.tools.pages[owner].console_state;
        let rows: Vec<_> = self
            .tools
            .log
            .entries
            .iter()
            .filter(|e| e.owner == owner && state.matches(e))
            .cloned()
            .collect();
        let mut grouped: Vec<(console_model::Entry, usize)> = Vec::new();
        for row in rows {
            if state.group_repeats
                && let Some((last, count)) = grouped.last_mut()
                && last.same_message(&row)
            {
                *count += 1;
            } else {
                grouped.push((row, 1));
            }
        }
        // Capture options before taking mutable actions inside the output UI.
        let times = state.timestamps;
        let follow = state.follow;
        let mut jump = None;
        let mut style_jump = None;
        let live: std::collections::HashSet<_> = self
            .tools
            .log
            .entries
            .iter()
            .filter(|e| e.owner == owner)
            .map(|e| e.id)
            .collect();
        self.tools
            .page(view)
            .console_state
            .expanded
            .retain(|id| live.contains(id));
        let output_width = ui.available_width();
        egui::ScrollArea::both()
            .id_salt("console-output")
            .max_width(output_width)
            .max_height((ui.available_height() - 22.0).max(30.0))
            .auto_shrink([false, false])
            .stick_to_bottom(follow)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if grouped.is_empty() {
                    if live.is_empty() {
                        crate::components::empty_state(
                            ui,
                            "No console messages yet",
                            "Messages from this page appear here.",
                            None,
                        );
                    } else {
                        crate::components::empty_state(
                            ui,
                            "No messages match the filters",
                            "Change the level or search filters to see them.",
                            None,
                        );
                    }
                }
                for (entry, count) in &grouped {
                    let color = match entry.level.as_str() {
                        "Error" => colors.danger,
                        "Warn" => colors.amber,
                        _ => colors.text,
                    };
                    ui.push_id(entry.id, |ui| {
                        egui::Frame::new()
                            .fill(match entry.level.as_str() {
                                "Error" => colors.danger_bg,
                                "Warn" => colors.warning_bg,
                                _ => egui::Color32::TRANSPARENT,
                            })
                            .inner_margin(4)
                            .show(ui, |ui| {
                                ui.set_min_width((output_width - 24.0).max(80.0));
                                let mut expanded = self.tools.pages[owner]
                                    .console_state
                                    .expanded
                                    .contains(&entry.id);
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = 5.0;
                                    if !entry.details.is_null() {
                                        if ui
                                            .small_button(if expanded { "v" } else { ">" })
                                            .on_hover_text(
                                                "Expand arguments, object previews and stack trace",
                                            )
                                            .clicked()
                                        {
                                            expanded = !expanded;
                                            let state =
                                                &mut self.tools.page(view).console_state.expanded;
                                            if expanded {
                                                state.insert(entry.id);
                                            } else {
                                                state.remove(&entry.id);
                                            }
                                        }
                                    } else {
                                        ui.add_space(18.0);
                                    }
                                    if times {
                                        ui.label(
                                            RichText::new(timestamp(entry.time))
                                                .small()
                                                .color(colors.muted),
                                        );
                                    }
                                    ui.label(RichText::new(&entry.level).small().color(color));
                                    let source_width = if entry.source.is_empty() {
                                        0.0
                                    } else {
                                        (output_width * 0.23).clamp(95.0, 210.0)
                                    };
                                    let text_width = (ui.available_width()
                                        - source_width
                                        - if *count > 1 { 40.0 } else { 0.0 }
                                        - 12.0)
                                        .max(60.0);
                                    let response = ui
                                        .allocate_ui_with_layout(
                                            egui::vec2(text_width, 0.0),
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(&entry.text)
                                                            .monospace()
                                                            .color(color),
                                                    )
                                                    .wrap()
                                                    .selectable(true),
                                                )
                                            },
                                        )
                                        .inner;
                                    response.context_menu(|ui| {
                                        if ui.button("Copy message").clicked() {
                                            ui.ctx().copy_text(entry.text.clone());
                                            ui.close();
                                        }
                                        if ui.button("Copy structured record").clicked() {
                                            ui.ctx()
                                                .copy_text(crate::toolbox::pretty(&entry.details));
                                            ui.close();
                                        }
                                    });
                                    if *count > 1 {
                                        ui.strong(format!("×{count}"));
                                    }
                                    if !entry.source.is_empty() {
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(source_width, 0.0),
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                let name = entry
                                                    .source
                                                    .rsplit('/')
                                                    .next()
                                                    .unwrap_or(&entry.source);
                                                if ui
                                                    .link(format!(
                                                        "{name}:{}:{}",
                                                        entry.line, entry.column
                                                    ))
                                                    .on_hover_text(&entry.source)
                                                    .clicked()
                                                {
                                                    if entry.category == "CSS" {
                                                        style_jump = Some((
                                                            entry.source.clone(),
                                                            entry.line,
                                                        ));
                                                    } else {
                                                        jump = Some((
                                                            entry.source.clone(),
                                                            entry.line,
                                                        ));
                                                    }
                                                }
                                            },
                                        );
                                    }
                                });
                                if expanded {
                                    ui.indent("console-details", |ui| {
                                        if let Some(args) = entry.details["arguments"].as_array() {
                                            for (index, value) in args.iter().enumerate() {
                                                ui.push_id(index, |ui| {
                                                    value_ui(
                                                        ui,
                                                        &format!("argument {index}"),
                                                        value,
                                                        &self.tools.objects,
                                                        0,
                                                    )
                                                });
                                            }
                                        } else {
                                            value_ui(
                                                ui,
                                                "result",
                                                &entry.details,
                                                &self.tools.objects,
                                                0,
                                            );
                                        }
                                        if let Some(stack) = entry.details["stacktrace"].as_array()
                                        {
                                            for frame in stack.iter().take(50) {
                                                let file = frame["filename"].as_str().unwrap_or("");
                                                let line = frame["lineNumber"].as_u64().unwrap_or(0)
                                                    as u32;
                                                let name =
                                                    frame["functionName"].as_str().unwrap_or("");
                                                if ui
                                                    .link(format!("{name} · {file}:{line}"))
                                                    .clicked()
                                                {
                                                    jump = Some((file.into(), line));
                                                }
                                            }
                                        }
                                    });
                                }
                            });
                    });
                }
            });
        ui.weak(format!(
            "{} visible groups · {} records for this tab · 1,000 records retained across tabs",
            grouped.len(),
            self.tools
                .log
                .entries
                .iter()
                .filter(|e| e.owner == owner)
                .count()
        ));
        let dropped = self
            .tools
            .bridge
            .dropped
            .load(std::sync::atomic::Ordering::Relaxed);
        if dropped > 0 {
            ui.colored_label(colors.amber,format!("{dropped} native developer events dropped under load; capture may be incomplete."));
        }
        if let Some((source, line)) = style_jump {
            self.tools.active = Tool::StyleEditor;
            self.tools.refresh(view, &self.servo);
            self.tools.notice = Some(format!(
                "CSS source: {source}:{line}. Choose its stylesheet in Style Editor."
            ));
        }
        if let Some((source, line)) = jump {
            self.jump_to_source(owner, &source, line);
        }
        if self.tools.pages[owner].console_state.export_open {
            let mut open = true;
            let mut save = false;
            egui::Window::new("Save console output")
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    ui.label("Saves currently filtered messages and source locations as text.");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(
                            &mut self.tools.page(view).console_state.export_path,
                        );
                        if ui.button("Choose…").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_file_name("console.txt")
                                .save_file()
                        {
                            self.tools.page(view).console_state.export_path =
                                path.display().to_string();
                        }
                    });
                    if let Some(error) = &self.tools.error {
                        crate::components::inline_message(ui, crate::ux::Severity::Error, error);
                    }
                    save = ui.button("Save").clicked();
                });
            if save {
                let result = std::fs::write(
                    &self.tools.pages[owner].console_state.export_path,
                    self.visible_console_text(owner),
                );
                // On failure keep the dialog and the typed path.
                match result {
                    Ok(()) => {
                        self.tools.notice = Some("Console output saved".into());
                        self.tools.error = None;
                        open = false;
                    }
                    Err(e) => {
                        self.tools.error = Some(format!("Could not save console output: {e}"))
                    }
                }
            }
            self.tools.page(view).console_state.export_open = open;
        }
    }
    fn visible_console_text(&self, owner: &str) -> String {
        let state = &self.tools.pages[owner].console_state;
        self.tools
            .log
            .entries
            .iter()
            .filter(|e| e.owner == owner && state.matches(e))
            .map(|e| {
                format!(
                    "{} [{} / {}] {} {}:{}:{}",
                    timestamp(e.time),
                    e.level,
                    e.category,
                    e.text,
                    e.source,
                    e.line,
                    e.column
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn jump_to_source(&mut self, owner: &str, url: &str, line: u32) {
        if let Some(debug) = self.tools.debug.get_mut(owner)
            && let Some(index) = debug.sources.iter().position(|s| s.url.as_str() == url)
        {
            debug.selected = index;
            debug.advanced.goto = Some(line.max(1));
            debug.line = line.max(1).to_string();
            self.tools.active = Tool::Debugger;
        } else {
            self.tools.notice=Some("Source text was not captured for this location. Reload the page with native developer tools enabled.".into());
        }
    }
}

#[cfg(test)]
#[path = "console_ui_tests.rs"]
mod tests;
