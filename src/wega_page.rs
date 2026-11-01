//! A real page in the independently built Blitz/V8 worker. IPC never blocks the UI.
use crate::engine::Wake;
use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex, mpsc},
};
use wega::{
    engine_protocol::{
        self as protocol, Command as Request, ContentEvent, DialogId, DocumentContext, ErrorCode,
        Event, ProtocolError, RequestContext,
        authority::{FrameAuthority, Principal, TrustedRequest},
        identity::{FrameId, ProcessSessionId, SessionIdentity, TabId},
        lifecycle::Lifecycle,
        requests::{DocumentRequests, PendingRequest, RequestKind},
    },
    model::Policy,
    navigation::NavigationPort,
};
use winit::event_loop::EventLoopProxy;

pub(crate) fn executable() -> Option<PathBuf> {
    let path = std::env::var_os("WEGA_ENGINE_BINARY")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe().ok()?.parent().map(|p| {
                if cfg!(target_os = "macos")
                    && let Some(contents) = p.parent()
                {
                    let helper =
                        contents.join("Helpers/WegaContent.app/Contents/MacOS/wega-engine");
                    if helper.is_file() {
                        return helper;
                    }
                }
                p.join(if cfg!(windows) {
                    "wega-engine.exe"
                } else {
                    "wega-engine"
                })
            })
        })?;
    path.is_file().then_some(path)
}
/// A blocking `alert`, `confirm` or `prompt` the page is waiting on.
#[derive(Clone, Debug)]
pub(crate) struct Dialog {
    pub requester: TrustedRequest,
    pub context: DocumentContext,
    pub id: DialogId,
    pub kind: String,
    pub message: String,
    pub default: String,
}
#[derive(Default)]
pub(crate) struct Snapshot {
    /// Resource and containment failures require an explicit restart, not automatic fallback.
    pub recovery_reason: Option<String>,
    pub authority: Option<FrameAuthority>,
    pub lifecycle: Lifecycle,
    pub native: std::collections::VecDeque<(TrustedRequest, ContentEvent)>,
    last_dialog: u64,
    pub page_checks: Vec<(RequestContext, u64, bool)>,
    pub requests: DocumentRequests,
    pub capabilities: Vec<protocol::Capability>,
    pub evaluations: std::collections::VecDeque<(RequestContext, Result<String, ProtocolError>)>,
    pub harness_results: std::collections::VecDeque<(RequestContext, Result<Event, ProtocolError>)>,
    pub rss_bytes: Option<u64>,
    pub interacted: bool,
    pub compatibility: Option<String>,
    pub fallback_reported: bool,
    pub url: String,
    pub title: String,
    pub nodes: usize,
    pub js_heap: usize,
    pub notices: std::collections::VecDeque<String>,
    pub frame: Option<(u32, u32, Vec<u8>)>,
    pub capture: Option<PathBuf>,
    pub capture_result: Option<Result<(), String>>,
    pub visited: Option<(String, String)>,
    pub evaluated: Option<Result<String, String>>,
    /// The page's own history (pushState, fragments) can go back / forward.
    pub document_back: bool,
    pub document_forward: bool,
    pub cursor: Option<String>,
    pub dialog: Option<Dialog>,
}

impl Snapshot {
    pub fn accepts_requester(&self, requester: &TrustedRequest) -> bool {
        self.accepts_context(requester.document)
            && self
                .authority
                .as_ref()
                .and_then(|authority| authority.resolve(requester.document))
                .as_ref()
                == Some(requester)
    }

    fn queue_native(&mut self, context: DocumentContext, event: ContentEvent) {
        if self.native.len() < 32
            && let Some(requester) = self
                .authority
                .as_ref()
                .and_then(|authority| authority.resolve(context))
        {
            self.native.push_back((requester, event));
        }
    }
    pub fn accepts_context(&self, context: DocumentContext) -> bool {
        self.lifecycle.accepts_content()
            && context.document == self.requests.document()
            && context.revision == self.requests.revision()
    }

    #[cfg(test)]
    fn bind_document(
        &mut self,
        serial: u64,
        revision: protocol::DocumentRevision,
    ) -> Result<(), String> {
        self.bind_principal(
            serial,
            revision,
            Principal {
                origin: protocol::authority::Origin::Opaque,
                top_level_site: None,
            },
        )
    }

    fn bind_principal(
        &mut self,
        serial: u64,
        revision: protocol::DocumentRevision,
        principal: Principal,
    ) -> Result<(), String> {
        if serial != self.requests.document().0 || !self.lifecycle.accepts_requests() {
            return Ok(());
        }
        if self.lifecycle.accepts_content() && revision == self.requests.revision() {
            return Ok(());
        }
        let cancelled = self
            .requests
            .commit(revision)
            .map_err(|error| error.message)?;
        self.lifecycle.commit();
        if let Some(authority) = &mut self.authority {
            authority.bind(
                DocumentContext {
                    document: self.requests.document(),
                    revision,
                },
                principal,
            );
        }
        self.native.clear();
        self.evaluated = None;
        self.frame = None;
        self.dialog = None;
        self.cursor = None;
        self.compatibility = None;
        self.fallback_reported = false;
        for request in cancelled {
            self.request_failed(
                request,
                ProtocolError::new(ErrorCode::StaleDocument, "Document was replaced"),
            );
        }
        Ok(())
    }

    /// Returns a denial to send when a stale/replayed prompt must be unblocked.
    fn content_event(
        &mut self,
        context: DocumentContext,
        event: ContentEvent,
    ) -> Option<(DocumentContext, DialogId)> {
        if !self.accepts_context(context) {
            return if let ContentEvent::Dialog { id, .. } = event {
                Some((context, id))
            } else {
                None
            };
        }
        match event {
            ContentEvent::Dialog {
                id,
                kind,
                message,
                default,
            } => {
                let Some(requester) = self
                    .authority
                    .as_ref()
                    .and_then(|authority| authority.resolve(context))
                else {
                    return Some((context, id));
                };
                if id.0 <= self.last_dialog {
                    if self
                        .dialog
                        .as_ref()
                        .is_some_and(|dialog| dialog.context == context && dialog.id == id)
                    {
                        return None;
                    }
                    return Some((context, id));
                }
                self.last_dialog = id.0;
                self.dialog = Some(Dialog {
                    requester,
                    context,
                    id,
                    kind,
                    message,
                    default,
                });
            }
            ContentEvent::DialogClosed(id) => {
                if self
                    .dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.context == context && dialog.id == id)
                {
                    self.dialog = None;
                }
            }
            // Unactivated or replayed popup requests never reach the native UI.
            ContentEvent::OpenWindow(_) => {}
            ContentEvent::ActivatedOpenWindow { url, activation } => {
                if self.native.len() < 32
                    && self.authority.as_ref().is_some_and(|authority| {
                        authority.has_activation(context, activation, std::time::Instant::now())
                    })
                {
                    self.queue_native(
                        context,
                        ContentEvent::ActivatedOpenWindow { url, activation },
                    );
                }
            }
            event => {
                self.queue_native(context, event);
            }
        }
        None
    }

    pub fn take_native_events(&mut self, active: bool) -> Vec<(TrustedRequest, ContentEvent)> {
        let events = std::mem::take(&mut self.native);
        let mut ready = Vec::new();
        for (requester, event) in events {
            let context = requester.document;
            if !self.accepts_requester(&requester) {
                continue;
            }
            if active || matches!(&event, ContentEvent::Download { .. }) {
                if let ContentEvent::ActivatedOpenWindow { activation, .. } = &event
                    && !self.authority.as_mut().is_some_and(|authority| {
                        authority.consume_activation(
                            context,
                            *activation,
                            std::time::Instant::now(),
                        )
                    })
                {
                    continue;
                }
                ready.push((requester, event));
            } else {
                self.native.push_back((requester, event));
            }
        }
        ready
    }
    pub fn take_page_checks(&mut self) -> Vec<(u64, bool)> {
        let document = self.requests.document();
        let revision = self.requests.revision();
        self.page_checks
            .drain(..)
            .map(|(context, request, dirty)| {
                (
                    request,
                    dirty || context.document != document || context.revision != revision,
                )
            })
            .collect()
    }

    fn evaluation_result(
        &mut self,
        context: RequestContext,
        result: Result<String, ProtocolError>,
    ) {
        self.evaluated = Some(result.clone().map_err(|error| error.message));
        if self.evaluations.len() == protocol::requests::MAX_PENDING {
            self.evaluations.pop_front();
        }
        self.evaluations.push_back((context, result));
    }

    fn request_failed(&mut self, request: PendingRequest, error: ProtocolError) {
        match request.kind {
            RequestKind::Evaluate => self.evaluation_result(request.context, Err(error)),
            RequestKind::PageState(browser_request) => {
                if self.page_checks.len() < protocol::requests::MAX_PENDING {
                    self.page_checks
                        .push((request.context, browser_request, true));
                }
            }
            RequestKind::Capture | RequestKind::Navigate => {
                self.harness_result(request.context, Err(error))
            }
        }
    }

    fn harness_result(&mut self, context: RequestContext, result: Result<Event, ProtocolError>) {
        if self.harness_results.len() == protocol::requests::MAX_PENDING {
            self.harness_results.pop_front();
        }
        self.harness_results.push_back((context, result));
    }
    fn complete_harness(
        &mut self,
        context: RequestContext,
        kind: RequestKind,
        event: Event,
    ) -> Result<(), String> {
        let Some(request) = self.requests.take(context) else {
            return Ok(());
        };
        if request.kind != kind {
            self.request_failed(
                request,
                ProtocolError::new(ErrorCode::InvalidRequest, "Mismatched harness response"),
            );
            return Err("Mismatched harness response".into());
        }
        let result = if request.deadline <= std::time::Instant::now() {
            Err(ProtocolError::new(
                ErrorCode::Timeout,
                "Harness operation timed out",
            ))
        } else {
            Ok(event)
        };
        self.harness_result(context, result);
        Ok(())
    }

    fn cancel_requests(&mut self, error: ProtocolError) {
        for request in self.requests.cancel_all() {
            self.request_failed(request, error.clone());
        }
    }

    pub fn expire_requests(&mut self) -> Vec<RequestContext> {
        let expired = self.requests.expired(std::time::Instant::now());
        let contexts = expired.iter().map(|request| request.context).collect();
        for request in expired {
            self.request_failed(
                request,
                ProtocolError::new(ErrorCode::Timeout, "Document request timed out"),
            );
        }
        contexts
    }
}
pub(crate) struct Page {
    child: wega_platform::process::OwnedProcess,
    _frames: Arc<wega_platform::FrameReader>,
    broker: Arc<wega_network::SessionHandle>,
    _budget: wega_platform::supervision::Ticket,
    requests: Option<mpsc::SyncSender<Request>>,
    pub state: Arc<Mutex<Snapshot>>,
    pub texture: Option<egui::TextureHandle>,
    pub viewport: Option<(u32, u32, f32, f32, bool)>,
    pub zoom: f32,
    /// The text of an open `prompt`.
    pub dialog_input: String,
    pub dialog_input_for: Option<DialogId>,
    history: Vec<String>,
    index: usize,
}
fn failed(state: &Arc<Mutex<Snapshot>>, message: String, proxy: &EventLoopProxy<Wake>) {
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.lifecycle.accepts_requests() {
        return;
    }
    if let Some(authority) = &mut state.authority {
        authority.invalidate();
    }
    wega::diagnostics::report(
        "worker:failure",
        wega::diagnostic_event::Diagnostic::error(
            wega::diagnostic_event::Category::Browser,
            message.clone(),
            None,
        ),
    );
    state.compatibility.get_or_insert_with(|| message.clone());
    state.recovery_reason.get_or_insert_with(|| message.clone());
    state.lifecycle.crash();
    state.native.clear();
    state.dialog = None;
    state.frame = None;
    if state.capture.take().is_some() {
        state.capture_result = Some(Err(message.clone()));
    }
    state.cancel_requests(ProtocolError::new(
        ErrorCode::Cancelled,
        "Content worker stopped",
    ));
    if state.notices.len() < 16 {
        state.notices.push_back(message);
    }
    let _ = proxy.send_event(Wake::Ui);
}
fn accept_frame(
    shared: &Mutex<Snapshot>,
    serial: u64,
    revision: protocol::DocumentRevision,
    width: u32,
    height: u32,
    bytes: Vec<u8>,
) {
    let capture = {
        let mut state = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if serial != state.requests.document().0
            || revision != state.requests.revision()
            || !state.lifecycle.presents_frames()
        {
            return;
        }
        state.capture.take()
    };
    // PNG encoding must not hold the UI's state lock.
    let result = capture.map(|path| {
        image::save_buffer(path, &bytes, width, height, image::ColorType::Rgba8)
            .map_err(|e| e.to_string())
    });
    let mut state = shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if serial == state.requests.document().0
        && revision == state.requests.revision()
        && state.lifecycle.presents_frames()
    {
        if let Some(result) = result {
            state.capture_result = Some(result);
        }
        state.frame = Some((width, height, bytes));
    }
}
impl Page {
    pub fn download_cookies(&self, url: &str) -> Result<String, String> {
        self.broker
            .download_cookies(&url::Url::parse(url).map_err(|error| error.to_string())?)
    }
    pub fn new(
        url: &url::Url,
        policy: &Policy,
        proxy: EventLoopProxy<Wake>,
        profile_dir: &std::path::Path,
        tab: u64,
    ) -> Result<Self, String> {
        let executable=executable().ok_or("Wega engine worker is not installed. Build it with python3 scripts/build-wega-engine.py --debug first.")?.canonicalize().map_err(|error| error.to_string())?;
        let mut command = Command::new(&executable);
        let launch = wega_platform::launch::PreparedLaunch::browser(
            &mut command,
            if cfg!(any(unix, windows)) {
                wega_platform::launch::BrokerTransport::InheritedEndpoint
            } else {
                wega_platform::launch::BrokerTransport::Tcp
            },
        )
        .map_err(|error| error.to_string())?;
        let development = launch.is_development();
        let budget = wega_platform::supervision::Supervisor::shared()?
            .reserve((protocol::MAX_PIXELS * 4 * 2) as u64)
            .map_err(|error| error.to_string())?;
        let broker = Arc::new(wega_network::Broker::shared()?.open_bound_session(
            profile_dir.join("wega-storage"),
            wega_network::Policy {
                offline: policy.offline,
                blocked_hosts: policy.blocked_hosts.clone(),
                allowed_hosts: policy.allowed_hosts.clone(),
                ..Default::default()
            },
        )?);
        broker.begin_navigation(url);
        let mut process = [0; 16];
        getrandom::fill(&mut process).map_err(|error| error.to_string())?;
        let identity = SessionIdentity {
            tab: TabId(tab),
            frame: FrameId(1),
            process: ProcessSessionId(process),
            service: Some(broker.identity()),
        };
        identity.validate().map_err(|error| error.message)?;
        let state = Arc::new(Mutex::new(Snapshot {
            authority: Some(FrameAuthority::new(identity)),
            url: url.to_string(),
            notices: if development {
                ["Explicit unsandboxed development worker".to_string()].into()
            } else {
                Default::default()
            },
            ..Default::default()
        }));
        let frames = Arc::new(
            wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4)
                .map_err(|e| e.to_string())?
                .into_reader(),
        );
        let frame_path = frames.path().unwrap().to_string_lossy().into_owned();
        let (broker_address, broker_token) = broker.endpoint();
        #[cfg(any(unix, windows))]
        let channels = broker.inherited_channels(4)?;
        #[cfg(any(unix, windows))]
        let broker_fds = launch
            .inherit_broker_channels(&mut command, &channels)
            .map_err(|error| error.to_string())?;
        #[cfg(not(any(unix, windows)))]
        let broker_fds = Vec::new();
        let broker_address = if broker_fds.is_empty() {
            Some(broker_address)
        } else {
            None
        };
        command.env("MALLOC_ARENA_MAX", "2");
        #[cfg(any(unix, windows))]
        let frame_fd = Some(
            launch
                .inherit_frame_file(
                    &mut command,
                    &frames.try_clone_file().map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?,
        );
        #[cfg(not(any(unix, windows)))]
        let frame_fd = None;
        let frame_path = if frame_fd.is_some() {
            None
        } else {
            Some(frame_path)
        };
        // Workers are private to a view, so process-wide V8 flags stay per-view.
        if std::env::var_os("WEGA_ENGINE_LITE").is_some_and(|value| value == "1")
            || wega::engine_choice::rollout_site(url)
        {
            command.arg("--lite");
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // Private worker, no console window.
        }
        let child = launch
            .spawn(&mut command)
            .map_err(|e| format!("Cannot start Wega engine: {e}"))?;
        drop(command);
        #[cfg(any(unix, windows))]
        drop(channels);
        let mut child = wega_platform::process::OwnedProcess::new(child, |report| {
            wega::diagnostics::record(
                &format!("worker:{}:exit", report.pid),
                &format!(
                    "Reaped content process: forced={}, status={:?}",
                    report.forced, report.status
                ),
            );
        });
        launch
            .attach(&mut child, budget.worker_memory_limit())
            .map_err(|error| error.to_string())?;
        let budget_state = state.clone();
        let budget_proxy = proxy.clone();
        let budget_broker = broker.clone();
        child.retain_until_exit(frames.clone());
        budget
            .attach(&mut child, move |violation| {
                budget_broker.revoke();
                {
                    let mut state = budget_state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    state.recovery_reason = Some(violation.to_string());
                }
                failed(&budget_state, violation.to_string(), &budget_proxy);
            })
            .map_err(|error| error.to_string())?;
        let worker_id = child.id();
        wega::diagnostics::record(
            &format!("worker:{worker_id}"),
            &format!("Started for {}", url),
        );
        if let Some(stderr) = child.take_stderr() {
            std::thread::spawn(move || {
                wega::diagnostics::drain(&format!("worker:{worker_id}:stderr"), stderr)
            });
        }
        let mut stdin = child.take_stdin().ok_or("Missing worker input")?;
        let mut stdout = child.take_stdout().ok_or("Missing worker output")?;
        let (sender, receiver) = mpsc::sync_channel(32);
        let shared = state.clone();
        let writer_proxy = proxy.clone();
        let writer_broker = broker.clone();
        let writer_budget = budget.clone();
        std::thread::spawn(move || {
            'commands: while let Ok(request) = receiver.recv() {
                if let Some(context) = protocol::scoped_context(&request) {
                    let (admitted, expired, cancellation) = {
                        let mut state = shared
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        let expired = state.expire_requests();
                        (
                            state.requests.contains(context),
                            expired,
                            state
                                .capabilities
                                .contains(&protocol::Capability::QueuedCancellation),
                        )
                    };
                    if cancellation {
                        for context in expired {
                            if let Err(error) = protocol::write(
                                &mut stdin,
                                &identity.command(Request::CancelRequest(context)),
                            ) {
                                writer_broker.revoke();
                                writer_budget.terminate();
                                failed(
                                    &shared,
                                    format!("Wega engine cancellation failed: {error}"),
                                    &writer_proxy,
                                );
                                break 'commands;
                            }
                        }
                    }
                    if !admitted {
                        let _ = writer_proxy.send_event(Wake::Ui);
                        continue;
                    }
                }
                if let Request::Load { url, .. } = &request
                    && let Ok(url) = url::Url::parse(url)
                {
                    writer_broker.begin_navigation(&url);
                }
                let packet = if matches!(request, Request::Init { .. }) {
                    request
                } else {
                    identity.command(request)
                };
                if let Err(error) = protocol::write(&mut stdin, &packet) {
                    writer_broker.revoke();
                    writer_budget.terminate();
                    failed(
                        &shared,
                        format!("Wega engine connection failed: {error}"),
                        &writer_proxy,
                    );
                    break;
                }
            }
        });
        let shared = state.clone();
        let reader_frames = frames.clone();
        let acknowledgements = sender.clone();
        let reader_broker = broker.clone();
        let reader_budget = budget.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                let mut ready = false;
                while let Some(event) =
                    protocol::read::<Event>(&mut stdout).map_err(|e| e.to_string())?
                {
                    let event = identity
                        .receive_event(event)
                        .map_err(|error| error.message)?;
                    if !ready {
                        if !matches!(
                            event,
                            Event::Ready {
                                version: protocol::VERSION
                            }
                        ) {
                            return Err("Incompatible Wega engine worker".into());
                        }
                        ready = true;
                        reader_budget.ready();
                        shared
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .lifecycle
                            .ready();
                        continue;
                    }
                    match event {
                        Event::ShutdownComplete => {
                            let mut state = shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if !state.lifecycle.acknowledge_close() {
                                return Err("Unrequested worker shutdown acknowledgement".into());
                            }
                        }
                        Event::Content { context, event } => {
                            if let ContentEvent::ScriptStopped(reason) = &event
                                && shared
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .accepts_context(context)
                            {
                                return Err(format!(
                                    "Script execution stopped: {}",
                                    reason.chars().take(4096).collect::<String>()
                                ));
                            }

                            if let ContentEvent::Notice(message)
                            | ContentEvent::Compatibility(message) = &event
                            {
                                wega::diagnostics::record(
                                    &format!(
                                        "worker:{worker_id}:document:{}:{}",
                                        context.document.0, context.revision.0
                                    ),
                                    message,
                                );
                            }
                            let denial = shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .content_event(context, event);
                            if let Some((context, id)) = denial {
                                acknowledgements
                                    .send(Request::AnswerDialog {
                                        context,
                                        id,
                                        value: None,
                                    })
                                    .map_err(|_| "Worker closed before stale dialog denial")?;
                            }
                        }
                        Event::DocumentCommitted {
                            serial,
                            revision,
                            url,
                        } => {
                            let mut state = shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if serial == state.requests.document().0
                                && state.lifecycle.accepts_requests()
                                && (!state.lifecycle.accepts_content()
                                    || revision != state.requests.revision())
                            {
                                state.bind_principal(
                                    serial,
                                    revision,
                                    reader_broker.take_committed_principal(url.as_deref()),
                                )?;
                            }
                        }
                        Event::Capabilities { version, supported } => {
                            if version != protocol::VERSION {
                                return Err("Capability version mismatch".into());
                            }
                            shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .capabilities = supported;
                        }
                        Event::DocumentState { context, dirty } => {
                            let mut state = shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if let Some(request) = state.requests.take(context) {
                                let RequestKind::PageState(browser_request) = request.kind else {
                                    state.request_failed(
                                        request,
                                        ProtocolError::new(
                                            ErrorCode::InvalidRequest,
                                            "Mismatched page-state response",
                                        ),
                                    );
                                    return Err("Mismatched page-state response".into());
                                };
                                if state.page_checks.len() < protocol::requests::MAX_PENDING {
                                    state.page_checks.push((
                                        context,
                                        browser_request,
                                        dirty || request.deadline <= std::time::Instant::now(),
                                    ));
                                }
                            }
                        }
                        Event::DocumentEvaluated { context, result } => {
                            let mut state = shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if let Some(request) = state.requests.take(context) {
                                if request.kind != RequestKind::Evaluate {
                                    state.request_failed(
                                        request,
                                        ProtocolError::new(
                                            ErrorCode::InvalidRequest,
                                            "Mismatched evaluation response",
                                        ),
                                    );
                                    return Err("Mismatched evaluation response".into());
                                }
                                let result = if request.deadline <= std::time::Instant::now() {
                                    Err(ProtocolError::new(
                                        ErrorCode::Timeout,
                                        "Evaluation timed out",
                                    ))
                                } else if result
                                    .as_ref()
                                    .is_ok_and(|value| value.len() > protocol::MAX_RESULT_BYTES)
                                {
                                    Err(ProtocolError::new(
                                        ErrorCode::ResourceLimit,
                                        "Evaluation result exceeds the IPC budget",
                                    ))
                                } else {
                                    result.map_err(|error| {
                                        ProtocolError::new(error.code, error.message)
                                    })
                                };
                                state.evaluation_result(context, result);
                            }
                        }
                        Event::RequestFailed { context, error } => {
                            let mut state = shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if let Some(request) = state.requests.take(context) {
                                state.request_failed(request, error);
                            }
                        }
                        Event::DocumentCaptured {
                            context,
                            width,
                            height,
                            hash,
                        } => {
                            protocol::frame_bytes(width, height)
                                .map_err(|error| error.to_string())?;
                            if hash.len() != 64
                                || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                            {
                                return Err("Invalid capture digest".into());
                            }
                            shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .complete_harness(
                                    context,
                                    RequestKind::Capture,
                                    Event::DocumentCaptured {
                                        context,
                                        width,
                                        height,
                                        hash,
                                    },
                                )?;
                        }
                        Event::DocumentNavigated { context, status } => {
                            shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .complete_harness(
                                    context,
                                    RequestKind::Navigate,
                                    Event::DocumentNavigated { context, status },
                                )?;
                        }
                        Event::Diagnostic { context, record } => {
                            let state = shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if state.lifecycle.accepts_requests()
                                && context.is_none_or(|context| state.accepts_context(context))
                            {
                                wega::diagnostics::record(
                                    &format!("worker:{worker_id}:diagnostic"),
                                    &format!(
                                        "{:?}: {}",
                                        record.level,
                                        record.message.chars().take(4096).collect::<String>()
                                    ),
                                );
                            }
                        }
                        Event::PageState { .. } | Event::Evaluated { .. } => {
                            return Err("Unscoped worker response".into());
                        }
                        Event::SharedFrame {
                            serial,
                            revision,
                            width,
                            height,
                            slot,
                            sequence,
                        } => {
                            let size =
                                protocol::frame_bytes(width, height).map_err(|e| e.to_string())?;
                            // Acknowledge stale/frozen frames without faulting their pages back in.
                            let accepted = {
                                let state = shared
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                serial == state.requests.document().0
                                    && revision == state.requests.revision()
                                    && state.lifecycle.presents_frames()
                            };
                            // No UI lock or host mmap spans untrusted backing I/O.
                            // accept_frame revalidates the scope after the copy.
                            let bytes = if accepted {
                                std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
                                Some(
                                    reader_frames
                                        .read_slot(slot as usize, size)
                                        .map_err(|e| e.to_string())?,
                                )
                            } else {
                                None
                            };
                            acknowledgements
                                .send(Request::FrameConsumed { slot, sequence })
                                .map_err(|_| "Worker closed before frame acknowledgement")?;
                            if let Some(bytes) = bytes {
                                accept_frame(&shared, serial, revision, width, height, bytes);
                            }
                        }
                        Event::Compatibility { .. } => {
                            return Err("Unscoped compatibility event".into());
                        }
                        Event::Frame {
                            serial,
                            revision,
                            width,
                            height,
                        } => {
                            let mut bytes = vec![
                                0;
                                protocol::frame_bytes(width, height)
                                    .map_err(|e| e.to_string())?
                            ];
                            stdout.read_exact(&mut bytes).map_err(|e| e.to_string())?;
                            accept_frame(&shared, serial, revision, width, height, bytes);
                        }
                        Event::State {
                            serial,
                            rss_bytes: _,
                            url,
                            title,
                            loading,
                            nodes,
                            js_heap,
                            back,
                            forward,
                        } => {
                            let mut state = shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if serial != state.requests.document().0
                                || !state.lifecycle.accepts_requests()
                            {
                                continue;
                            }
                            state.rss_bytes = reader_budget.resident_bytes();
                            state.document_back = back;
                            state.document_forward = forward;
                            if state.lifecycle.loading()
                                && !loading
                                && state.lifecycle.accepts_content()
                            {
                                state.visited = Some((url.clone(), title.clone()));
                            }
                            state.url = url;
                            state.title = title;
                            state.lifecycle.observe_loading(loading);
                            state.nodes = nodes;
                            state.js_heap = js_heap;
                        }
                        Event::Notice(_)
                        | Event::Navigate(_)
                        | Event::Download { .. }
                        | Event::OpenWindow(_)
                        | Event::TraverseUnhandled(_)
                        | Event::Cursor(_)
                        | Event::Dialog { .. } => return Err("Unscoped content event".into()),
                        Event::Ready { .. } | Event::Session { .. } => {
                            return Err("Unexpected worker handshake/envelope".into());
                        }
                    }
                    let _ = proxy.send_event(Wake::Ui);
                }
                Err("Wega engine stopped. Reload this page or switch to Servo.".into())
            })();
            if let Err(error) = result {
                reader_broker.revoke();
                reader_budget.terminate();
                failed(&shared, error, &proxy);
            }
        });
        let page = Self {
            child,
            _frames: frames,
            broker,
            _budget: budget,
            requests: Some(sender),
            state,
            texture: None,
            viewport: None,
            zoom: 1.0,
            dialog_input: String::new(),
            dialog_input_for: None,
            history: vec![url.to_string()],
            index: 0,
        };
        page.send(Request::Init {
            identity: Some(Box::new(identity)),
            version: protocol::VERSION,
            url: url.to_string(),
            offline: policy.offline,
            blocked_hosts: policy.blocked_hosts.clone(),
            allowed_hosts: policy.allowed_hosts.clone(),
            broker_address,
            broker_token: Some(broker_token),
            broker_fds,
            frame_path,
            frame_fd,
            storage_dir: None,
        })?;
        page.send(Request::Capabilities)?;
        Ok(page)
    }
    pub fn cancel_queued(&self, contexts: Vec<RequestContext>) {
        if contexts.is_empty() {
            return;
        }
        let supported = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .capabilities
            .contains(&protocol::Capability::QueuedCancellation);
        self.send_cancellations(supported, contexts);
    }

    fn send_cancellations(
        &self,
        supported: bool,
        contexts: impl IntoIterator<Item = RequestContext>,
    ) {
        if !supported {
            return;
        }
        if let Some(sender) = &self.requests {
            for context in contexts {
                // Queue deadlines still apply if the bounded writer queue is full.
                let _ = sender.try_send(Request::CancelRequest(context));
            }
        }
    }

    pub fn send(&self, request: Request) -> Result<(), String> {
        let (request, scoped, expired) = match request {
            Request::Evaluate(source) => {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let expired = state.expire_requests();
                if !state.lifecycle.accepts_requests() {
                    return Err("Content worker is stopped".into());
                }
                let context = state
                    .requests
                    .begin(
                        RequestKind::Evaluate,
                        std::time::Duration::from_millis(protocol::EVALUATION_TIMEOUT_MS),
                    )
                    .map_err(|error| error.message)?;
                state.interacted = true;
                state.evaluated = None;
                (
                    Request::EvaluateDocument { context, source },
                    Some(context),
                    expired,
                )
            }
            Request::CheckPageState(browser_request) => {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let expired = state.expire_requests();
                if !state.lifecycle.accepts_requests() {
                    return Err("Content worker is stopped".into());
                }
                let context = state
                    .requests
                    .begin(
                        RequestKind::PageState(browser_request),
                        std::time::Duration::from_millis(protocol::PAGE_STATE_TIMEOUT_MS),
                    )
                    .map_err(|error| error.message)?;
                (Request::CheckDocumentState(context), Some(context), expired)
            }
            request => (request, None, Vec::new()),
        };
        self.cancel_queued(expired);
        let activates = matches!(&request, Request::Click { .. })
            || matches!(&request, Request::Pointer { kind, .. } if kind == "down")
            || matches!(&request, Request::Key { pressed: true, repeat: false, key, .. } if !matches!(key.as_str(), "Escape" | "Shift" | "Control" | "Alt" | "Meta" | "Unidentified"));
        if activates {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let context = DocumentContext {
                document: state.requests.document(),
                revision: state.requests.revision(),
            };
            if state.lifecycle.accepts_content()
                && let Some(authority) = &mut state.authority
                && let Some(activation) = authority.activate(context, std::time::Instant::now())
            {
                if !self.requests.as_ref().is_some_and(|sender| {
                    sender
                        .try_send(Request::Activate {
                            context,
                            activation,
                        })
                        .is_ok()
                }) {
                    authority.clear_activation();
                    return Err("Worker could not accept trusted input activation".into());
                }
            }
        }
        if matches!(
            &request,
            Request::Click { .. } | Request::Key { pressed: true, .. } | Request::Ime { .. }
        ) || matches!(&request, Request::Pointer { kind, .. } if kind == "down")
        {
            self.state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .interacted = true;
        }
        if let Request::Policy {
            offline,
            blocked_hosts,
            allowed_hosts,
        } = &request
        {
            self.broker.set_policy(wega_network::Policy {
                offline: *offline,
                blocked_hosts: blocked_hosts.clone(),
                allowed_hosts: allowed_hosts.clone(),
                ..Default::default()
            });
        }
        let result = self
            .requests
            .as_ref()
            .ok_or("Worker is closed")?
            .try_send(request)
            .map_err(|_| "Wega engine is busy or stopped. Reload or switch to Servo.".to_string());
        if result.is_err() && activates {
            if let Some(authority) = &mut self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .authority
            {
                authority.clear_activation();
            }
        }
        if result.is_err()
            && let Some(context) = scoped
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(request) = state.requests.take(context) {
                state.request_failed(
                    request,
                    ProtocolError::new(ErrorCode::Cancelled, "Worker command was not queued"),
                );
            }
        }
        result
    }
    pub fn keyboard(
        &self,
        event: &winit::event::KeyEvent,
        modifiers: winit::keyboard::ModifiersState,
    ) -> Result<(), String> {
        let key = match &event.logical_key {
            winit::keyboard::Key::Character(value) => value.to_string(),
            winit::keyboard::Key::Named(value) => format!("{value:?}"),
            _ => "Unidentified".into(),
        };
        let code = match event.physical_key {
            winit::keyboard::PhysicalKey::Code(code) => format!("{code:?}"),
            _ => "Unidentified".into(),
        };
        let flags = u8::from(modifiers.shift_key())
            | u8::from(modifiers.control_key()) << 1
            | u8::from(modifiers.alt_key()) << 2
            | u8::from(modifiers.super_key()) << 3;
        self.send(Request::Key {
            key,
            code,
            text: event.text.as_ref().map(ToString::to_string),
            pressed: event.state.is_pressed(),
            modifiers: flags,
            repeat: event.repeat,
        })
    }
    pub fn ime(&self, event: &winit::event::Ime) -> Result<(), String> {
        match event {
            winit::event::Ime::Preedit(text, cursor) => self.send(Request::Ime {
                text: text.clone(),
                preedit: true,
                cursor: *cursor,
            }),
            winit::event::Ime::Commit(text) => self.send(Request::Ime {
                text: text.clone(),
                preedit: false,
                cursor: None,
            }),
            _ => Ok(()),
        }
    }
    pub fn command_sender(&self) -> Option<mpsc::SyncSender<Request>> {
        self.requests.clone()
    }
    pub fn url(&self) -> String {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .url
            .clone()
    }
    pub fn title(&self) -> String {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .title
            .clone()
    }
    pub fn loading(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .lifecycle
            .loading()
    }
    fn document_history(&self) -> (bool, bool) {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (state.document_back, state.document_forward)
    }
    pub fn can_back(&self) -> bool {
        self.index > 0 || self.document_history().0
    }
    pub fn can_forward(&self) -> bool {
        self.index + 1 < self.history.len() || self.document_history().1
    }
    /// Pointer input in viewport CSS pixels; see `Command::Pointer`.
    /// `buttons` is (DOM `button`, DOM `buttons`).
    pub fn pointer(
        &self,
        kind: &str,
        (x, y): (f32, f32),
        (button, buttons): (u8, u8),
        modifiers: u8,
        delta: (f64, f64),
    ) {
        let _ = self.send(Request::Pointer {
            kind: kind.into(),
            x,
            y,
            button,
            buttons,
            modifiers,
            delta_x: delta.0,
            delta_y: delta.1,
        });
    }
    /// Answer the open dialog (None cancels).
    pub fn answer_dialog(&mut self, context: DocumentContext, id: DialogId, value: Option<String>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.accepts_context(context)
            || !state.dialog.as_ref().is_some_and(|dialog| {
                dialog.context == context
                    && dialog.id == id
                    && state
                        .authority
                        .as_ref()
                        .and_then(|authority| authority.resolve(context))
                        .as_ref()
                        == Some(&dialog.requester)
            })
        {
            return;
        }
        if self
            .send(Request::AnswerDialog { context, id, value })
            .is_err()
        {
            return;
        }
        state.dialog = None;
        self.dialog_input.clear();
        self.dialog_input_for = None;
    }
    pub fn freeze(&mut self, frozen: bool) {
        if self.send(Request::Freeze(frozen)).is_err() {
            return;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.lifecycle.freeze(frozen);
        if frozen {
            if let Some(authority) = &mut state.authority {
                authority.clear_activation();
            }
            self.texture = None;
            state.frame = None;
        }
        self.viewport = None;
    }
    pub fn resident_bytes(&self) -> Option<u64> {
        self._budget.resident_bytes()
    }
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    fn request_load(&self, url: &url::Url) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.lifecycle.accepts_requests() {
            return Err("Content worker is stopped".into());
        }
        let document = state
            .requests
            .next_document()
            .map_err(|error| error.message)?;
        self.send(Request::Load {
            url: url.to_string(),
            serial: document.0,
        })?;
        state.lifecycle.navigate();
        if let Some(authority) = &mut state.authority {
            authority.invalidate();
        }
        state.native.clear();
        state.dialog = None;
        state.cursor = None;
        state.evaluated = None;
        state.frame = None;
        for request in state
            .requests
            .navigate(document)
            .map_err(|error| error.message)?
        {
            self.send_cancellations(
                state
                    .capabilities
                    .contains(&protocol::Capability::QueuedCancellation),
                [request.context],
            );
            state.request_failed(
                request,
                ProtocolError::new(ErrorCode::StaleDocument, "Document navigated"),
            );
        }
        state.interacted = false;
        state.compatibility = None;
        state.fallback_reported = false;
        drop(state);
        Ok(())
    }
    pub fn load_url(&mut self, url: url::Url) -> Result<(), String> {
        self.request_load(&url)?;
        self.history.truncate(self.index + 1);
        self.history.push(url.to_string());
        self.index = self.history.len() - 1;
        if self.history.len() > 256 {
            self.history.remove(0);
            self.index -= 1;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.url = url.to_string();
        state.lifecycle.observe_loading(true);
        Ok(())
    }
    pub fn resize(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        dark: bool,
    ) -> Result<(), String> {
        let viewport = (width, height, scale, self.zoom, dark);
        if self.viewport != Some(viewport) {
            self.send(Request::Resize {
                width,
                height,
                scale,
                zoom: self.zoom,
                dark,
            })?;
            self.viewport = Some(viewport);
        }
        Ok(())
    }
}
impl NavigationPort for Page {
    fn load(&mut self, url: url::Url) {
        let _ = self.load_url(url);
    }
    fn reload(&mut self) {
        if let Ok(url) = url::Url::parse(&self.url()) {
            let _ = self.request_load(&url);
        }
    }
    fn traverse(&mut self, forward: bool) -> bool {
        // The page's own entries first; it reports TraverseUnhandled otherwise.
        let (back, ahead) = self.document_history();
        if (forward && ahead) || (!forward && back) {
            return self
                .send(Request::Traverse(if forward { 1 } else { -1 }))
                .is_ok();
        }
        if (forward && self.index + 1 >= self.history.len()) || (!forward && self.index == 0) {
            return false;
        }
        let next = if forward {
            self.index + 1
        } else {
            self.index - 1
        };
        if url::Url::parse(&self.history[next])
            .map_err(|e| e.to_string())
            .and_then(|url| self.request_load(&url))
            .is_err()
        {
            return false;
        }
        self.index = next;
        true
    }
    fn zoom(&self) -> f32 {
        self.zoom
    }
    fn set_zoom(&mut self, value: f32) {
        self.zoom = value;
        self.viewport = None;
    }
}
impl Drop for Page {
    fn drop(&mut self) {
        self.broker.revoke();
        let graceful = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.lifecycle.close();
            if let Some(authority) = &mut state.authority {
                authority.invalidate();
            }
            state.cancel_requests(ProtocolError::new(
                ErrorCode::Cancelled,
                "Content session is closing",
            ));
            state.native.clear();
            state.dialog = None;
            state
                .capabilities
                .contains(&protocol::Capability::GracefulShutdown)
        };
        if graceful
            && self
                .requests
                .as_ref()
                .is_some_and(|sender| sender.try_send(Request::Shutdown).is_ok())
        {
            self.child
                .allow_graceful_shutdown(wega_platform::process::MAX_SHUTDOWN_GRACE);
        }
        self.requests.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn harness_replies_cannot_consume_an_evaluation_or_complete_after_expiry() {
        let (mut state, _) = committed_state();
        let evaluation = state
            .requests
            .begin(RequestKind::Evaluate, std::time::Duration::from_secs(5))
            .unwrap();
        assert!(
            state
                .complete_harness(
                    evaluation,
                    RequestKind::Capture,
                    Event::DocumentCaptured {
                        context: evaluation,
                        width: 1,
                        height: 1,
                        hash: "0".repeat(64)
                    }
                )
                .is_err()
        );
        assert!(state.evaluations.back().unwrap().1.is_err());
        let capture = state
            .requests
            .begin(RequestKind::Capture, std::time::Duration::ZERO)
            .unwrap();
        state
            .complete_harness(
                capture,
                RequestKind::Capture,
                Event::DocumentCaptured {
                    context: capture,
                    width: 1,
                    height: 1,
                    hash: "0".repeat(64),
                },
            )
            .unwrap();
        assert!(matches!(
            &state.harness_results.back().unwrap().1,
            Err(ProtocolError {
                code: ErrorCode::Timeout,
                ..
            })
        ));
        assert!(state.requests.take(capture).is_none());
    }

    #[test]
    fn native_actions_cannot_transfer_to_a_restarted_worker_with_the_same_document_counters() {
        let (mut old, context) = committed_state();
        old.content_event(
            context,
            ContentEvent::Navigate("https://example.test".into()),
        );
        let (requester, _) = old.take_native_events(true).pop().unwrap();
        let (mut replacement, replacement_context) = committed_state();
        assert_eq!(context, replacement_context);
        let mut session = requester.session;
        session.process = ProcessSessionId([8; 16]);
        let mut authority = FrameAuthority::new(session);
        authority.bind(context, requester.principal.clone());
        replacement.authority = Some(authority);
        assert!(replacement.accepts_context(context));
        assert!(!replacement.accepts_requester(&requester));
    }

    #[test]
    fn popup_authority_cannot_be_forged_replayed_or_renewed_by_evaluation() {
        use protocol::identity::ActivationId;
        let (mut state, context) = committed_state();
        let popup = |activation| ContentEvent::ActivatedOpenWindow {
            url: "https://example.test".into(),
            activation,
        };
        state.content_event(
            context,
            ContentEvent::OpenWindow("https://example.test".into()),
        );
        state.content_event(context, popup(ActivationId(1)));
        assert!(state.take_native_events(true).is_empty());
        let now = std::time::Instant::now();
        let token = state
            .authority
            .as_mut()
            .unwrap()
            .activate(context, now)
            .unwrap();
        state.content_event(context, popup(token));
        state.content_event(context, popup(token));
        assert_eq!(state.take_native_events(true).len(), 1);
        state.content_event(context, popup(token));
        assert!(state.take_native_events(true).is_empty());
        let token = state
            .authority
            .as_mut()
            .unwrap()
            .activate(context, now - std::time::Duration::from_secs(6))
            .unwrap();
        state.content_event(context, popup(token));
        assert!(state.take_native_events(true).is_empty());
        let token = state
            .authority
            .as_mut()
            .unwrap()
            .activate(context, now)
            .unwrap();
        let request = state
            .requests
            .begin(RequestKind::Evaluate, std::time::Duration::from_secs(5))
            .unwrap();
        state.evaluation_result(request, Ok("true".into()));
        state.content_event(context, popup(token));
        assert_eq!(state.take_native_events(true).len(), 1);
        state.evaluation_result(request, Ok("true".into()));
        state.content_event(context, popup(token));
        assert!(state.take_native_events(true).is_empty());
    }

    fn committed_state() -> (Snapshot, DocumentContext) {
        let mut state = Snapshot {
            authority: Some(FrameAuthority::new(SessionIdentity {
                tab: TabId(7),
                frame: FrameId(1),
                process: ProcessSessionId([7; 16]),
                service: None,
            })),
            ..Default::default()
        };
        state
            .bind_document(0, protocol::DocumentRevision(1))
            .unwrap();
        state.lifecycle.observe_loading(false);
        (
            state,
            DocumentContext {
                document: protocol::DocumentId(0),
                revision: protocol::DocumentRevision(1),
            },
        )
    }

    #[test]
    fn buffered_actions_cannot_escape_a_replacement_document_or_pending_navigation() {
        let (mut state, context) = committed_state();
        state.content_event(
            context,
            ContentEvent::OpenWindow("https://old.example/".into()),
        );
        state.content_event(
            context,
            ContentEvent::Download {
                url: "https://old.example/file".into(),
                cookies: "private".into(),
            },
        );
        state
            .bind_document(0, protocol::DocumentRevision(2))
            .unwrap();
        assert!(state.take_native_events(true).is_empty());
        assert!(!state.accepts_context(context));
        let current = DocumentContext {
            revision: protocol::DocumentRevision(2),
            ..context
        };
        state.content_event(
            current,
            ContentEvent::Navigate("https://new.example/".into()),
        );
        state.lifecycle.navigate();
        assert!(state.take_native_events(true).is_empty());
        assert!(!state.lifecycle.presents_frames());
    }

    #[test]
    fn a_duplicate_commit_does_not_close_an_inline_dialog_and_a_late_close_cannot_close_its_successor()
     {
        let (mut state, context) = committed_state();
        let dialog = |id| ContentEvent::Dialog {
            id: DialogId(id),
            kind: "confirm".into(),
            message: "Question".into(),
            default: String::new(),
        };
        assert!(state.content_event(context, dialog(1)).is_none());
        state.bind_document(0, context.revision).unwrap();
        assert_eq!(state.dialog.as_ref().unwrap().id, DialogId(1));
        assert!(state.content_event(context, dialog(1)).is_none());
        state.content_event(context, ContentEvent::DialogClosed(DialogId(1)));
        state.content_event(context, dialog(2));
        state.content_event(context, ContentEvent::DialogClosed(DialogId(1)));
        assert_eq!(state.dialog.as_ref().unwrap().id, DialogId(2));
        assert_eq!(
            state.content_event(context, dialog(1)),
            Some((context, DialogId(1)))
        );
        state.lifecycle.navigate();
        assert_eq!(
            state.content_event(context, dialog(3)),
            Some((context, DialogId(3)))
        );
    }

    #[test]
    fn background_actions_remain_bounded_and_wait_for_their_own_tab() {
        let (mut state, context) = committed_state();
        let activation = state
            .authority
            .as_mut()
            .unwrap()
            .activate(context, std::time::Instant::now())
            .unwrap();
        state.content_event(
            context,
            ContentEvent::ActivatedOpenWindow {
                url: "https://example.org/".into(),
                activation,
            },
        );
        state.content_event(
            context,
            ContentEvent::Download {
                url: "https://example.org/file".into(),
                cookies: String::new(),
            },
        );
        let background = state.take_native_events(false);
        assert_eq!(background.len(), 1);
        assert!(matches!(&background[0].1, ContentEvent::Download { .. }));
        for _ in 0..100 {
            state.content_event(
                context,
                ContentEvent::Navigate("https://example.org/".into()),
            );
        }
        assert_eq!(state.native.len(), 32);
        assert!(matches!(
            &state.take_native_events(true)[0].1,
            ContentEvent::ActivatedOpenWindow { .. }
        ));
    }

    #[test]
    fn a_clean_reply_buffered_before_navigation_cannot_authorize_closing_the_new_document() {
        let mut state = Snapshot::default();
        let context = state
            .requests
            .begin(RequestKind::PageState(7), std::time::Duration::from_secs(2))
            .unwrap();
        state.requests.take(context).unwrap();
        state.page_checks.push((context, 7, false));
        state
            .requests
            .navigate(state.requests.next_document().unwrap())
            .unwrap();
        assert_eq!(state.take_page_checks(), vec![(7, true)]);
        assert!(state.take_page_checks().is_empty());
    }

    #[test]
    fn a_clean_reply_from_an_old_dom_is_stale_even_without_a_browser_navigation() {
        let mut state = Snapshot::default();
        state
            .requests
            .commit(protocol::DocumentRevision(1))
            .unwrap();
        let context = state
            .requests
            .begin(
                RequestKind::PageState(10),
                std::time::Duration::from_secs(2),
            )
            .unwrap();
        state.requests.take(context).unwrap();
        state.page_checks.push((context, 10, false));
        state
            .requests
            .commit(protocol::DocumentRevision(2))
            .unwrap();
        assert_eq!(state.take_page_checks(), vec![(10, true)]);
    }

    #[test]
    fn timeout_and_worker_exit_require_confirmation_and_resolve_evaluations() {
        let mut state = Snapshot::default();
        state
            .requests
            .begin(RequestKind::PageState(8), std::time::Duration::ZERO)
            .unwrap();
        state.expire_requests();
        assert_eq!(state.take_page_checks(), vec![(8, true)]);
        state
            .requests
            .begin(RequestKind::PageState(9), std::time::Duration::from_secs(2))
            .unwrap();
        let evaluation = state
            .requests
            .begin(RequestKind::Evaluate, std::time::Duration::from_secs(5))
            .unwrap();
        state.cancel_requests(ProtocolError::new(ErrorCode::Cancelled, "Worker stopped"));
        assert_eq!(state.take_page_checks(), vec![(9, true)]);
        let (context, result) = state.evaluations.pop_front().unwrap();
        assert_eq!(context, evaluation);
        assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
        assert!(state.requests.take(evaluation).is_none());
    }
}
