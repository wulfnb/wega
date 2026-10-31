//! Native debugger workspace: sources, executable breakpoints, call frames, scopes and watches.
use crate::{browser::Browser, debug_model::Action, toolbox::DebugPage};
use egui::{RichText, Ui};
use serde_json::Value;
#[derive(Default)]
struct Actions {
    breakpoint: Option<(u32, u32)>,
    enabled: Vec<(u32, u32, bool)>,
    all: Option<bool>,
    clear: bool,
    ignore: Option<(u32, bool)>,
    frame: Option<String>,
    refresh: bool,
    watches: bool,
    remove: Vec<u64>,
}
impl Browser {
    pub fn debugger_ui(&mut self, ui: &mut Ui, owner: &str) {
        let colors = crate::theme::palette(ui.ctx());
        let mut action = Actions::default();
        let mut command = None;
        let mut resume = None;
        let Some(debug) = self.tools.debug.get_mut(owner) else {
            ui.label(
                "Waiting for the main page's native script context. Reload to capture sources.",
            );
            return;
        };
        if debug.paused {
            egui::Frame::new()
                .fill(colors.warning_bg)
                .inner_margin(6)
                .show(ui, |ui| {
                    ui.colored_label(colors.amber, format!("Paused · {}", debug.location));
                });
        }
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!debug.paused,egui::Button::new("Pause")).on_hover_text("Pause (F8)").clicked(){command=Some(devtools_traits::DevtoolScriptControlMsg::Interrupt);}
            if ui.add_enabled(debug.paused,egui::Button::new("Resume")).on_hover_text("Resume (F8)").on_disabled_hover_text("Resume (F8) — available while paused").clicked(){resume=Some(None);}
            for(label,kind,key)in [("Step over","next","F10"),("Step in","step","F11"),("Step out","finish","Shift+F11")] {if ui.add_enabled(debug.paused,egui::Button::new(label)).on_hover_text(format!("{label} ({key})")).on_disabled_hover_text(format!("{label} ({key}) — available while paused")).clicked(){resume=Some(Some(kind));}}
            let mut enabled=!debug.advanced.all_disabled;if ui.checkbox(&mut enabled,"Breakpoints enabled").changed(){action.all=Some(enabled);}
            if ui.add_enabled(debug.paused,egui::Button::new("Refresh stack/scopes")).clicked(){action.refresh=true;}
            ui.weak(if debug.paused{"Selected paused frame"}else{"Running · main page"});
            ui.menu_button("Capabilities",|ui|{ui.label("Native: pause/step, frame evaluation, call stack, lexical scopes, watches, executable breakpoints and whole-source ignore listing.");ui.separator();ui.weak("Not exposed here: pause-on-exception settings, conditional/log/XHR/event/DOM breakpoints, source maps, source-mapped pretty-printing and worker debugger targets.");});
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut debug.advanced.search)
                    .hint_text("Find text in source files")
                    .desired_width(220.0),
            );
            ui.checkbox(&mut debug.advanced.search_all, "All sources");
            if ui.button("Search").clicked() {
                let (hits, truncated) = crate::debug_model::search(
                    &debug.sources,
                    &debug.advanced.search,
                    if debug.advanced.search_all {
                        None
                    } else {
                        Some(debug.selected)
                    },
                );
                debug.advanced.hits = hits;
                debug.advanced.search_truncated = truncated;
            }
            if ui.button("Clear search").clicked() {
                debug.advanced.hits.clear();
                debug.advanced.search.clear();
            }
        });
        if ui.available_width() < 900.0 {
            ui.horizontal(|ui| {
                for (index, label) in ["Code", "Sources / Search", "Stack / Scopes / Watches"]
                    .iter()
                    .enumerate()
                {
                    ui.selectable_value(&mut debug.advanced.sidebar, index, *label);
                }
            });
            match debug.advanced.sidebar {
                1 => sources(ui, debug),
                2 => context(
                    ui,
                    debug,
                    &self.tools.environments,
                    &self.tools.objects,
                    &mut action,
                ),
                _ => code(ui, debug, &mut action),
            }
        } else {
            let height = ui.available_height();
            let width = ui.available_width();
            ui.horizontal_top(|ui| {
                for (index, pane_width) in [180.0, (width - 490.0).max(240.0), 290.0]
                    .into_iter()
                    .enumerate()
                {
                    ui.allocate_ui_with_layout(
                        egui::vec2(pane_width, height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                            match index {
                                0 => sources(ui, debug),
                                1 => code(ui, debug, &mut action),
                                _ => context(
                                    ui,
                                    debug,
                                    &self.tools.environments,
                                    &self.tools.objects,
                                    &mut action,
                                ),
                            }
                        },
                    );
                }
            });
        }
        let view = self.view().clone();
        if let Some((source, line)) = action.breakpoint {
            self.tools.toggle_breakpoint(&view, source, line);
        }
        if let Some(enabled) = action.all {
            self.tools.set_breakpoints_enabled(owner, enabled);
        }
        for (source, line, enabled) in action.enabled {
            self.tools
                .set_breakpoint_enabled(owner, source, line, enabled);
        }
        if action.clear {
            self.tools.clear_breakpoints(owner);
        }
        if let Some((source, ignored)) = action.ignore {
            self.tools.ignore_source(owner, source, ignored);
        }
        if let Some(frame) = action.frame {
            self.tools.select_debug_frame(owner, frame);
        }
        if action.refresh {
            self.tools.refresh_debug_context(owner);
        }
        for id in action.remove {
            self.tools
                .debug
                .get_mut(owner)
                .unwrap()
                .advanced
                .watches
                .retain(|w| w.id != id);
            self.tools.driver.queue.retain(|r| {
                r.owner != owner || !matches!(r.action,Action::Eval{watch:Some(w),..} if w==id)
            });
        }
        if action.watches {
            let watches: Vec<_> = self.tools.debug[owner]
                .advanced
                .watches
                .iter()
                .map(|w| (w.id, w.expression.clone()))
                .collect();
            for (id, expression) in watches {
                self.tools.queue_debug(
                    owner,
                    Action::Eval {
                        expression,
                        console: false,
                        watch: Some(id),
                    },
                );
            }
        }
        if let Some(command) = command {
            self.tools.command(owner, command);
        }
        if let Some(kind) = resume {
            self.tools.resume(owner, kind);
        }
    }
}
fn sources(ui: &mut Ui, debug: &mut DebugPage) {
    ui.add(
        egui::TextEdit::singleline(&mut debug.advanced.source_filter)
            .hint_text("Find file / domain")
            .desired_width(ui.available_width()),
    );
    let filter = debug.advanced.source_filter.to_lowercase();
    egui::ScrollArea::both()
        .id_salt("debugger-source-list")
        .max_height(ui.available_height())
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut groups = std::collections::BTreeMap::<String, Vec<usize>>::new();
            for (index, source) in debug.sources.iter().enumerate() {
                if source.url.as_str().to_lowercase().contains(&filter) {
                    let domain = url::Url::parse(source.url.as_str())
                        .ok()
                        .and_then(|u| u.host_str().map(str::to_owned))
                        .unwrap_or_else(|| "Inline / eval".into());
                    groups.entry(domain).or_default().push(index);
                }
            }
            for (domain, indices) in groups {
                egui::CollapsingHeader::new(domain)
                    .default_open(true)
                    .show(ui, |ui| {
                        for index in indices {
                            let source = &debug.sources[index];
                            let name = source
                                .url
                                .as_str()
                                .rsplit('/')
                                .next()
                                .filter(|s| !s.is_empty())
                                .unwrap_or(source.url.as_str());
                            if ui
                                .selectable_label(
                                    debug.selected == index,
                                    format!(
                                        "{}{}",
                                        if debug.advanced.ignored.contains(&source.spidermonkey_id)
                                        {
                                            "[ignored] "
                                        } else {
                                            ""
                                        },
                                        name
                                    ),
                                )
                                .on_hover_text(source.url.as_str())
                                .clicked()
                            {
                                debug.selected = index;
                                debug.advanced.sidebar = 0;
                            }
                        }
                    });
            }
            ui.separator();
            ui.strong(format!("Search results ({})", debug.advanced.hits.len()));
            for hit in &debug.advanced.hits {
                if ui
                    .small_button(format!(
                        "{}:{} {}",
                        debug.sources[hit.source]
                            .url
                            .as_str()
                            .rsplit('/')
                            .next()
                            .unwrap_or("source"),
                        hit.line,
                        hit.text
                    ))
                    .clicked()
                {
                    debug.selected = hit.source;
                    debug.advanced.goto = Some(hit.line);
                    debug.line = hit.line.to_string();
                    debug.advanced.sidebar = 0;
                }
            }
            if debug.advanced.search_truncated {
                ui.weak("Search bounded to 500 matches / 16 MiB of source text.");
            }
            ui.weak("Captured sources: up to 250, with at most 1 MiB of text each.");
        });
}
fn code(ui: &mut Ui, debug: &mut DebugPage, action: &mut Actions) {
    let colors = crate::theme::palette(ui.ctx());
    let Some(source) = debug.sources.get(debug.selected) else {
        ui.weak("Reload the page to capture source files.");
        return;
    };
    ui.label(RichText::new(source.url.as_str()).small());
    ui.horizontal_wrapped(|ui| {
        ui.label("Line");
        ui.add(egui::TextEdit::singleline(&mut debug.line).desired_width(50.0));
        if ui.button("Go").clicked()
            && let Ok(line) = debug.line.parse::<u32>()
        {
            debug.advanced.goto = Some(line.max(1));
        }
        if ui.button("Toggle breakpoint").clicked()
            && let Ok(line) = debug.line.parse::<u32>()
        {
            action.breakpoint = Some((source.spidermonkey_id, line));
        }
        let mut ignored = debug.advanced.ignored.contains(&source.spidermonkey_id);
        if ui.checkbox(&mut ignored, "Ignore source").changed() {
            action.ignore = Some((source.spidermonkey_id, ignored));
        }
        if ui.button("Copy source").clicked()
            && let Some(content) = &source.content
        {
            ui.ctx().copy_text(content.clone());
        }
    });
    let Some(content) = &source.content else {
        ui.weak("Engine did not report source text.");
        return;
    };
    let lines: Vec<_> = content.lines().collect();
    let width = (lines
        .iter()
        .map(|line| line.chars().count().min(20000))
        .max()
        .unwrap_or(0) as f32
        * 7.3
        + 64.0)
        .max(ui.available_width());
    let mut scroll = egui::ScrollArea::both()
        .id_salt(("debugger-code", source.spidermonkey_id))
        .max_height(ui.available_height())
        .max_width(ui.available_width())
        .auto_shrink([false, false]);
    if let Some(line) = debug.advanced.goto.take() {
        scroll = scroll
            .vertical_scroll_offset((line.clamp(1, lines.len().max(1) as u32) - 1) as f32 * 18.0);
    }
    scroll.show_rows(ui, 18.0, lines.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for index in range {
            let line = (index + 1) as u32;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
            let paused = debug.paused
                && debug.paused_source == Some(source.spidermonkey_id)
                && debug.paused_line == Some(line);
            if paused {
                ui.painter().rect_filled(rect, 0, colors.warning_bg);
            }
            let gutter = egui::Rect::from_min_size(rect.min, egui::vec2(52.0, 18.0));
            let response = ui.interact(
                gutter,
                ui.scope_id()
                    .with(("breakpoint", source.spidermonkey_id, line)),
                egui::Sense::click(),
            );
            let marked = debug
                .breakpoints
                .iter()
                .any(|p| p.0 == source.spidermonkey_id && p.1 == line);
            let disabled = debug.advanced.all_disabled
                || debug
                    .advanced
                    .disabled
                    .contains(&(source.spidermonkey_id, line));
            if marked || response.hovered() {
                ui.painter().circle_filled(
                    gutter.min + egui::vec2(7.0, 9.0),
                    3.5,
                    if marked && !disabled {
                        colors.accent
                    } else {
                        colors.muted
                    },
                );
            }
            ui.painter().text(
                gutter.right_top() - egui::vec2(7.0, -1.0),
                egui::Align2::RIGHT_TOP,
                line.to_string(),
                egui::FontId::monospace(12.0),
                if paused { colors.amber } else { colors.muted },
            );
            if response
                .on_hover_text("Toggle executable breakpoint")
                .clicked()
            {
                action.breakpoint = Some((source.spidermonkey_id, line));
            }
            ui.painter()
                .with_clip_rect(rect.intersect(ui.clip_rect()))
                .text(
                    rect.min + egui::vec2(58.0, 1.0),
                    egui::Align2::LEFT_TOP,
                    crate::console_model::bounded(&lines[index].replace('\t', "    "), 20000),
                    egui::FontId::monospace(12.0),
                    colors.text,
                );
        }
    });
}
fn context(
    ui: &mut Ui,
    debug: &mut DebugPage,
    environments: &std::collections::HashMap<String, Value>,
    objects: &std::collections::HashMap<String, Value>,
    action: &mut Actions,
) {
    egui::ScrollArea::both().id_salt("debugger-context").max_height(ui.available_height()).auto_shrink([false,false]).show(ui,|ui|{
        ui.strong("Main page thread");
        ui.collapsing("Call stack",|ui|{
            if !debug.paused{ui.weak("Pause execution to inspect frames.");}
            for actor in &debug.advanced.stack{
                let frame=debug.frames.iter().find(|(name,_)|name==actor).map(|(_,v)|v);
                let label=frame.and_then(|v|v["displayName"].as_str()).unwrap_or("anonymous frame");
                if ui.selectable_label(debug.frame.as_ref()==Some(actor),label).on_hover_text(frame.and_then(|v|v["url"].as_str()).unwrap_or("")).clicked(){action.frame=Some(actor.clone());}
            }
        });
        ui.collapsing("Scopes",|ui|{
            if !debug.paused{ui.weak("Scope values are available while paused.");}
            let mut current=debug.advanced.environment.clone();let mut seen=std::collections::HashSet::new();
            while let Some(actor)=current {
                if seen.len()>=20 || !seen.insert(actor.clone()){break;}
                let Some(scope)=environments.get(&actor)else{ui.weak("Waiting for native environment data…");break;};
                let info=&scope["info"];
                ui.push_id(&actor,|ui|{
                    ui.collapsing(format!("{} {}",info["type"].as_str().or(info["type_"] .as_str()).unwrap_or("environment"),info["functionDisplayName"].as_str().unwrap_or("")),|ui|{
                        if let Some(bindings)=info["bindingVariables"].as_array(){for (index,binding)in bindings.iter().take(200).enumerate(){ui.push_id(index,|ui|crate::console_ui::value_ui(ui,binding["name"].as_str().unwrap_or("?"),&binding["value"],objects,0));}}
                        if !info["object"].is_null(){crate::console_ui::value_ui(ui,"object",&info["object"],objects,0);}
                    });
                });
                current=scope["parent"].as_str().map(str::to_owned);
            }
        });
        ui.separator();ui.strong("Watch expressions");
        ui.horizontal_wrapped(|ui|{
            ui.add(egui::TextEdit::singleline(&mut debug.advanced.watch_input).hint_text("Expression").desired_width(165.0));
            if ui.add_enabled(!debug.advanced.watch_input.trim().is_empty()&&debug.advanced.watches.len()<20&&debug.advanced.watch_input.len()<=4096,egui::Button::new("Add")).clicked(){debug.advanced.watch_next+=1;debug.advanced.watches.push(crate::debug_model::Watch{id:debug.advanced.watch_next,expression:std::mem::take(&mut debug.advanced.watch_input),result:None});action.watches=true;}
            if ui.button("Evaluate watches").clicked(){action.watches=true;}
        });
        ui.weak("Evaluates code in the selected paused frame, or the main page while running. Up to 20 watches; refreshed on pause.");
        for watch in &debug.advanced.watches {
            ui.push_id(watch.id,|ui|{
                ui.horizontal_wrapped(|ui|{if ui.small_button("×").on_hover_text("Remove watch").clicked(){action.remove.push(watch.id);}ui.monospace(&watch.expression);});
                if let Some(result)=&watch.result{if result["hasException"]==true{ui.colored_label(crate::theme::palette(ui.ctx()).danger,result["exceptionMessage"].as_str().unwrap_or("Exception"));}else{crate::console_ui::value_ui(ui,"value",&result["value"],objects,0);}}else{ui.weak("Not evaluated in this context yet");}
            });
        }
        ui.separator();ui.strong(format!("Breakpoints ({})",debug.breakpoints.len()));
        if ui.button("Remove all breakpoints").clicked(){action.clear=true;}
        if debug.breakpoints.is_empty(){ui.weak("Click a line number in the source to add a breakpoint.");}
        for point in &debug.breakpoints {
            ui.push_id((point.0,point.1),|ui|{ui.horizontal_wrapped(|ui|{
                let mut enabled=!debug.advanced.disabled.contains(&(point.0,point.1));let toggle=ui.checkbox(&mut enabled,"");toggle.widget_info(||egui::WidgetInfo::selected(egui::Role::CheckBox,true,enabled,format!("Enable breakpoint at line {}",point.1)));if toggle.changed(){action.enabled.push((point.0,point.1,enabled));}
                let source=debug.sources.iter().position(|s|s.spidermonkey_id==point.0);
                let name=source.map(|i|debug.sources[i].url.as_str().rsplit('/').next().unwrap_or("source")).unwrap_or("source");
                if ui.link(format!("{name}:{}",point.1)).clicked()&&let Some(index)=source{debug.selected=index;debug.advanced.goto=Some(point.1);debug.advanced.sidebar=0;}
                if ui.small_button("×").on_hover_text("Remove breakpoint").clicked(){action.breakpoint=Some((point.0,point.1));}
            });});
        }
    });
}
