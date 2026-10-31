use crate::network::{self, BackendEvent, Bridge, Recorder};
use crossbeam_channel::{Receiver, Sender};
use devtools_traits::{DevtoolScriptControlMsg as Command, SourceInfo};
use serde_json::Value;
use servo::{WebView, WebViewId};
use servo_base::{
    generic_channel::{GenericSender, channel},
    id::PipelineId,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tool {
    Inspector,
    #[default]
    Console,
    Debugger,
    Network,
    StyleEditor,
    Performance,
    Memory,
    Storage,
    Accessibility,
    Application,
}
impl Tool {
    pub const ALL: [Self; 10] = [
        Self::Inspector,
        Self::Console,
        Self::Debugger,
        Self::Network,
        Self::StyleEditor,
        Self::Performance,
        Self::Memory,
        Self::Storage,
        Self::Accessibility,
        Self::Application,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Inspector => "Inspector",
            Self::Console => "Console",
            Self::Debugger => "Debugger",
            Self::Network => "Network",
            Self::StyleEditor => "Style Editor",
            Self::Performance => "Performance",
            Self::Memory => "Memory",
            Self::Storage => "Storage",
            Self::Accessibility => "Accessibility",
            Self::Application => "Application",
        }
    }
}
#[derive(Default)]
pub struct Snapshot {
    pub data: Value,
    pub error: Option<String>,
    pub pending: bool,
    pub updated: Option<Instant>,
}
pub struct DebugPage {
    pub pipeline: PipelineId,
    pub sender: GenericSender<Command>,
    pub sources: Vec<SourceInfo>,
    pub selected: usize,
    pub paused: bool,
    pub frame: Option<String>,
    pub paused_frame: Option<String>,
    pub location: String,
    pub paused_line: Option<u32>,
    pub paused_source: Option<u32>,
    pub frames: Vec<(String, Value)>,
    pub breakpoints: Vec<(u32, u32, u32, u32)>,
    pub line: String,
    pub advanced: crate::debug_model::State,
}
pub struct Page {
    pub url: String,
    pub snapshots: HashMap<Tool, Snapshot>,
    pub selector: String,
    pub inspector: crate::inspector_ui::State,
    pub html: String,
    pub css: String,
    pub style_index: i32,
    pub style_loaded: bool,
    pub style_sheets: Value,
    pub performance: crate::performance_ui::State,
    pub storage_area: usize,
    pub storage_key: String,
    pub storage_value: String,
    pub cookies: crate::cookie_inspector::State,
    pub console_input: String,
    pub console_state: crate::console_model::State,
    pub console_results: Vec<String>,
    pub ax: HashMap<String, Value>,
}
impl Page {
    fn new(url: String) -> Self {
        Self {
            url,
            snapshots: HashMap::new(),
            selector: "body".into(),
            inspector: Default::default(),
            html: String::new(),
            css: String::new(),
            style_index: 0,
            style_loaded: false,
            style_sheets: Value::Null,
            performance: Default::default(),
            storage_area: 0,
            storage_key: String::new(),
            storage_value: String::new(),
            cookies: Default::default(),
            console_input: String::new(),
            console_state: Default::default(),
            console_results: Vec::new(),
            ax: HashMap::new(),
        }
    }
}
struct Reply {
    token: u64,
    value: Result<Value, String>,
}
struct Job {
    view: WebView,
    tool: Tool,
    url: String,
    token: u64,
    deadline: Instant,
    next: Instant,
    polling: bool,
}
pub(super) struct NativeJob {
    owner: String,
    url: String,
    tool: Tool,
    deadline: Instant,
    receive: Box<dyn FnMut() -> Option<Value>>,
}
pub struct Toolbox {
    pub bridge: Bridge,
    pub log: crate::console_model::Log,
    pub driver: crate::debug_model::Driver,
    pub environments: HashMap<String, Value>,
    pub objects: HashMap<String, Value>,
    pub network: Recorder,
    pub network_ui: crate::network::UiState,
    pub active: Tool,
    pub split_console: bool,
    pub split_rect: egui::Rect,
    pub content_rect: egui::Rect,
    pub network_rects: [egui::Rect; 2],
    pub style_rects: [egui::Rect; 2],
    pub pages: HashMap<String, Page>,
    pub debug: HashMap<String, DebugPage>,
    pipelines: HashMap<PipelineId, String>,
    jobs: Vec<Job>,
    native_jobs: Vec<NativeJob>,
    replies: Receiver<Reply>,
    sender: Sender<Reply>,
    next: u64,
    pub detail_tab: usize,
    pub export_path: String,
    pub export_open: bool,
    pub notice: Option<String>,
    notice_seen: Option<(String, Instant)>,
    /// A failure stays visible until dismissed; `notice` fades after 5 s.
    pub error: Option<String>,
    pub attention: Option<String>,
    pub panel_rect: egui::Rect,
}
impl Toolbox {
    pub fn new(
        proxy: winit::event_loop::EventLoopProxy<crate::engine::Wake>,
    ) -> Result<Self, String> {
        let (sender, replies) = crossbeam_channel::unbounded();
        let mut network = Recorder::default();
        network.show_initiator = true;
        network.show_transferred = true;
        Ok(Self {
            bridge: Bridge::new(proxy)?,
            log: Default::default(),
            driver: Default::default(),
            environments: HashMap::new(),
            objects: HashMap::new(),
            network,
            network_ui: Default::default(),
            active: Tool::Console,
            split_console: false,
            split_rect: egui::Rect::NOTHING,
            content_rect: egui::Rect::NOTHING,
            network_rects: [egui::Rect::NOTHING; 2],
            style_rects: [egui::Rect::NOTHING; 2],
            pages: HashMap::new(),
            debug: HashMap::new(),
            pipelines: HashMap::new(),
            jobs: Vec::new(),
            native_jobs: Vec::new(),
            replies,
            sender,
            next: 0,
            detail_tab: 0,
            export_path: String::new(),
            export_open: false,
            notice: None,
            notice_seen: None,
            error: None,
            attention: None,
            panel_rect: egui::Rect::NOTHING,
        })
    }
    pub fn transient_notice(&mut self) -> Option<(String, Duration)> {
        let Some(text) = self.notice.clone() else {
            self.notice_seen = None;
            return None;
        };
        if self
            .notice_seen
            .as_ref()
            .is_none_or(|(old, _)| *old != text)
        {
            self.notice_seen = Some((text.clone(), Instant::now()));
        }
        let elapsed = self.notice_seen.as_ref().unwrap().1.elapsed();
        if elapsed >= Duration::from_secs(5) {
            self.notice = None;
            self.notice_seen = None;
            None
        } else {
            Some((text, Duration::from_secs(5) - elapsed))
        }
    }
    pub fn page(&mut self, view: &WebView) -> &mut Page {
        let owner = network::owner(view.id());
        let url = view.url().map(|u| u.to_string()).unwrap_or_default();
        let page = self
            .pages
            .entry(owner)
            .or_insert_with(|| Page::new(url.clone()));
        if page.url != url {
            let console_state = std::mem::take(&mut page.console_state);
            *page = Page::new(url);
            page.console_state = console_state;
        }
        page
    }
    pub fn pending(&self) -> bool {
        !self.jobs.is_empty()
            || self.pages.values().any(|page| page.cookies.pending())
            || !self.native_jobs.is_empty()
            || self.driver.active.is_some()
            || !self.driver.queue.is_empty()
    }
    pub fn run(&mut self, view: &WebView, tool: Tool, body: String) {
        if self.jobs.len() >= 32 {
            self.notice = Some("Wait for the current developer-tool requests to complete.".into());
            return;
        }
        let snapshot = self.page(view).snapshots.entry(tool).or_default();
        snapshot.pending = true;
        snapshot.error = None;
        self.next += 1;
        let token = self.next;
        let script = format!(
            "globalThis.__wegaToolResults ||= Object.create(null);Promise.resolve().then(async()=>{{{body}}}).then(value=>{{globalThis.__wegaToolResults[{token}]={{ok:true,value}}}},error=>{{globalThis.__wegaToolResults[{token}]={{ok:false,error:String(error)}}}});'queued'"
        );
        let sender = self.sender.clone();
        view.evaluate_javascript(script, move |value| {
            if let Err(error) = value {
                let _ = sender.send(Reply {
                    token,
                    value: Err(format!("{error:?}")),
                });
            }
        });
        self.jobs.push(Job {
            view: view.clone(),
            tool,
            url: view.url().map(|u| u.to_string()).unwrap_or_default(),
            token,
            deadline: Instant::now() + Duration::from_secs(15),
            next: Instant::now() + Duration::from_millis(40),
            polling: false,
        });
    }
    pub fn refresh(&mut self, view: &WebView, servo: &servo::Servo) {
        let tool = self.active;
        match tool {
            Tool::Inspector => {
                let selector = self.page(view).selector.clone();
                self.run(
                    view,
                    tool,
                    format!(
                        "const selector={};{}",
                        quote(&selector),
                        crate::probes::INSPECTOR
                    ),
                );
            }
            Tool::Performance => self.run(view, tool, crate::probes::PERFORMANCE.into()),
            Tool::Storage => {
                self.page(view).cookies.refresh(view, servo);
                self.run(view, tool, crate::probes::STORAGE.into());
            }
            Tool::Application => self.run(view, tool, crate::probes::APPLICATION.into()),
            Tool::Accessibility => {
                if view.accesskit_tree_id().is_none() {
                    self.page(view).ax.clear();
                }
                view.set_accessibility_active(true);
                self.run(view, tool, crate::probes::ACCESSIBILITY.into());
            }
            Tool::Memory => {
                let owner = network::owner(view.id());
                let sender = self.sender.clone();
                self.next += 1;
                let token = self.next;
                if let Ok(callback) = servo_base::generic_channel::GenericCallback::new(
                    move |result: Result<servo::profile_traits::mem::MemoryReportResult, _>| {
                        let value = result
                            .map_err(|e| format!("{e:?}"))
                            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()));
                        let _ = sender.send(Reply { token, value });
                    },
                ) {
                    self.page(view).snapshots.entry(tool).or_default().pending = true;
                    self.jobs.push(Job {
                        view: view.clone(),
                        tool,
                        url: view.url().map(|u| u.to_string()).unwrap_or_default(),
                        token,
                        deadline: Instant::now() + Duration::from_secs(15),
                        next: Instant::now() + Duration::from_secs(30),
                        polling: true,
                    });
                    servo.create_memory_report(callback);
                } else {
                    self.notice = Some(format!("Could not request memory report for {owner}"));
                }
            }
            Tool::StyleEditor => {
                self.page(view).style_loaded = false;
                self.native(view, tool, |pipeline, reply| {
                    Command::GetStyleSheets(pipeline, reply)
                });
            }
            _ => {}
        }
    }
    pub fn native<T: serde::Serialize + serde::de::DeserializeOwned + 'static>(
        &mut self,
        view: &WebView,
        tool: Tool,
        make: impl FnOnce(PipelineId, GenericSender<T>) -> Command,
    ) {
        let owner = network::owner(view.id());
        let Some(debug) = self.debug.get(&owner) else {
            self.notice = Some(
                "The page's script context is not ready yet. Try Refresh after it loads.".into(),
            );
            return;
        };
        let Some((sender, receiver)) = channel::<T>() else {
            return;
        };
        if let Err(error) = debug.sender.send(make(debug.pipeline, sender)) {
            self.notice = Some(format!("Engine command failed: {error:?}"));
            return;
        }
        self.page(view).snapshots.entry(tool).or_default().pending = true;
        self.native_jobs.push(NativeJob {
            url: view.url().map(|u| u.to_string()).unwrap_or_default(),
            owner,
            tool,
            deadline: Instant::now() + Duration::from_secs(10),
            receive: Box::new(move || {
                receiver
                    .try_recv()
                    .ok()
                    .map(|v| serde_json::to_value(v).unwrap_or(Value::Null))
            }),
        });
    }
    pub fn stylesheet(&mut self, view: &WebView, index: i32) {
        self.page(view).style_index = index;
        self.page(view).style_loaded = true;
        self.page(view).css.clear();
        self.native(view, Tool::StyleEditor, move |pipeline, reply| {
            Command::GetStyleSheetText(pipeline, index, reply)
        });
    }
    pub fn console(&mut self, view: &WebView) {
        let source = std::mem::take(&mut self.page(view).console_input);
        if source.trim().is_empty() {
            return;
        }
        let owner = network::owner(view.id());
        self.page(view).console_state.remember(&source);
        self.page(view).console_results.push(format!("> {source}"));
        self.log
            .text(&owner, "Log", "Input", format!("> {source}"), Value::Null);
        if self.debug.get(&owner).is_some_and(|d| d.paused)
            || self.page(view).console_state.native_evaluation
        {
            self.queue_debug(
                &owner,
                crate::debug_model::Action::Eval {
                    expression: source,
                    console: true,
                    watch: None,
                },
            );
            return;
        }
        self.run(
            view,
            Tool::Console,
            format!(
                "const source={};{}",
                quote(&source),
                include_str!("console_eval.js")
            ),
        );
    }
    pub fn clear_breakpoints(&mut self, owner: &str) {
        self.native_jobs
            .retain(|j| j.owner != owner || j.tool != Tool::Debugger);
        if let Some(debug) = self.debug.get_mut(owner) {
            for p in debug.breakpoints.drain(..) {
                let _ = debug.sender.send(Command::ClearBreakpoint(p.0, p.2, p.3));
            }
            debug.advanced.disabled.clear();
        }
    }
    pub fn toggle_breakpoint(&mut self, view: &WebView, source: u32, line: u32) {
        let owner = network::owner(view.id());
        if self
            .native_jobs
            .iter()
            .any(|j| j.owner == owner && j.tool == Tool::Debugger)
        {
            self.notice = Some("Wait for the breakpoint location response.".into());
            return;
        }
        let Some(debug) = self.debug.get_mut(&owner) else {
            return;
        };
        if let Some(index) = debug
            .breakpoints
            .iter()
            .position(|p| p.0 == source && p.1 == line)
        {
            let point = debug.breakpoints.remove(index);
            debug.advanced.disabled.remove(&(source, line));
            let _ = debug
                .sender
                .send(Command::ClearBreakpoint(point.0, point.2, point.3));
            return;
        }
        let Some((sender, receiver)) =
            channel::<Vec<devtools_traits::RecommendedBreakpointLocation>>()
        else {
            return;
        };
        if debug
            .sender
            .send(Command::GetPossibleBreakpoints(source, sender))
            .is_err()
        {
            return;
        }
        self.native_jobs.push(NativeJob{owner,url:view.url().map(|u|u.to_string()).unwrap_or_default(),tool:Tool::Debugger,deadline:Instant::now()+Duration::from_secs(10),receive:Box::new(move||receiver.try_recv().ok().map(|locations|{
            if let Some(location)=locations.iter().filter(|p|p.line_number==line).min_by_key(|p|p.column_number){serde_json::json!({"breakpoint":[source,line,location.script_id,location.offset]})}else{serde_json::json!({"breakpointError":format!("No executable breakpoint at line {line}")})}
        }))});
    }
    pub fn command(&mut self, owner: &str, command: Command) {
        if let Some(debug) = self.debug.get_mut(owner)
            && let Err(error) = debug.sender.send(command)
        {
            self.notice = Some(format!("Debugger command failed: {error:?}"));
        }
    }
    pub fn resume(&mut self, owner: &str, kind: Option<&str>) {
        if let Some(debug) = self.debug.get_mut(owner) {
            if let Err(error) = debug.sender.send(Command::Resume(
                kind.map(str::to_owned),
                debug.paused_frame.clone(),
            )) {
                self.notice = Some(format!("Could not resume debugger: {error:?}"));
                return;
            }
            debug.paused_frame = None;
            debug.paused = false;
            debug.advanced.generation += 1;
            debug.advanced.stack.clear();
            debug.advanced.environment = None;
            for watch in &mut debug.advanced.watches {
                watch.result = None;
            }
        }
    }
    pub fn resume_all(&mut self) {
        for debug in self.debug.values_mut().filter(|d| d.paused) {
            let _ = debug.sender.send(Command::Resume(None, None));
            debug.paused = false;
            debug.advanced.generation += 1;
            debug.advanced.stack.clear();
            debug.advanced.environment = None;
            for watch in &mut debug.advanced.watches {
                watch.result = None;
            }
        }
    }
    fn complete(&mut self, owner: String, tool: Tool, value: Result<Value, String>) {
        if tool == Tool::Debugger
            && let Ok(data) = &value
        {
            if let Some(error) = data["breakpointError"].as_str() {
                self.notice = Some(error.into());
            }
            if let Some(point) = data["breakpoint"].as_array()
                && point.len() == 4
                && let Some(debug) = self.debug.get_mut(&owner)
            {
                let p = (
                    point[0].as_u64().unwrap() as u32,
                    point[1].as_u64().unwrap() as u32,
                    point[2].as_u64().unwrap() as u32,
                    point[3].as_u64().unwrap() as u32,
                );
                let result = if debug.advanced.all_disabled
                    || debug.advanced.disabled.contains(&(p.0, p.1))
                {
                    Ok(())
                } else {
                    debug.sender.send(Command::SetBreakpoint(p.0, p.2, p.3))
                };
                match result {
                    Ok(()) => {
                        debug.breakpoints.push(p);
                        self.notice = Some(format!("Breakpoint set at line {}", p.1));
                    }
                    Err(e) => self.notice = Some(format!("Breakpoint failed: {e:?}")),
                }
            }
        }
        let Some(page) = self.pages.get_mut(&owner) else {
            return;
        };
        if tool == Tool::Inspector {
            page.inspector.picking = false;
        }
        let snapshot = page.snapshots.entry(tool).or_default();
        snapshot.pending = false;
        snapshot.updated = Some(Instant::now());
        match value {
            Ok(value) => {
                if tool == Tool::Inspector
                    && let Some(html) = value.pointer("/selected/html").and_then(Value::as_str)
                {
                    page.html = html.into();
                    page.inspector.reveal = true;
                    if let Some(selector) =
                        value.pointer("/selected/selector").and_then(Value::as_str)
                    {
                        if let Some(ancestors) = value
                            .pointer("/selected/ancestors")
                            .and_then(Value::as_array)
                        {
                            for node in ancestors {
                                if let Some(s) = node["selector"].as_str() {
                                    page.inspector.collapsed.remove(s);
                                }
                            }
                        }
                        page.selector = selector.into();
                    }
                    page.inspector.inline_css = value
                        .pointer("/selected/inlineCSS")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into();
                    page.inspector.classes = value
                        .pointer("/selected/classes")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into();
                }
                if tool == Tool::StyleEditor {
                    if value.is_null() {
                        snapshot.error =
                            Some("The engine did not return stylesheet source.".into());
                        return;
                    }
                    if let Some(css) = value.as_str() {
                        page.css = css.into();
                    } else {
                        page.style_sheets = value.clone();
                        snapshot.data = value;
                        return;
                    }
                }
                if tool == Tool::Console {
                    let text = value
                        .get("value")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| pretty(&value));
                    let details =
                        serde_json::from_str::<Value>(&text).unwrap_or_else(|_| value.clone());
                    self.log.text(&owner, "Log", "Result", text, details);
                    page.console_results
                        .push(format!("{}", value.get("value").unwrap_or(&value)));
                    while page.console_results.len() > 200 {
                        page.console_results.remove(0);
                    }
                } else {
                    snapshot.data = value;
                }
                snapshot.error = None;
            }
            Err(error) => {
                if tool == Tool::Console {
                    self.log
                        .text(&owner, "Error", "Result", error.clone(), Value::Null);
                    page.console_results.push(format!("Error: {error}"));
                }
                snapshot.error = Some(error);
            }
        }
    }
    pub fn pump(&mut self) {
        for page in self.pages.values_mut() {
            page.cookies.poll();
        }
        for event in self.bridge.events.try_iter().collect::<Vec<_>>() {
            match event {
                BackendEvent::Network(update) => self.network.apply(update),
                BackendEvent::Navigation(owner) => {
                    self.network.navigation(&owner);
                    self.log.navigation(&owner);
                }
                BackendEvent::Console(pipeline, message) => {
                    if let Some(owner) = self.pipelines.get(&pipeline) {
                        let text = message
                            .arguments
                            .iter()
                            .map(|v| {
                                crate::console_model::value_text(
                                    &crate::console_model::native_value(v),
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        let level = match message.fields.level {
                            servo::ConsoleLogLevel::Trace | servo::ConsoleLogLevel::Dir => {
                                "Log".to_owned()
                            }
                            _ => format!("{:?}", message.fields.level),
                        };
                        let mut details = serde_json::to_value(&message).unwrap_or(Value::Null);
                        details["arguments"] = serde_json::json!(
                            message
                                .arguments
                                .iter()
                                .map(crate::console_model::native_value)
                                .collect::<Vec<_>>()
                        );
                        self.log.push(crate::console_model::Entry {
                            id: 0,
                            owner: owner.clone(),
                            level,
                            category: "JS".into(),
                            text,
                            time: message.fields.time_stamp,
                            source: message.fields.filename.clone(),
                            line: message.fields.line_number,
                            column: message.fields.column_number,
                            details,
                        });
                    }
                }
                BackendEvent::PageError(pipeline, error) => {
                    if let Some(owner) = self.pipelines.get(&pipeline) {
                        self.log.push(crate::console_model::Entry {
                            id: 0,
                            owner: owner.clone(),
                            level: "Error".into(),
                            category: "JS".into(),
                            text: error.error_message,
                            time: error.time_stamp,
                            source: error.source_name,
                            line: error.line_number,
                            column: error.column_number,
                            details: Value::Null,
                        });
                    }
                }
                BackendEvent::CssError(pipeline, error) => {
                    if let Some(owner) = self.pipelines.get(&pipeline) {
                        self.log.push(crate::console_model::Entry {
                            id: 0,
                            owner: owner.clone(),
                            level: "Warn".into(),
                            category: "CSS".into(),
                            text: error.msg,
                            time: devtools_traits::get_time_stamp(),
                            source: error.filename,
                            line: error.line,
                            column: error.column,
                            details: Value::Null,
                        });
                    }
                }
                BackendEvent::ClearConsole(pipeline) => {
                    if let Some(owner) = self.pipelines.get(&pipeline)
                        && !self.log.preserve
                    {
                        self.log.clear(owner);
                    }
                }
                BackendEvent::Object(name, value) => {
                    if self.objects.len() >= 1000 {
                        self.objects.clear();
                    }
                    let value = crate::console_model::native_value(&value);
                    self.objects.insert(
                        name,
                        if value.to_string().len() > 65536 {
                            serde_json::json!({"StringValue":"[Native preview exceeds 64 KiB]"})
                        } else {
                            value
                        },
                    );
                }
                BackendEvent::Environment(name, value, parent) => {
                    if self.environments.len() >= 500 {
                        self.environments.clear();
                    }
                    let mut bindings =
                        serde_json::to_value(&value.binding_variables).unwrap_or(Value::Null);
                    for (index, binding) in value.binding_variables.iter().enumerate() {
                        bindings[index]["value"] =
                            crate::console_model::native_value(&binding.value);
                    }
                    let info = serde_json::json!({"type":value.type_,"scopeKind":value.scope_kind,"functionDisplayName":value.function_display_name,"object":value.object.as_ref().map(crate::console_model::native_value),"bindingVariables":bindings});
                    let info = if info.to_string().len() > 262144 {
                        serde_json::json!({"note":"Native environment preview exceeds 256 KiB"})
                    } else {
                        info
                    };
                    self.environments
                        .insert(name, serde_json::json!({"info":info,"parent":parent}));
                }

                BackendEvent::Global {
                    view,
                    pipeline,
                    sender,
                    top,
                } => {
                    let owner = network::owner(view);
                    self.pipelines.insert(pipeline, owner.clone());
                    if top {
                        let _ = sender.send(Command::WantsLiveNotifications(pipeline, true));
                        let mut advanced = crate::debug_model::State::default();
                        if let Some(previous) = self.debug.remove(&owner) {
                            advanced.watches = previous.advanced.watches;
                            for watch in &mut advanced.watches {
                                watch.result = None;
                            }
                            advanced.watch_next = previous.advanced.watch_next;
                            advanced.source_filter = previous.advanced.source_filter;
                            advanced.sidebar = previous.advanced.sidebar;
                            advanced.all_disabled = previous.advanced.all_disabled;
                        }
                        self.debug.insert(
                            owner,
                            DebugPage {
                                pipeline,
                                sender,
                                sources: Vec::new(),
                                selected: 0,
                                paused: false,
                                frame: None,
                                paused_frame: None,
                                location: String::new(),
                                paused_line: None,
                                paused_source: None,
                                frames: Vec::new(),
                                breakpoints: Vec::new(),
                                line: "1".into(),
                                advanced,
                            },
                        );
                    }
                }
                BackendEvent::Source(pipeline, source) => {
                    if let Some(debug) = self
                        .pipelines
                        .get(&pipeline)
                        .and_then(|owner| self.debug.get_mut(owner))
                        && debug.pipeline == pipeline
                    {
                        if let Some(index) = debug
                            .sources
                            .iter()
                            .position(|s| s.spidermonkey_id == source.spidermonkey_id)
                        {
                            debug.sources[index] = source;
                        } else if debug.sources.len() < 250 {
                            debug.sources.push(source);
                        }
                    }
                }
                BackendEvent::Frame(pipeline, name, frame) => {
                    if let Some(debug) = self
                        .pipelines
                        .get(&pipeline)
                        .and_then(|owner| self.debug.get_mut(owner))
                        && debug.pipeline == pipeline
                    {
                        debug.frames.push((name,serde_json::json!({"displayName":frame.display_name,"url":frame.url,"type":frame.type_,"this":crate::console_model::native_value(&frame.this_value),"onStack":frame.on_stack,"oldest":frame.oldest,"terminated":frame.terminated})));
                        if debug.frames.len() > 100 {
                            debug.frames.remove(0);
                        }
                    }
                }
                BackendEvent::Paused(pipeline, frame, reason) => {
                    if let Some(owner) = self.pipelines.get(&pipeline)
                        && let Some(debug) = self.debug.get_mut(owner)
                        && debug.pipeline == pipeline
                    {
                        debug.paused = true;
                        debug.paused_line = Some(frame.line);
                        debug.paused_source = debug
                            .frames
                            .iter()
                            .find(|(name, _)| *name == frame.actor)
                            .and_then(|(_, f)| f["url"].as_str())
                            .and_then(|url| {
                                debug.sources.iter().position(|s| s.url.as_str() == url)
                            })
                            .map(|index| {
                                debug.selected = index;
                                debug.sources[index].spidermonkey_id
                            });
                        debug.advanced.generation += 1;
                        debug.advanced.stack.clear();
                        debug.advanced.environment = None;
                        for watch in &mut debug.advanced.watches {
                            watch.result = None;
                        }
                        debug.paused_frame = Some(frame.actor.clone());
                        debug.frame = Some(frame.actor);
                        debug.location = format!(
                            "{} · line {}, column {}",
                            reason.type_, frame.line, frame.column
                        );
                        self.attention = Some(owner.clone());
                    }
                    if let Some(owner) = self.pipelines.get(&pipeline).cloned() {
                        self.refresh_debug_context(&owner);
                    }
                }
            }
        }
        self.pump_debug_driver();
        let mut done = Vec::new();
        for reply in self.replies.try_iter().collect::<Vec<_>>() {
            if let Some(index) = self.jobs.iter().position(|j| j.token == reply.token) {
                let job = &mut self.jobs[index];
                job.polling = false;
                let value = match reply.value {
                    Ok(Value::Null) => {
                        job.next = Instant::now() + Duration::from_millis(100);
                        continue;
                    }
                    Ok(value) if value.get("ok").is_some() => {
                        if value["ok"] == true {
                            Ok(value["value"].clone())
                        } else {
                            Err(value["error"]
                                .as_str()
                                .unwrap_or("Page operation failed")
                                .into())
                        }
                    }
                    other => other,
                };
                done.push((index, network::owner(job.view.id()), job.tool, value));
            }
        }
        done.sort_by_key(|item| std::cmp::Reverse(item.0));
        done.dedup_by_key(|item| item.0);
        for (index, owner, tool, value) in done {
            let job = self.jobs.remove(index);
            if job.view.url().map(|u| u.to_string()).unwrap_or_default() == job.url {
                job.view.evaluate_javascript(
                    format!("delete globalThis.__wegaToolResults?.[{}]", job.token),
                    |_| {},
                );
                self.complete(owner, tool, value);
            }
        }
        let mut expired = Vec::new();
        for (index, job) in self.jobs.iter_mut().enumerate() {
            if Instant::now() > job.deadline {
                expired.push(index);
                continue;
            }
            if !job.polling && Instant::now() >= job.next {
                job.polling = true;
                let token = job.token;
                let sender = self.sender.clone();
                job.view.evaluate_javascript(
                    format!("JSON.stringify(globalThis.__wegaToolResults?.[{token}]??null)"),
                    move |result| {
                        let value = match result {
                            Ok(servo::JSValue::String(text)) if text.len() <= 2 * 1024 * 1024 => {
                                serde_json::from_str(&text).map_err(|e| e.to_string())
                            }
                            Ok(_) => Err("Tool response exceeds the preview limit".into()),
                            Err(error) => Err(format!("{error:?}")),
                        };
                        let _ = sender.send(Reply { token, value });
                    },
                );
            }
        }
        for index in expired.into_iter().rev() {
            let job = self.jobs.remove(index);
            self.complete(
                network::owner(job.view.id()),
                job.tool,
                Err("Tool request timed out. If JavaScript is paused, resume it first.".into()),
            );
        }
        let mut native_done = Vec::new();
        for (index, job) in self.native_jobs.iter_mut().enumerate() {
            if let Some(value) = (job.receive)() {
                native_done.push((index, job.owner.clone(), job.tool, Ok(value)));
            } else if Instant::now() > job.deadline {
                native_done.push((
                    index,
                    job.owner.clone(),
                    job.tool,
                    Err("Engine request timed out".into()),
                ));
            }
        }
        for (index, owner, tool, value) in native_done.into_iter().rev() {
            let job = self.native_jobs.remove(index);
            if self.pages.get(&owner).is_some_and(|p| p.url == job.url) {
                self.complete(owner, tool, value);
            }
        }
    }
    pub fn shutdown(&mut self) {
        self.resume_all();
        self.jobs.clear();
        self.native_jobs.clear();
        self.debug.clear();
        self.pipelines.clear();
    }
    pub fn closed(&mut self, view: WebViewId) {
        let owner = network::owner(view);
        self.resume(&owner, None);
        self.pages.remove(&owner);
        self.debug.remove(&owner);
        self.network.clear(&owner);
        self.log.clear(&owner);
        self.driver.queue.retain(|r| r.owner != owner);
        self.jobs.retain(|j| j.view.id() != view);
        self.native_jobs.retain(|j| j.owner != owner);
        self.pipelines.retain(|_, v| v != &owner);
    }
}
pub fn quote(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}
pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}
