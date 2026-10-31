//! Bounded, passive network recording from Servo's own developer-tool event stream.
#[path = "network_har.rs"]
mod har;
use crossbeam_channel::{Receiver, Sender};
use devtools_traits::{
    ChromeToDevtoolsControlMsg as Chrome, DevtoolScriptControlMsg, DevtoolsControlMsg,
    NetworkEvent, ScriptToDevtoolsControlMsg as Script,
};
use serde_json::{Value, json};
use servo_base::{
    generic_channel::GenericSender,
    id::{BrowsingContextId, PipelineId, WebViewId},
};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant, UNIX_EPOCH},
};

pub const BODY_LIMIT: usize = 64 * 1024;
pub const ROW_LIMIT: usize = 500;
pub fn owner(view: WebViewId) -> String {
    format!("{:?}", BrowsingContextId::from(view))
}
fn headers(headers: &http::HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .take(200)
        .map(|(key, value)| {
            (
                key.to_string(),
                String::from_utf8_lossy(value.as_bytes())
                    .chars()
                    .take(8192)
                    .collect(),
            )
        })
        .collect()
}
#[derive(Clone, Debug, Default)]
pub struct Body {
    pub bytes: Vec<u8>,
    pub size: usize,
}
impl Body {
    fn from_bytes(bytes: Option<&[u8]>) -> Self {
        bytes
            .map(|bytes| Self {
                bytes: bytes[..bytes.len().min(BODY_LIMIT)].to_vec(),
                size: bytes.len(),
            })
            .unwrap_or_default()
    }
    pub fn truncated(&self) -> bool {
        self.size > self.bytes.len()
    }
    pub fn text(&self) -> Option<&str> {
        std::str::from_utf8(&self.bytes).ok()
    }
}
#[derive(Debug)]
pub enum Payload {
    Request {
        initial: bool,
        url: String,
        method: String,
        headers: Vec<(String, String)>,
        body: Body,
        kind: String,
        started_ms: u64,
        connect_ms: f64,
        send_ms: f64,
    },
    Response {
        status: u16,
        reason: String,
        headers: Vec<(String, String)>,
        body: Body,
        cached: bool,
    },
    Security(String),
}
#[derive(Debug)]
pub struct Update {
    pub id: String,
    pub owner: String,
    pub at: Instant,
    pub payload: Payload,
}
pub enum BackendEvent {
    Network(Update),
    Global {
        view: WebViewId,
        pipeline: PipelineId,
        sender: GenericSender<DevtoolScriptControlMsg>,
        top: bool,
    },
    Source(PipelineId, devtools_traits::SourceInfo),
    Frame(PipelineId, String, devtools_traits::FrameInfo),
    Paused(
        PipelineId,
        devtools_traits::FrameOffset,
        devtools_traits::PauseReason,
    ),
    Navigation(String),
    Console(servo_base::id::PipelineId, devtools_traits::ConsoleMessage),
    PageError(servo_base::id::PipelineId, devtools_traits::PageError),
    CssError(servo_base::id::PipelineId, devtools_traits::CSSError),
    ClearConsole(servo_base::id::PipelineId),
    Object(String, devtools_traits::DebuggerValue),
    Environment(String, devtools_traits::EnvironmentInfo, Option<String>),
}
pub struct Bridge {
    pub sender: Sender<DevtoolsControlMsg>,
    pub events: Receiver<BackendEvent>,
    pub dropped: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Bridge {
    pub fn new(
        proxy: winit::event_loop::EventLoopProxy<crate::engine::Wake>,
    ) -> Result<Self, String> {
        let (sender, raw) = crossbeam_channel::unbounded();
        let (ui, events) = crossbeam_channel::bounded(512);
        let stop = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicUsize::new(0));
        let worker_stop = stop.clone();
        let worker_dropped = dropped.clone();
        let worker = std::thread::Builder::new()
            .name("native-devtools".into())
            .spawn(move || {
                let mut actor = 0_u64;
                while !worker_stop.load(Ordering::Relaxed) {
                    let message = match raw.recv_timeout(Duration::from_millis(100)) {
                        Ok(message) => message,
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    };
                    let event = match message {
                        DevtoolsControlMsg::FromChrome(Chrome::NetworkEvent(id, event)) => {
                            normalize(id, event).map(BackendEvent::Network)
                        }
                        DevtoolsControlMsg::FromScript(Script::NewGlobal(
                            (_, pipeline, worker, view),
                            sender,
                            info,
                        )) => Some(BackendEvent::Global {
                            view,
                            pipeline,
                            sender,
                            top: worker.is_none() && info.is_top_level_global,
                        }),
                        DevtoolsControlMsg::FromScript(Script::CreateSourceActor(
                            _,
                            pipeline,
                            mut source,
                        )) => {
                            if let Some(text) = &mut source.content
                                && text.len() > 1024 * 1024
                            {
                                let mut end = 1024 * 1024;
                                while !text.is_char_boundary(end) {
                                    end -= 1;
                                }
                                text.truncate(end);
                            }
                            Some(BackendEvent::Source(pipeline, source))
                        }
                        DevtoolsControlMsg::FromScript(Script::CreateFrameActor(
                            reply,
                            pipeline,
                            frame,
                        )) => {
                            actor += 1;
                            let name = format!("native-frame-{actor}");
                            let _ = reply.send(name.clone());
                            Some(BackendEvent::Frame(pipeline, name, frame))
                        }
                        DevtoolsControlMsg::FromScript(Script::CreateObjectActor(reply, value)) => {
                            actor += 1;
                            let name = format!("native-object-{actor}");
                            let _ = reply.send(name.clone());
                            Some(BackendEvent::Object(name, value))
                        }
                        DevtoolsControlMsg::FromScript(Script::CreateEnvironmentActor(
                            reply,
                            value,
                            parent,
                            existing,
                        )) => {
                            actor += 1;
                            let name =
                                existing.unwrap_or_else(|| format!("native-environment-{actor}"));
                            let _ = reply.send(name.clone());
                            Some(BackendEvent::Environment(name, value, parent))
                        }
                        DevtoolsControlMsg::FromScript(Script::ConsoleAPI(
                            pipeline,
                            message,
                            _,
                        )) => Some(BackendEvent::Console(pipeline, message)),
                        DevtoolsControlMsg::FromScript(Script::ReportPageError(
                            pipeline,
                            error,
                        )) => Some(BackendEvent::PageError(pipeline, error)),
                        DevtoolsControlMsg::FromScript(Script::ReportCSSError(pipeline, error)) => {
                            Some(BackendEvent::CssError(pipeline, error))
                        }
                        DevtoolsControlMsg::FromScript(Script::ClearConsole(pipeline, worker))
                            if worker.is_none() =>
                        {
                            Some(BackendEvent::ClearConsole(pipeline))
                        }
                        DevtoolsControlMsg::FromScript(Script::DebuggerPause(
                            pipeline,
                            frame,
                            reason,
                        )) => Some(BackendEvent::Paused(pipeline, frame, reason)),
                        DevtoolsControlMsg::FromScript(Script::Navigate(
                            context,
                            devtools_traits::NavigationState::Start(_),
                        )) => Some(BackendEvent::Navigation(format!("{context:?}"))),
                        DevtoolsControlMsg::FromChrome(Chrome::ServerExitMsg) => break,
                        _ => None,
                    };
                    if let Some(event) = event {
                        if matches!(
                            event,
                            BackendEvent::Network(_)
                                | BackendEvent::Source(..)
                                | BackendEvent::Console(..)
                                | BackendEvent::PageError(..)
                                | BackendEvent::CssError(..)
                                | BackendEvent::Object(..)
                        ) {
                            if ui.try_send(event).is_err() {
                                worker_dropped.fetch_add(1, Ordering::Relaxed);
                            }
                        } else {
                            let mut pending = event;
                            loop {
                                let _ = proxy.send_event(crate::engine::Wake::Ui);
                                match ui.send_timeout(pending, Duration::from_millis(20)) {
                                    Ok(()) => break,
                                    Err(crossbeam_channel::SendTimeoutError::Timeout(event))
                                        if !worker_stop.load(Ordering::Relaxed) =>
                                    {
                                        pending = event
                                    }
                                    Err(_) => break,
                                }
                            }
                        }
                        let _ = proxy.send_event(crate::engine::Wake::Ui);
                    }
                }
            })
            .map_err(|e| format!("Cannot start the developer tools observer: {e}"))?;
        Ok(Self {
            sender,
            events,
            dropped,
            stop,
            worker: Some(worker),
        })
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn normalize(id: String, event: NetworkEvent) -> Option<Update> {
    let at = Instant::now();
    let (context, payload) = match event {
        NetworkEvent::HttpRequest(request) | NetworkEvent::HttpRequestUpdate(request) => {
            // The initial event has no timestamp; the later request update has measured timings.
            let initial = request.time_stamp == 0;
            let kind = if request.is_xhr {
                "XHR".into()
            } else {
                match format!("{:?}", request.destination).as_str() {
                    "Document" => "HTML",
                    "Script" => "JS",
                    "Style" => "CSS",
                    "Image" => "Img",
                    "Font" => "Font",
                    "Audio" | "Video" | "Track" => "Media",
                    "Empty" => "Fetch",
                    _ => "Other",
                }
                .into()
            };
            (
                request.browsing_context_id,
                Payload::Request {
                    initial,
                    url: request.url.to_string(),
                    method: request.method.to_string(),
                    headers: headers(&request.headers),
                    body: Body::from_bytes(request.body.as_deref()),
                    kind,
                    started_ms: request
                        .started_date_time
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    connect_ms: request.connect_time.as_secs_f64() * 1000.0,
                    send_ms: request.send_time.as_secs_f64() * 1000.0,
                },
            )
        }
        NetworkEvent::HttpResponse(response) => (
            response.browsing_context_id,
            Payload::Response {
                status: response.status.raw_code(),
                reason: String::from_utf8_lossy(response.status.message()).into_owned(),
                headers: response.headers.as_ref().map(headers).unwrap_or_default(),
                body: Body::from_bytes(response.body.as_deref()),
                cached: response.from_cache,
            },
        ),
        NetworkEvent::SecurityInfo(info) => (
            info.browsing_context_id,
            Payload::Security(security_summary(
                &serde_json::to_value(&info.security_info).unwrap_or(Value::Null),
            )),
        ),
    };
    Some(Update {
        id,
        owner: format!("{context:?}"),
        at,
        payload,
    })
}
#[derive(Clone, Debug)]
pub struct Request {
    pub uid: u64,
    pub id: String,
    pub owner: String,
    pub url: String,
    pub method: String,
    pub kind: String,
    pub request_headers: Vec<(String, String)>,
    pub response_headers: Vec<(String, String)>,
    pub request_body: Body,
    pub response_body: Body,
    pub status: Option<u16>,
    pub reason: String,
    pub started: Instant,
    pub started_ms: u64,
    pub completed: Option<Instant>,
    pub cached: bool,
    pub connect_ms: f64,
    pub send_ms: f64,
    pub security: String,
    pub imported: bool,
    pub recorded_duration: Option<f64>,
    pub transferred: Option<u64>,
    pub initiator: String,
}
impl Request {
    pub fn duration_ms(&self) -> f64 {
        if let Some(duration) = self.recorded_duration {
            return duration;
        }
        self.completed
            .unwrap_or_else(Instant::now)
            .saturating_duration_since(self.started)
            .as_secs_f64()
            * 1000.0
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.response_headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    pub fn domain(&self) -> String {
        url::Url::parse(&self.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default()
    }
    pub fn filename(&self) -> String {
        url::Url::parse(&self.url)
            .ok()
            .map(|u| {
                let path = u
                    .path()
                    .rsplit('/')
                    .next()
                    .filter(|p| !p.is_empty())
                    .unwrap_or("/");
                match u.query() {
                    Some(q) => format!("{path}?{q}"),
                    None => path.into(),
                }
            })
            .unwrap_or_else(|| self.url.clone())
    }
    pub fn status_label(&self) -> String {
        match self.status {
            Some(0) => "Error".into(),
            Some(status) => status.to_string(),
            None => "Pending".into(),
        }
    }
    pub fn transfer(&self) -> Option<u64> {
        if let Some(size) = self.transferred {
            return Some(size);
        }
        if self.cached {
            Some(0)
        } else {
            self.header("content-length").and_then(|v| v.parse().ok())
        }
    }
}
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub enum Sort {
    #[default]
    Started,
    Status,
    Method,
    Domain,
    File,
    Type,
    Size,
    Transferred,
    Duration,
    Initiator,
}
pub fn size_label(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} kB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}
/// Request duration: milliseconds below one second, seconds above.
pub fn duration_label(ms: f64) -> String {
    if ms >= 1000.0 {
        format!("{:.2} s", ms / 1000.0)
    } else {
        format!("{ms:.0} ms")
    }
}
fn matches_query(row: &Request, query: &str) -> bool {
    query.split_whitespace().all(|term| {
        let (negative, term) = term
            .strip_prefix('-')
            .map(|t| (true, t))
            .unwrap_or((false, term));
        let term = term.to_lowercase();
        let matched = if let Some((key, value)) = term.split_once(':') {
            match key {
                "method" => row.method.eq_ignore_ascii_case(value),
                "status" => row.status_label().eq_ignore_ascii_case(value),
                "domain" => row.domain().to_lowercase().contains(value),
                "type" => row.kind.eq_ignore_ascii_case(value),
                "is" if value == "cached" => row.cached,
                "is" if value == "imported" => row.imported,
                _ => row.url.to_lowercase().contains(&term),
            }
        } else {
            format!(
                "{} {} {} {}",
                row.url,
                row.method,
                row.status_label(),
                row.kind
            )
            .to_lowercase()
            .contains(&term)
        };
        if negative { !matched } else { matched }
    })
}
impl Request {
    pub fn curl(&self) -> Result<String, String> {
        if self.request_body.truncated() {
            return Err(
                "Cannot copy a complete cURL command: the captured request body is truncated."
                    .into(),
            );
        }
        let body = self
            .request_body
            .text()
            .ok_or("Binary request bodies cannot be represented as a shell argument")?;
        if body.contains('\0') {
            return Err("NUL bytes cannot be represented as a shell argument".into());
        }
        let quote = |text: &str| format!("'{}'", text.replace('\'', "'\"'\"'"));
        let mut command = format!(
            "curl --request {} {}",
            quote(&self.method),
            quote(&self.url)
        );
        for (name, value) in &self.request_headers {
            if !name.eq_ignore_ascii_case("content-length")
                && !name.eq_ignore_ascii_case("connection")
            {
                command.push_str(&format!(" --header {}", quote(&format!("{name}: {value}"))));
            }
        }
        if !body.is_empty() {
            command.push_str(&format!(" --data-binary {}", quote(body)));
        }
        Ok(command)
    }
}
#[derive(Default)]
pub struct Recorder {
    pub rows: VecDeque<Request>,
    pub selected: Option<u64>,
    pub preserve: bool,
    pub paused: bool,
    pub query: String,
    pub filter: String,
    pub disable_cache: bool,
    pub types: std::collections::BTreeSet<String>,
    pub sort: Sort,
    pub descending: bool,
    pub show_initiator: bool,
    pub show_transferred: bool,
    next: u64,
}
impl Recorder {
    pub fn apply(&mut self, update: Update) {
        if self.paused {
            return;
        }
        match update.payload {
            Payload::Request {
                initial,
                url,
                method,
                headers,
                body,
                kind,
                started_ms,
                connect_ms,
                send_ms,
            } => {
                let existing = self
                    .rows
                    .iter_mut()
                    .rev()
                    .find(|r| r.id == update.id && r.owner == update.owner && r.url == url);
                if let Some(row) = existing {
                    row.method = method;
                    row.request_headers = headers;
                    row.kind = kind;
                    if body.size > 0 {
                        row.request_body = body;
                    }
                    row.connect_ms = connect_ms;
                    row.send_ms = send_ms;
                } else if initial
                    || self
                        .rows
                        .iter()
                        .any(|r| r.id == update.id && r.owner == update.owner)
                {
                    self.next += 1;
                    self.rows.push_back(Request {
                        uid: self.next,
                        id: update.id,
                        owner: update.owner,
                        url,
                        method,
                        kind,
                        request_headers: headers,
                        response_headers: Vec::new(),
                        request_body: body,
                        response_body: Body::default(),
                        status: None,
                        reason: String::new(),
                        started: update.at,
                        started_ms,
                        completed: None,
                        cached: false,
                        connect_ms,
                        send_ms,
                        security: String::new(),
                        imported: false,
                        recorded_duration: None,
                        transferred: None,
                        initiator: String::new(),
                    });
                    while self.rows.len() > ROW_LIMIT {
                        self.rows.pop_front();
                    }
                }
            }
            Payload::Response {
                status,
                reason,
                headers,
                body,
                cached,
            } => {
                if let Some(row) = self
                    .rows
                    .iter_mut()
                    .rev()
                    .find(|r| r.id == update.id && r.owner == update.owner)
                {
                    row.status = Some(status);
                    row.reason = reason;
                    row.response_headers = headers;
                    if body.size > 0 || row.response_body.size == 0 {
                        row.response_body = body;
                    }
                    row.cached = cached;
                    row.completed = Some(update.at);
                }
            }
            Payload::Security(info) => {
                if let Some(row) = self
                    .rows
                    .iter_mut()
                    .rev()
                    .find(|r| r.id == update.id && r.owner == update.owner)
                {
                    row.security = info;
                }
            }
        }
    }
    pub fn clear(&mut self, owner: &str) {
        self.rows.retain(|r| r.owner != owner);
        self.selected = None;
    }
    pub fn navigation(&mut self, owner: &str) {
        if !self.preserve {
            self.clear(owner);
        }
    }
    pub fn visible(&self, owner: &str) -> Vec<&Request> {
        let mut rows: Vec<_> = self
            .rows
            .iter()
            .filter(|r| {
                r.owner == owner
                    && (self.filter.is_empty() || r.kind == self.filter)
                    && (self.types.is_empty() || self.types.contains(&r.kind))
                    && matches_query(r, &self.query)
            })
            .collect();
        rows.sort_by(|a, b| {
            let ordering = match self.sort {
                Sort::Started => a.started_ms.cmp(&b.started_ms).then(a.uid.cmp(&b.uid)),
                Sort::Status => a.status.cmp(&b.status),
                Sort::Method => a.method.cmp(&b.method),
                Sort::Domain => a.domain().cmp(&b.domain()),
                Sort::File => a.filename().cmp(&b.filename()),
                Sort::Type => a.kind.cmp(&b.kind),
                Sort::Size => a.response_body.size.cmp(&b.response_body.size),
                Sort::Transferred => a.transfer().cmp(&b.transfer()),
                Sort::Duration => a.duration_ms().total_cmp(&b.duration_ms()),
                Sort::Initiator => a.initiator.cmp(&b.initiator),
            };
            if self.descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
        rows
    }
    pub fn har(&self, owner: &str) -> Value {
        har::export(self.rows.iter().filter(|r| r.owner == owner))
    }
    pub fn import_har(&mut self, owner: &str, bytes: &[u8]) -> Result<usize, String> {
        har::import(self, owner, bytes)
    }
}
fn har_headers(headers: &[(String, String)]) -> Vec<Value> {
    headers
        .iter()
        .map(|(name, value)| json!({"name":name,"value":value}))
        .collect()
}
fn iso_date(ms: u64) -> String {
    // RFC3339 UTC conversion without depending on the execution host timezone.
    let days = (ms / 86_400_000) as i64;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += i64::from(m <= 2);
    let secs = ms / 1000;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        secs / 3600 % 24,
        secs / 60 % 60,
        secs % 60,
        ms % 1000
    )
}
/// TLS details as "Label: value" lines for the Security tab, instead of a
/// Rust debug dump. Works on the serialized engine record so new fields
/// never break it; unknown fields are skipped.
pub fn security_summary(info: &Value) -> String {
    let Some(fields) = info.as_object() else {
        return "Not an HTTPS connection; there are no TLS details.".into();
    };
    let text = |value: &Value| match value {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "Yes".into(),
        Value::Bool(false) => "No".into(),
        other => other.to_string(),
    };
    let mut lines = Vec::new();
    for (key, label) in [
        ("state", "Connection"),
        ("protocol_version", "Protocol"),
        ("cipher_suite", "Cipher suite"),
        ("kea_group_name", "Key exchange"),
        ("signature_scheme_name", "Signature"),
        ("alpn_protocol", "Application protocol"),
        ("certificate_transparency", "Certificate transparency"),
        ("hsts", "Strict transport security (HSTS)"),
        ("used_ech", "Encrypted Client Hello"),
        ("used_ocsp", "OCSP stapling"),
    ] {
        if let Some(value) = fields.get(key).filter(|v| !v.is_null()) {
            lines.push(format!("{label}: {}", text(value)));
        }
    }
    if let Some(chain) = fields
        .get("certificate_chain_der")
        .and_then(Value::as_array)
    {
        lines.push(format!("Certificates in chain: {}", chain.len()));
    }
    if let Some(reasons) = fields.get("weakness_reasons").and_then(Value::as_array)
        && !reasons.is_empty()
    {
        let reasons: Vec<_> = reasons.iter().map(text).collect();
        lines.push(format!("Weaknesses: {}", reasons.join("; ")));
    }
    lines.join("\n")
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;

#[derive(Default)]
pub struct UiState {
    pub import_open: bool,
    pub import_path: String,
    pub composer: Option<Composer>,
    pub body_search: String,
    pub widths: [f32; 10],
}
pub struct Composer {
    pub url: String,
    pub method: String,
    pub headers: String,
    pub body: String,
}
impl Default for Composer {
    fn default() -> Self {
        Self {
            url: String::new(),
            method: "GET".into(),
            headers: String::new(),
            body: String::new(),
        }
    }
}
impl Composer {
    pub fn from_request(row: &Request) -> Self {
        Self {
            url: row.url.clone(),
            method: row.method.clone(),
            headers: row
                .request_headers
                .iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.to_ascii_lowercase().as_str(),
                        "host"
                            | "content-length"
                            | "cookie"
                            | "origin"
                            | "referer"
                            | "connection"
                            | "accept-encoding"
                    )
                })
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join("\n"),
            body: row.request_body.text().unwrap_or("").into(),
        }
    }
    pub fn script(&self) -> Result<String, String> {
        let url = url::Url::parse(&self.url).map_err(|e| e.to_string())?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("Requests require an HTTP or HTTPS URL".into());
        }
        let method =
            http::Method::from_bytes(self.method.trim().as_bytes()).map_err(|e| e.to_string())?;
        if matches!(
            method.as_str().to_ascii_uppercase().as_str(),
            "CONNECT" | "TRACE" | "TRACK"
        ) {
            return Err("This method is forbidden by page fetch".into());
        }
        if matches!(method, http::Method::GET | http::Method::HEAD) && !self.body.is_empty() {
            return Err("GET and HEAD cannot have a request body".into());
        }
        let mut headers = serde_json::Map::new();
        for line in self.headers.lines().filter(|l| !l.trim().is_empty()) {
            let (name, value) = line
                .split_once(':')
                .ok_or("Headers must use Name: value, one per line")?;
            let name = http::header::HeaderName::from_bytes(name.trim().as_bytes())
                .map_err(|e| e.to_string())?;
            let value = value.trim();
            http::HeaderValue::from_str(value).map_err(|e| e.to_string())?;
            headers.insert(name.to_string(), json!(value));
        }
        let mut options =
            json!({"method":method.as_str(),"headers":headers,"credentials":"same-origin"});
        if !self.body.is_empty() {
            options["body"] = json!(self.body);
        }
        Ok(format!(
            "fetch({},{}).then(r=>console.info('Network request completed:',r.status,r.url)).catch(e=>console.error('Network request failed:',String(e)))",
            serde_json::to_string(url.as_str()).unwrap(),
            options
        ))
    }
}
