//! Serialize native debugger commands: Servo permits one outstanding request of each kind.
use crate::{
    debug_model::{Action, Call, Request},
    toolbox::Toolbox,
};
use devtools_traits::{DevtoolScriptControlMsg as Command, GetEnvironmentRequest};
use serde_json::{Value, json};
use servo_base::generic_channel::{GenericReceiver, TryReceiveError, channel};
use std::time::{Duration, Instant};
fn poll<T: serde::Serialize + serde::de::DeserializeOwned>(
    receiver: &GenericReceiver<T>,
) -> Option<Result<T, &'static str>> {
    match receiver.try_recv() {
        Ok(value) => Some(Ok(value)),
        Err(TryReceiveError::Empty) => None,
        Err(_) => Some(Err("Native debugger reply channel closed")),
    }
}
impl Toolbox {
    pub fn scope_identifiers(&self, owner: &str, prefix: &str) -> Vec<String> {
        let Some(debug) = self.debug.get(owner) else {
            return vec![];
        };
        if !debug.paused {
            return vec![];
        }
        let mut names = std::collections::BTreeSet::new();
        let mut current = debug.advanced.environment.clone();
        let mut seen = std::collections::HashSet::new();
        while let Some(actor) = current {
            if seen.len() >= 20 || !seen.insert(actor.clone()) {
                break;
            }
            let Some(env) = self.environments.get(&actor) else {
                break;
            };
            if let Some(variables) = env["info"]["bindingVariables"].as_array() {
                for variable in variables {
                    if let Some(name) = variable["name"].as_str()
                        && name.starts_with(prefix)
                    {
                        names.insert(name.to_owned());
                    }
                }
            }
            if let Some(properties) =
                env["info"]["object"]["ObjectValue"]["preview"]["ownProperties"].as_array()
            {
                for property in properties {
                    if let Some(name) = property["name"].as_str()
                        && name.starts_with(prefix)
                    {
                        names.insert(name.to_owned());
                    }
                }
            }
            current = env["parent"].as_str().map(str::to_owned);
        }
        names.into_iter().take(100).collect()
    }
    pub fn queue_debug(&mut self, owner: &str, action: Action) {
        if self.driver.queue.len() >= 64 {
            self.notice =
                Some("Debugger request queue is full; wait for the current evaluation.".into());
            return;
        }
        if let Some(debug) = self.debug.get(owner) {
            let same = |request: &Request| {
                request.owner == owner
                    && request.pipeline == debug.pipeline
                    && request.generation == debug.advanced.generation
                    && request.action == action
            };
            if !matches!(action, Action::Eval { console: true, .. })
                && (self.driver.queue.iter().any(&same)
                    || self
                        .driver
                        .active
                        .as_ref()
                        .is_some_and(|call| same(&call.request)))
            {
                return;
            }
            self.driver.queue.push_back(Request {
                owner: owner.into(),
                pipeline: debug.pipeline,
                generation: debug.advanced.generation,
                action,
            });
        }
    }
    pub fn refresh_debug_context(&mut self, owner: &str) {
        let Some(debug) = self.debug.get(owner) else {
            return;
        };
        let frame = debug.frame.clone();
        let watches: Vec<_> = debug
            .advanced
            .watches
            .iter()
            .map(|w| (w.id, w.expression.clone()))
            .collect();
        self.queue_debug(owner, Action::Frames);
        if let Some(frame) = frame {
            self.queue_debug(owner, Action::Scope(frame));
        }
        for (id, expression) in watches {
            self.queue_debug(
                owner,
                Action::Eval {
                    expression,
                    console: false,
                    watch: Some(id),
                },
            );
        }
    }
    pub fn select_debug_frame(&mut self, owner: &str, frame: String) {
        if let Some(debug) = self.debug.get_mut(owner)
            && debug.paused
        {
            debug.frame = Some(frame.clone());
            if let Some(url) = debug
                .frames
                .iter()
                .find(|(name, _)| name == &frame)
                .and_then(|(_, data)| data["url"].as_str())
                && let Some(index) = debug
                    .sources
                    .iter()
                    .position(|source| source.url.as_str() == url)
            {
                debug.selected = index;
            }
            debug.advanced.goto = if debug.paused_frame.as_ref() == Some(&frame) {
                debug.paused_line
            } else {
                None
            };
            debug.advanced.generation += 1;
            debug.advanced.environment = None;
            for watch in &mut debug.advanced.watches {
                watch.result = None;
            }
            self.queue_debug(owner, Action::Scope(frame));
            let watches: Vec<_> = self.debug[owner]
                .advanced
                .watches
                .iter()
                .map(|w| (w.id, w.expression.clone()))
                .collect();
            for (id, expression) in watches {
                self.queue_debug(
                    owner,
                    Action::Eval {
                        expression,
                        console: false,
                        watch: Some(id),
                    },
                );
            }
        }
    }
    pub fn set_breakpoints_enabled(&mut self, owner: &str, enabled: bool) {
        if let Some(debug) = self.debug.get_mut(owner) {
            for p in &debug.breakpoints {
                if debug.advanced.disabled.contains(&(p.0, p.1)) {
                    continue;
                }
                let command = if enabled {
                    Command::SetBreakpoint(p.0, p.2, p.3)
                } else {
                    Command::ClearBreakpoint(p.0, p.2, p.3)
                };
                if let Err(e) = debug.sender.send(command) {
                    self.notice = Some(format!("Breakpoint update failed: {e:?}"));
                    return;
                }
            }
            debug.advanced.all_disabled = !enabled;
        }
    }
    pub fn set_breakpoint_enabled(&mut self, owner: &str, source: u32, line: u32, enabled: bool) {
        if let Some(debug) = self.debug.get_mut(owner)
            && let Some(p) = debug
                .breakpoints
                .iter()
                .find(|p| p.0 == source && p.1 == line)
        {
            if !debug.advanced.all_disabled {
                let command = if enabled {
                    Command::SetBreakpoint(p.0, p.2, p.3)
                } else {
                    Command::ClearBreakpoint(p.0, p.2, p.3)
                };
                if let Err(e) = debug.sender.send(command) {
                    self.notice = Some(format!("Breakpoint update failed: {e:?}"));
                    return;
                }
            }
            if enabled {
                debug.advanced.disabled.remove(&(source, line));
            } else {
                debug.advanced.disabled.insert((source, line));
            }
        }
    }
    pub fn ignore_source(&mut self, owner: &str, source: u32, ignored: bool) {
        if let Some(debug) = self.debug.get_mut(owner) {
            let command = if ignored {
                Command::Blackbox(source, devtools_traits::BlackboxCoverage::Full)
            } else {
                Command::Unblackbox(source, devtools_traits::BlackboxCoverage::Full)
            };
            match debug.sender.send(command) {
                Ok(()) => {
                    if ignored {
                        debug.advanced.ignored.insert(source);
                    } else {
                        debug.advanced.ignored.remove(&source);
                    }
                }
                Err(e) => self.notice = Some(format!("Source ignore failed: {e:?}")),
            }
        }
    }
    pub fn pump_debug_driver(&mut self) {
        let mut finished = None;
        if let Some(call) = &mut self.driver.active {
            if let Some(value) = (call.receive)() {
                finished = Some(value);
            } else if !call.warned && call.started.elapsed() > Duration::from_secs(10) {
                call.warned = true;
                self.notice=Some("Debugger request is taking longer than 10 seconds. The engine still owns this request; additional native requests wait for its reply.".into());
            }
        }
        if let Some(value) = finished {
            let call = self.driver.active.take().unwrap();
            let request = call.request;
            if let Some(error) = value["driverError"].as_str() {
                self.notice = Some(error.into());
            }
            if self.debug.get(&request.owner).is_some_and(|d| {
                d.pipeline == request.pipeline && d.advanced.generation == request.generation
            }) {
                let debug = self.debug.get_mut(&request.owner).unwrap();
                match request.action {
                    Action::Frames => {
                        debug.advanced.stack = value
                            .as_array()
                            .map(|v| {
                                v.iter()
                                    .filter_map(|v| v.as_str().map(str::to_owned))
                                    .collect()
                            })
                            .unwrap_or_default()
                    }
                    Action::Scope(_) => {
                        debug.advanced.environment = value.as_str().map(str::to_owned)
                    }
                    Action::Eval { console, watch, .. } => {
                        if let Some(watch) = watch
                            && let Some(item) =
                                debug.advanced.watches.iter_mut().find(|w| w.id == watch)
                        {
                            item.result = Some(value.clone());
                        }
                        if console {
                            let error = value["hasException"] == true;
                            let text = if error {
                                value["exceptionMessage"]
                                    .as_str()
                                    .unwrap_or("Evaluation threw an exception")
                                    .to_owned()
                            } else {
                                crate::console_model::value_text(&value["value"])
                            };
                            self.log.text(
                                &request.owner,
                                if error { "Error" } else { "Log" },
                                "Result",
                                text.clone(),
                                value.clone(),
                            );
                            if let Some(page) = self.pages.get_mut(&request.owner) {
                                page.console_results.push(text);
                                while page.console_results.len() > 200 {
                                    page.console_results.remove(0);
                                }
                            }
                        }
                    }
                }
            } else if matches!(request.action, Action::Eval { console: true, .. }) {
                self.log.text(
                    &request.owner,
                    "Warn",
                    "Result",
                    "Evaluation result discarded because the debugger context changed.".into(),
                    Value::Null,
                );
            }
        }
        if self.driver.active.is_some() {
            return;
        }
        while let Some(request) = self.driver.queue.pop_front() {
            let Some(debug) = self.debug.get(&request.owner) else {
                continue;
            };
            if debug.pipeline != request.pipeline || debug.advanced.generation != request.generation
            {
                continue;
            }
            let receive: Box<dyn FnMut() -> Option<Value>> = match &request.action {
                Action::Frames => {
                    if !debug.paused {
                        continue;
                    }
                    let Some((sender, receiver)) = channel::<Vec<String>>() else {
                        continue;
                    };
                    if debug
                        .sender
                        .send(Command::ListFrames(debug.pipeline, 0, 50, sender))
                        .is_err()
                    {
                        continue;
                    }
                    Box::new(move || {
                        poll(&receiver).map(|r| match r {
                            Ok(value) => json!(value),
                            Err(error) => json!({"driverError":error}),
                        })
                    })
                }
                Action::Scope(frame) => {
                    if !debug.paused {
                        continue;
                    }
                    let Some((sender, receiver)) = channel::<String>() else {
                        continue;
                    };
                    if debug
                        .sender
                        .send(Command::GetEnvironment(
                            GetEnvironmentRequest::Frame(frame.clone()),
                            sender,
                        ))
                        .is_err()
                    {
                        continue;
                    }
                    Box::new(move || {
                        poll(&receiver).map(|r| match r {
                            Ok(value) => json!(value),
                            Err(error) => json!({"driverError":error}),
                        })
                    })
                }
                Action::Eval { expression, .. } => {
                    let Some((sender, receiver)) = channel::<devtools_traits::EvaluateJSReply>()
                    else {
                        continue;
                    };
                    let frame = if debug.paused {
                        debug.frame.clone()
                    } else {
                        None
                    };
                    if debug
                        .sender
                        .send(Command::Eval(
                            expression.clone(),
                            debug.pipeline,
                            frame,
                            false,
                            sender,
                        ))
                        .is_err()
                    {
                        continue;
                    }
                    Box::new(move || {
                        poll(&receiver).map(|r|match r{Ok(v)=>json!({"value":crate::console_model::native_value(&v.value),"hasException":v.has_exception,"exceptionMessage":v.exception_message}),Err(error)=>json!({"hasException":true,"exceptionMessage":error,"driverError":error})})
                    })
                }
            };
            self.driver.active = Some(Call {
                request,
                receive,
                started: Instant::now(),
                warned: false,
            });
            break;
        }
    }
}

impl crate::browser::Browser {
    pub fn debugger_shortcut(&mut self, key: &winit::keyboard::Key, shift: bool) -> bool {
        use winit::keyboard::{Key, NamedKey};
        if !self.chrome.show_console {
            return false;
        }
        let Some(view) = self.tab().view() else {
            return false;
        };
        let owner = crate::network::owner(view.id());
        let Some(debug) = self.tools.debug.get(&owner) else {
            return false;
        };
        if self.tools.active != crate::toolbox::Tool::Debugger && !debug.paused {
            return false;
        }
        match key {
            Key::Named(NamedKey::F8) => {
                if debug.paused {
                    self.tools.resume(&owner, None);
                } else {
                    self.tools.command(&owner, Command::Interrupt);
                }
            }
            Key::Named(NamedKey::F10) if debug.paused => self.tools.resume(&owner, Some("next")),
            Key::Named(NamedKey::F11) if debug.paused => self
                .tools
                .resume(&owner, Some(if shift { "finish" } else { "step" })),
            _ => return false,
        }
        self.window.request_redraw();
        true
    }
}

#[cfg(test)]
#[path = "debug_driver_tests.rs"]
mod tests;
