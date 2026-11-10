//! Loopback W3C WebDriver transport. The HTTP layer is Mozilla's webdriver crate.
use crate::{
    browser::{Browser, Tab},
    engine::Wake,
};
use crossbeam_channel::{Receiver, Sender};
use serde_json::{Value, json};
use std::{sync::OnceLock, time::Duration};
use webdriver::{
    command::{WebDriverCommand as Command, WebDriverMessage},
    error::{ErrorStatus, WebDriverError, WebDriverResult},
    response::{NewSessionResponse, ValueResponse, WebDriverResponse as Response},
    server::{Session, SessionTeardownKind, WebDriverHandler},
};
pub type Job = Box<dyn FnOnce(&mut Browser) + Send>;
// The listener owns process-wide signal handling and lives until process exit.
static SERVER: OnceLock<webdriver::server::Listener> = OnceLock::new();
#[derive(Clone, Debug, PartialEq)]
struct ResourcesRoute;
#[derive(Clone, Debug)]
pub(crate) struct ResourcesCommand;
impl webdriver::command::WebDriverExtensionCommand for ResourcesCommand {
    fn parameters_json(&self) -> Option<Value> {
        None
    }
}
impl webdriver::httpapi::WebDriverExtensionRoute for ResourcesRoute {
    type Command = ResourcesCommand;
    fn command(
        &self,
        _: &webdriver::Parameters,
        _: &Value,
    ) -> WebDriverResult<Command<ResourcesCommand>> {
        Ok(Command::Extension(ResourcesCommand))
    }
}
struct Handler {
    jobs: Sender<Job>,
    proxy: winit::event_loop::EventLoopProxy<Wake>,
}
pub(crate) fn error(status: ErrorStatus, message: impl Into<String>) -> WebDriverError {
    WebDriverError::new(status, message.into())
}
fn value(value: Value) -> WebDriverResult<Response> {
    Ok(Response::Generic(ValueResponse(value)))
}
impl WebDriverHandler<ResourcesRoute> for Handler {
    fn handle_command(
        &mut self,
        _: &Option<Session>,
        message: WebDriverMessage<ResourcesRoute>,
    ) -> WebDriverResult<Response> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let timeout = Duration::from_secs(35);
        let deadline = wega::deadline::Deadline::new(timeout);
        let queued = deadline.clone();
        self.jobs
            .send_timeout(
                Box::new(move |browser| {
                    if !queued.expired() {
                        browser.automation_command(message.command, tx);
                    }
                }),
                timeout,
            )
            .map_err(|_| error(ErrorStatus::Timeout, "Browser command queue unavailable"))?;
        let _ = self.proxy.send_event(Wake::Ui);
        rx.recv_timeout(deadline.remaining()).map_err(|_| {
            deadline.cancel();
            error(ErrorStatus::Timeout, "Browser command timed out")
        })?
    }
    fn teardown_session(&mut self, _: SessionTeardownKind) {}
}
pub fn start(
    port: u16,
    proxy: winit::event_loop::EventLoopProxy<Wake>,
) -> Result<Receiver<Job>, String> {
    let (jobs, receiver) = crossbeam_channel::bounded(32);
    let listener = webdriver::server::start(
        (std::net::Ipv4Addr::LOCALHOST, port).into(),
        vec![
            url::Host::Ipv4(std::net::Ipv4Addr::LOCALHOST),
            url::Host::Domain("localhost".into()),
        ],
        vec![],
        Handler { jobs, proxy },
        vec![(
            http::Method::GET,
            "/session/{sessionId}/wega/resources",
            ResourcesRoute,
        )],
    )
    .map_err(|e| e.to_string())?;
    println!("WebDriver listening on http://{}", listener.socket);
    SERVER
        .set(listener)
        .map_err(|_| "WebDriver server already running".to_string())?;
    Ok(receiver)
}
fn js(value: servo::JSValue) -> Value {
    use servo::JSValue::*;
    match value {
        Undefined | Null => Value::Null,
        Boolean(v) => json!(v),
        Number(v) => json!(v),
        String(v) => json!(v),
        Array(v) => Value::Array(v.into_iter().map(js).collect()),
        Object(v) => Value::Object(v.into_iter().map(|(k, v)| (k, js(v))).collect()),
        Element(v) => json!({"element-6066-11e4-a52e-4f735466cecf":v}),
        ShadowRoot(v) => json!({"shadow-6066-11e4-a52e-4f735466cecf":v}),
        Frame(v) => json!({"frame-075b-4da1-b6ba-e579c2d3230a":v}),
        Window(v) => json!({"window-fcc6-11e5-b4f8-330a88ab9d7f":v}),
    }
}
impl Browser {
    pub fn automation_command(
        &mut self,
        command: Command<ResourcesCommand>,
        reply: Sender<WebDriverResult<Response>>,
    ) {
        let result = match command {
            Command::Extension(_) => {
                fn resources(tab: &Tab) -> Value {
                    if let Some(page) = &tab.wega {
                        let state = page
                            .state
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        json!({"engine":"wega","pid":page.pid(),"workerRssBytes":page.resident_bytes(),"jsHeapBytes":state.js_heap,"frozen":state.lifecycle.frozen(),"loading":state.lifecycle.loading(),"phase":state.lifecycle.phase(),"compatibility":state.compatibility})
                    } else if tab.engine == wega::engine_choice::EngineKind::Wega {
                        json!({"engine":"wega","pid":null,"workerRssBytes":null,"jsHeapBytes":null,"frozen":false,"loading":false,"phase":"idle","compatibility":true})
                    } else {
                        json!({"engine":"servo","pid":std::process::id(),"workerRssBytes":null,"jsHeapBytes":null,"frozen":null,"loading":tab.loading()})
                    }
                }
                let mut report = resources(self.tab());
                report["tabs"] = self
                    .session()
                    .tabs()
                    .iter()
                    .map(resources)
                    .collect::<Vec<_>>()
                    .into();
                value(report)
            }
            Command::Status => {
                let ready = !self.tab().loading();
                value(
                    json!({"ready":ready,"message":if ready {"Wega Browser is ready"} else {"Page is loading"}}),
                )
            }
            Command::NewSession(params)
                if !params.capabilities.firstMatch.iter().any(|first| {
                    let mut merged = params.capabilities.alwaysMatch.clone();
                    for (key, value) in first {
                        if merged.contains_key(key) {
                            return false;
                        }
                        merged.insert(key.clone(), value.clone());
                    }
                    merged.iter().all(|(key, value)| match key.as_str() {
                        "browserName" => value == "wega",
                        "browserVersion" => value == env!("CARGO_PKG_VERSION") || value == "0.7.0",
                        "platformName" => value == std::env::consts::OS,
                        "pageLoadStrategy" => value == "normal",
                        "acceptInsecureCerts" => value == false,
                        "setWindowRect" => value == false,
                        _ => false,
                    })
                }) =>
            {
                Err(error(
                    ErrorStatus::SessionNotCreated,
                    "Requested capabilities do not match Wega Browser",
                ))
            }
            Command::NewSession(_) => Ok(Response::NewSession(NewSessionResponse::new(
                format!("wega-{}", crate::profile::visit_time()),
                json!({"browserName":"wega","browserVersion":env!("CARGO_PKG_VERSION"),"platformName":std::env::consts::OS,"pageLoadStrategy":"normal","setWindowRect":false,"timeouts":{"script":30000,"pageLoad":30000,"implicit":0}}),
            ))),
            Command::DeleteSession => {
                if let Some(pending) = self.automation_navigation.take() {
                    let _ = pending
                        .reply
                        .send(Err(error(ErrorStatus::NoSuchWindow, "Session was deleted")));
                }
                Ok(Response::DeleteSession)
            }
            Command::GetWindowHandle => value(json!(self.tab().handle())),
            Command::GetWindowHandles => value(json!(
                self.session()
                    .tabs()
                    .iter()
                    .map(Tab::handle)
                    .collect::<Vec<_>>()
            )),
            Command::SwitchToWindow(params) => {
                if let Some(index) = self
                    .session()
                    .tabs()
                    .iter()
                    .position(|t| t.handle() == params.handle)
                {
                    self.select_tab(index);
                    Ok(Response::Void)
                } else {
                    Err(error(ErrorStatus::NoSuchWindow, "Unknown tab"))
                }
            }
            Command::GetCurrentUrl => value(json!(self.tab().url())),
            Command::GetTitle => {
                if self.is_wega() || self.tab().is_internal() {
                    let _ = reply.send(value(json!(self.tab().title())));
                    return;
                }
                // History restoration can switch the active document before the
                // embedder's cached title notification arrives. Query the document.
                if let Some(view) = self.tab().view().cloned() {
                    view.evaluate_javascript("document.title", move |result| {
                        let result = result.map(js).map(value).unwrap_or_else(|error_value| {
                            Err(error(
                                ErrorStatus::JavascriptError,
                                format!("{error_value:?}"),
                            ))
                        });
                        let _ = reply.send(result);
                    });
                    return;
                }
                let _ = reply.send(value(json!(self.tab().title())));
                return;
            }
            Command::Get(params) => {
                // W3C requires an absolute URL; application resolution also keeps
                // internal aliases and supported schemes identical to the UI.
                let url = match url::Url::parse(&params.url)
                    .ok()
                    .filter(|url| {
                        matches!(
                            url.scheme(),
                            "http" | "https" | "about" | "wega" | "data" | "file"
                        )
                    })
                    .and_then(|url| crate::profile::resolve_address(url.as_str()).ok())
                {
                    Some(url) => url,
                    None => {
                        let _ = reply.send(Err(error(
                            ErrorStatus::InvalidArgument,
                            "Invalid navigation URL",
                        )));
                        return;
                    }
                };
                if self.modal() {
                    let _ = reply.send(Err(error(
                        ErrorStatus::UnexpectedAlertOpen,
                        "A browser prompt is open",
                    )));
                    return;
                }
                if let Some(pending) = self.automation_navigation.take() {
                    let _ = pending.reply.send(Err(error(
                        ErrorStatus::UnknownError,
                        "Navigation was superseded by another command",
                    )));
                }
                let is_wega = self.is_wega();
                let is_internal = url.scheme() == "wega";
                let owner = self.tab().id();
                let tab_key = self.tab().key;
                let mut pending = PendingNavigation::new(
                    owner,
                    tab_key,
                    Some(url.to_string()),
                    reply,
                    self.tab().url(),
                );
                if is_wega || is_internal {
                    pending.completion.started();
                }
                self.automation_navigation = Some(pending);
                self.navigate(url.as_str());
                let current_owner = self.tab().id();
                if let Some(pending) = &mut self.automation_navigation
                    && pending.owner.is_none()
                {
                    pending.owner = current_owner;
                }
                self.automation_poll();
                return;
            }
            Command::GoBack | Command::GoForward | Command::Refresh => {
                if self.modal() {
                    let _ = reply.send(Err(error(
                        ErrorStatus::UnexpectedAlertOpen,
                        "A browser prompt is open",
                    )));
                    return;
                }
                if matches!(command, Command::GoBack) && !self.can_go_back()
                    || matches!(command, Command::GoForward) && !self.can_go_forward()
                {
                    let _ = reply.send(Ok(Response::Void));
                    return;
                }
                let expected = match command {
                    Command::GoBack => self.history_target(false),
                    Command::GoForward => self.history_target(true),
                    _ => None,
                };
                if let Some(pending) = self.automation_navigation.take() {
                    let _ = pending.reply.send(Err(error(
                        ErrorStatus::UnknownError,
                        "Navigation was superseded by another command",
                    )));
                }
                let is_wega = self.is_wega();
                let is_internal = self.tab().is_internal();
                let owner = self.tab().id();
                let tab_key = self.tab().key;
                let mut pending =
                    PendingNavigation::new(owner, tab_key, expected, reply, self.tab().url());
                match command {
                    Command::GoBack => {
                        self.traverse(false);
                        if is_wega || is_internal {
                            pending.completion.traversed();
                        }
                    }
                    Command::GoForward => {
                        self.traverse(true);
                        if is_wega || is_internal {
                            pending.completion.traversed();
                        }
                    }
                    _ => {
                        // Unlike load(), reload() marks Started synchronously.
                        // Cached refreshes need not emit another Started callback.
                        self.reload();
                        pending.completion.started();
                    }
                };
                if let Some(view_id) = owner
                    && matches!(command, Command::GoBack | Command::GoForward)
                {
                    pending.traversal = self.features.traversals.get(&view_id).cloned();
                }
                self.automation_navigation = Some(pending);
                self.automation_poll();
                return;
            }
            Command::ExecuteScript(params) => {
                if serde_json::to_string(&params.args)
                    .unwrap_or_default()
                    .contains("element-6066-11e4-a52e-4f735466cecf")
                {
                    let _ = reply.send(Err(error(
                        ErrorStatus::UnsupportedOperation,
                        "Element arguments are not supported by this automation bridge",
                    )));
                    return;
                }
                let args = serde_json::to_string(&params.args).unwrap_or("[]".into());
                let source = format!("(function(){{ {} }}).apply(window,{args})", params.script);
                if self.is_wega() {
                    let eval_parts = self.tab().wega.as_ref().and_then(|p| p.eval_parts());
                    std::thread::spawn(move || {
                        let result = match eval_parts {
                            Some((requests, state)) => {
                                match crate::wega_page::evaluate_sync_parts(
                                    &requests,
                                    &state,
                                    &source,
                                    Duration::from_secs(30),
                                ) {
                                    Ok(json_str) => {
                                        let v =
                                            serde_json::from_str(&json_str).unwrap_or(Value::Null);
                                        value(v)
                                    }
                                    Err(e) => Err(error(ErrorStatus::JavascriptError, e)),
                                }
                            }
                            None => Err(error(ErrorStatus::NoSuchWindow, "No Wega page available")),
                        };
                        let _ = reply.send(result);
                    });
                    return;
                }
                let Some(view) = self.tab().view().cloned() else {
                    let _ = reply.send(Err(error(
                        ErrorStatus::UnsupportedOperation,
                        "Cannot execute script on internal page",
                    )));
                    return;
                };
                view.evaluate_javascript(source, move |result| {
                    let _ = reply.send(match result {
                        Ok(v) => value(js(v)),
                        Err(e) => Err(error(ErrorStatus::JavascriptError, format!("{e:?}"))),
                    });
                });
                return;
            }
            Command::GetPageSource => {
                if self.is_wega() {
                    let eval_parts = self.tab().wega.as_ref().and_then(|p| p.eval_parts());
                    std::thread::spawn(move || {
                        let result = match eval_parts {
                            Some((requests, state)) => match crate::wega_page::evaluate_sync_parts(
                                &requests,
                                &state,
                                "document.documentElement ? document.documentElement.outerHTML : ''",
                                Duration::from_secs(30),
                            ) {
                                Ok(json_str) => {
                                    let html: String =
                                        serde_json::from_str(&json_str).unwrap_or(json_str);
                                    value(json!(html))
                                }
                                Err(e) => Err(error(ErrorStatus::UnknownError, e)),
                            },
                            None => Err(error(ErrorStatus::NoSuchWindow, "No Wega page available")),
                        };
                        let _ = reply.send(result);
                    });
                    return;
                }
                let Some(view) = self.tab().view().cloned() else {
                    let _ = reply.send(value(json!(
                        "<!DOCTYPE html><html><body>Internal</body></html>"
                    )));
                    return;
                };
                let Some((sender, receiver)) = servo_base::generic_channel::channel() else {
                    let _ = reply.send(Err(error(
                        ErrorStatus::UnknownError,
                        "Could not create engine command channel",
                    )));
                    return;
                };
                self.servo.execute_webdriver_command(
                    servo::webdriver::WebDriverCommandMsg::ScriptCommand(
                        view.id().into(),
                        servo::webdriver::WebDriverScriptCommand::GetPageSource(sender),
                    ),
                );
                std::thread::spawn(move || {
                    let result = match receiver.try_recv_timeout(Duration::from_secs(30)) {
                        Ok(Ok(source)) => value(json!(source)),
                        Ok(Err(status)) => Err(error(status, "Could not read page source")),
                        Err(_) => Err(error(ErrorStatus::Timeout, "Page source timed out")),
                    };
                    let _ = reply.send(result);
                });
                return;
            }
            Command::TakeScreenshot => {
                if self.is_wega() {
                    let state = self.tab().wega.as_ref().map(|p| p.state.clone());
                    std::thread::spawn(move || {
                        let result = (|| {
                            let state = state.ok_or_else(|| {
                                error(ErrorStatus::NoSuchWindow, "No Wega page available")
                            })?;
                            let deadline = std::time::Instant::now() + Duration::from_secs(10);
                            let (w, h, bytes) = loop {
                                {
                                    let s = state
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                                    if let Some(frame) = s.frame.clone() {
                                        break frame;
                                    }
                                }
                                if std::time::Instant::now() >= deadline {
                                    return Err(error(
                                        ErrorStatus::UnknownError,
                                        "No frame rendered yet",
                                    ));
                                }
                                std::thread::sleep(Duration::from_millis(20));
                            };
                            use base64::Engine;
                            let image =
                                image::RgbaImage::from_raw(w, h, bytes).ok_or_else(|| {
                                    error(ErrorStatus::UnknownError, "Invalid frame buffer")
                                })?;
                            let mut cursor = std::io::Cursor::new(Vec::new());
                            image::DynamicImage::ImageRgba8(image)
                                .write_to(&mut cursor, image::ImageFormat::Png)
                                .map_err(|e| error(ErrorStatus::UnknownError, e.to_string()))?;
                            value(json!(
                                base64::engine::general_purpose::STANDARD
                                    .encode(cursor.into_inner())
                            ))
                        })();
                        let _ = reply.send(result);
                    });
                    return;
                }
                let Some(view) = self.tab().view().cloned() else {
                    let _ = reply.send(Err(error(
                        ErrorStatus::UnsupportedOperation,
                        "Cannot take screenshot of internal page",
                    )));
                    return;
                };
                view.take_screenshot(None, move |result| {
                    let result = result
                        .map_err(|e| error(ErrorStatus::UnknownError, format!("{e:?}")))
                        .and_then(|image| {
                            use base64::Engine;
                            let mut cursor = std::io::Cursor::new(Vec::new());
                            image::DynamicImage::ImageRgba8(image)
                                .write_to(&mut cursor, image::ImageFormat::Png)
                                .map_err(|e| error(ErrorStatus::UnknownError, e.to_string()))?;
                            value(json!(
                                base64::engine::general_purpose::STANDARD
                                    .encode(cursor.into_inner())
                            ))
                        });
                    let _ = reply.send(result);
                });
                return;
            }
            _ => Err(error(
                ErrorStatus::UnsupportedOperation,
                "This WebDriver command is not implemented",
            )),
        };
        let _ = reply.send(result);
    }
}

pub struct PendingNavigation {
    pub owner: Option<servo::WebViewId>,
    pub tab_key: u64,
    completion: wega::navigation::Completion,
    traversal: Option<servo::TraversalId>,
    pub(crate) deadline: std::time::Instant,
    pub(crate) reply: Sender<WebDriverResult<Response>>,
}
impl PendingNavigation {
    fn new(
        owner: Option<servo::WebViewId>,
        tab_key: u64,
        expected: Option<String>,
        reply: Sender<WebDriverResult<Response>>,
        initial_url: String,
    ) -> Self {
        Self {
            owner,
            tab_key,
            completion: wega::navigation::Completion::new(expected, initial_url),
            traversal: None,
            deadline: std::time::Instant::now() + Duration::from_secs(30),
            reply,
        }
    }
}
impl Browser {
    pub fn automation_load_status(&mut self, view: servo::WebViewId, status: servo::LoadStatus) {
        if let Some(pending) = &mut self.automation_navigation
            && pending.owner == Some(view)
            && status == servo::LoadStatus::Started
        {
            pending.completion.started();
        }
    }
    pub fn automation_traversal(&mut self, view: servo::WebViewId, id: &servo::TraversalId) {
        if let Some(pending) = &mut self.automation_navigation
            && pending.owner == Some(view)
            && pending.traversal.as_ref() == Some(id)
        {
            pending.completion.traversed();
        }
    }
    pub fn automation_finish_navigation(&mut self, view: servo::WebViewId) {
        if self
            .automation_navigation
            .as_ref()
            .is_some_and(|p| p.owner == Some(view))
        {
            let pending = self.automation_navigation.take().unwrap();
            let _ = pending.reply.send(Ok(Response::Void));
        }
    }
    pub fn automation_finish_navigation_by_key(&mut self, tab_key: u64) {
        if self
            .automation_navigation
            .as_ref()
            .is_some_and(|p| p.tab_key == tab_key)
        {
            let pending = self.automation_navigation.take().unwrap();
            let _ = pending.reply.send(Ok(Response::Void));
        }
    }
    pub fn automation_poll(&mut self) {
        let ready_view = self.automation_navigation.as_ref().and_then(|pending| {
            if let Some(owner) = pending.owner {
                self.session()
                    .tabs()
                    .iter()
                    .find(|tab| tab.id() == Some(owner))
                    .filter(|tab| pending.completion.ready(&tab.url(), !tab.loading()))
                    .and_then(Tab::id)
            } else {
                None
            }
        });
        if let Some(view) = ready_view {
            self.automation_finish_navigation(view);
            return;
        }

        let ready_key = self.automation_navigation.as_ref().and_then(|pending| {
            if pending.owner.is_none() {
                self.session()
                    .tabs()
                    .iter()
                    .find(|tab| tab.key == pending.tab_key)
                    .filter(|tab| pending.completion.ready(&tab.url(), !tab.loading()))
                    .map(|tab| tab.key)
            } else {
                None
            }
        });
        if let Some(key) = ready_key {
            self.automation_finish_navigation_by_key(key);
            return;
        }

        if self
            .automation_navigation
            .as_ref()
            .is_some_and(|p| p.deadline <= std::time::Instant::now())
        {
            let pending = self.automation_navigation.take().unwrap();
            let _ = pending
                .reply
                .send(Err(error(ErrorStatus::Timeout, "Navigation timed out")));
        }
    }
}
