//! Observable, crash-resilient subprocess test harness for the Wega engine worker.
#![allow(dead_code)]
use protocol::{
    Command, DocumentContext, DocumentId, DocumentRevision, Event, RequestContext, RequestId,
};
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Child, ChildStdin, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use wega_protocol as protocol;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestFailureKind {
    EngineError(String),
    Timeout(Duration),
    WorkerCrash { code: Option<i32> },
    HarnessFailure(String),
    AssertionFailure(String),
}

impl std::fmt::Display for TestFailureKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EngineError(e) => write!(f, "EngineError: {e}"),
            Self::Timeout(d) => write!(f, "Timeout after {d:?}"),
            Self::WorkerCrash { code } => write!(f, "WorkerCrash (exit code: {code:?})"),
            Self::HarnessFailure(h) => write!(f, "HarnessFailure: {h}"),
            Self::AssertionFailure(a) => write!(f, "AssertionFailure: {a}"),
        }
    }
}

impl std::error::Error for TestFailureKind {}

pub struct WorkerHarness {
    child: Child,
    stdin: ChildStdin,
    events: mpsc::Receiver<Event>,
    event_buffer: VecDeque<Event>,
    context: Arc<Mutex<DocumentContext>>,
    identity: Option<protocol::identity::SessionIdentity>,
    next_request: u64,
    diagnostics: Arc<Mutex<Vec<protocol::DiagnosticRecord>>>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
    crashed: Arc<AtomicBool>,
    reader_failure: Arc<Mutex<Option<TestFailureKind>>>,
}

impl WorkerHarness {
    pub fn start(frame_path: &Path) -> Result<Self, TestFailureKind> {
        Self::start_with_policy(frame_path, true)
    }

    pub fn start_with_policy(frame_path: &Path, offline: bool) -> Result<Self, TestFailureKind> {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_wega-engine"))
            .arg(wega_platform::launch::DEVELOPMENT_ARGUMENT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TestFailureKind::HarnessFailure(format!("Failed to spawn worker: {e}")))?;

        let stdin = child.stdin.take().ok_or_else(|| {
            TestFailureKind::HarnessFailure("Failed to capture worker stdin".into())
        })?;
        let mut stdout = child.stdout.take().ok_or_else(|| {
            TestFailureKind::HarnessFailure("Failed to capture worker stdout".into())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            TestFailureKind::HarnessFailure("Failed to capture worker stderr".into())
        })?;

        let (tx, events) = mpsc::sync_channel(32);
        let reader_failure = Arc::new(Mutex::new(None));
        let failed_reader = reader_failure.clone();
        let context = Arc::new(Mutex::new(DocumentContext {
            document: DocumentId(0),
            revision: DocumentRevision(0),
        }));
        let committed_context = context.clone();
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let diag_store = diagnostics.clone();

        use protocol::identity::{FrameId, ProcessSessionId, SessionIdentity, TabId};
        let identity = Some(SessionIdentity {
            tab: TabId(42),
            frame: FrameId(1),
            process: ProcessSessionId([42; 16]),
            service: None,
        });

        // Reader thread for stdout events
        thread::Builder::new()
            .name("harness-stdout-reader".into())
            .spawn(move || {
                loop {
                    let packet = match protocol::read::<Event>(&mut stdout) {
                        Ok(Some(packet)) => packet,
                        Ok(None) => break,
                        Err(error) => {
                            *failed_reader.lock().unwrap() = Some(TestFailureKind::HarnessFailure(
                                format!("Malformed worker IPC: {error}"),
                            ));
                            break;
                        }
                    };
                    let event = if let Some(identity) = identity {
                        match identity.receive_event(packet) {
                            Ok(ev) => ev,
                            Err(e) => {
                                *failed_reader.lock().unwrap() =
                                    Some(TestFailureKind::HarnessFailure(format!(
                                        "Foreign worker packet: {e:?}"
                                    )));
                                break;
                            }
                        }
                    } else {
                        packet
                    };

                    if let Event::DocumentCommitted {
                        serial, revision, ..
                    } = &event
                    {
                        *committed_context.lock().unwrap() = DocumentContext {
                            document: DocumentId(*serial),
                            revision: *revision,
                        };
                    } else if let Event::Diagnostic { record, .. } = &event {
                        let mut record = record.clone();
                        record.message = record.message.chars().take(4096).collect();
                        record.source = record
                            .source
                            .map(|source| source.chars().take(4096).collect());
                        let mut stored = diag_store.lock().unwrap();
                        if stored.len() == 256 {
                            stored.remove(0);
                        }
                        stored.push(record);
                    }

                    if tx.send(event).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| TestFailureKind::HarnessFailure(e.to_string()))?;

        // Stderr reader thread
        let stderr_lines = Arc::new(Mutex::new(Vec::new()));
        let stderr_store = stderr_lines.clone();
        thread::Builder::new()
            .name("harness-stderr-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(stderr);
                loop {
                    let mut fragment = Vec::new();
                    match (&mut reader).take(4096).read_until(b'\n', &mut fragment) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let mut lines = stderr_store.lock().unwrap();
                            if lines.len() == 256 {
                                lines.remove(0);
                            }
                            lines.push(String::from_utf8_lossy(&fragment).into_owned());
                        }
                    }
                }
            })
            .map_err(|e| TestFailureKind::HarnessFailure(e.to_string()))?;

        let mut harness = Self {
            child,
            stdin,
            events,
            event_buffer: VecDeque::new(),
            context,
            identity,
            next_request: 0,
            diagnostics,
            stderr_lines,
            crashed: Arc::new(AtomicBool::new(false)),
            reader_failure,
        };

        // Initialize worker
        harness.send(Command::Init {
            frame_fd: None,
            broker_fds: Vec::new(),
            identity: identity.map(Box::new),
            version: protocol::VERSION,
            url: "about:blank".into(),
            offline,
            blocked_hosts: String::new(),
            allowed_hosts: String::new(),
            storage_dir: None,
            broker_address: None,
            broker_token: None,
            frame_path: Some(frame_path.to_str().unwrap().into()),
        })?;

        // Wait for ready event and initial document commit
        let ready = harness.wait_for(
            |event| matches!(event, Event::Ready { .. }),
            Duration::from_secs(15),
        )?;
        if !matches!(
            ready,
            Event::Ready {
                version: protocol::VERSION
            }
        ) {
            return Err(TestFailureKind::HarnessFailure(
                "Incompatible worker protocol".into(),
            ));
        }
        harness.wait_for(
            |event| matches!(event, Event::DocumentCommitted { .. }),
            Duration::from_secs(15),
        )?;

        Ok(harness)
    }

    pub fn current_context(&self) -> DocumentContext {
        *self.context.lock().unwrap()
    }

    pub fn send(&mut self, command: Command) -> Result<(), TestFailureKind> {
        self.check_alive()?;
        let command = if !matches!(command, Command::Init { .. })
            && let Some(identity) = self.identity
        {
            identity.command(command)
        } else {
            command
        };
        protocol::write(&mut self.stdin, &command)
            .map_err(|e| TestFailureKind::HarnessFailure(format!("Write failed: {e}")))
    }

    pub fn next_request_context(&mut self) -> RequestContext {
        self.next_request = self
            .next_request
            .checked_add(1)
            .expect("Harness request identity exhausted");
        let doc = self.current_context();
        RequestContext {
            document: doc.document,
            revision: doc.revision,
            request: RequestId(self.next_request),
        }
    }

    pub fn check_alive(&mut self) -> Result<(), TestFailureKind> {
        if self.crashed.load(Ordering::Relaxed) {
            return Err(TestFailureKind::WorkerCrash { code: None });
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                self.crashed.store(true, Ordering::Relaxed);
                Err(TestFailureKind::WorkerCrash {
                    code: status.code(),
                })
            }
            Ok(None) => {
                if let Some(error) = self.reader_failure.lock().unwrap().clone() {
                    Err(error)
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(TestFailureKind::HarnessFailure(e.to_string())),
        }
    }

    pub fn wait_for<F>(&mut self, predicate: F, timeout: Duration) -> Result<Event, TestFailureKind>
    where
        F: Fn(&Event) -> bool,
    {
        // Check buffered events first
        if let Some(pos) = self.event_buffer.iter().position(&predicate) {
            return Ok(self.event_buffer.remove(pos).unwrap());
        }

        let start = Instant::now();
        loop {
            self.check_alive()?;
            while let Ok(event) = self.events.try_recv() {
                if predicate(&event) {
                    return Ok(event);
                }
                if self.event_buffer.len() == 256 {
                    return Err(TestFailureKind::HarnessFailure(
                        "Worker event buffer exceeded its budget".into(),
                    ));
                }
                self.event_buffer.push_back(event);
            }
            if start.elapsed() > timeout {
                return Err(TestFailureKind::Timeout(timeout));
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn load(
        &mut self,
        url: &str,
        timeout: Duration,
    ) -> Result<DocumentContext, TestFailureKind> {
        let serial = self
            .current_context()
            .document
            .0
            .checked_add(1)
            .ok_or_else(|| {
                TestFailureKind::HarnessFailure("Navigation identity exhausted".into())
            })?;
        self.send(Command::Load {
            url: url.into(),
            serial,
        })?;

        // Wait for committed and ready state
        self.wait_for(
            |event| matches!(event, Event::DocumentCommitted { serial: s, .. } if *s == serial),
            timeout,
        )?;
        self.wait_for(
            |event| matches!(event, Event::State { serial: s, loading: false, .. } if *s == serial),
            timeout,
        )?;

        // Assert no compatibility fallback was requested
        let events: Vec<_> = self.drain_events();
        for ev in &events {
            if let Event::Content {
                event: protocol::ContentEvent::Compatibility(reason),
                ..
            } = ev
            {
                return Err(TestFailureKind::EngineError(format!(
                    "Unexpected engine fallback requested: {reason}"
                )));
            }
        }

        Ok(self.current_context())
    }

    pub fn evaluate(&mut self, source: &str, timeout: Duration) -> Result<String, TestFailureKind> {
        let context = self.next_request_context();
        self.send(Command::EvaluateDocument {
            context,
            source: source.into(),
        })?;

        let event = self.wait_for(
            |event| {
                matches!(
                    event,
                    Event::DocumentEvaluated { context: c, .. } if *c == context
                ) || matches!(
                    event,
                    Event::RequestFailed { context: c, .. } if *c == context
                )
            },
            timeout,
        )?;

        match event {
            Event::DocumentEvaluated {
                result: Ok(val), ..
            } => Ok(val),
            Event::DocumentEvaluated {
                result: Err(err), ..
            } => Err(TestFailureKind::EngineError(err.message)),
            Event::RequestFailed { error, .. } => Err(TestFailureKind::EngineError(error.message)),
            _ => unreachable!(),
        }
    }

    pub fn capture(&mut self, timeout: Duration) -> Result<(u32, u32, String), TestFailureKind> {
        let context = self.next_request_context();
        self.send(Command::CaptureDocument { context })?;

        let event = self.wait_for(
            |event| {
                matches!(
                    event,
                    Event::DocumentCaptured { context: c, .. } if *c == context
                ) || matches!(
                    event,
                    Event::RequestFailed { context: c, .. } if *c == context
                )
            },
            timeout,
        )?;

        match event {
            Event::DocumentCaptured {
                width,
                height,
                hash,
                ..
            } => Ok((width, height, hash)),
            Event::RequestFailed { error, .. } => Err(TestFailureKind::EngineError(error.message)),
            _ => unreachable!(),
        }
    }

    pub fn navigate(&mut self, url: &str, timeout: Duration) -> Result<(), TestFailureKind> {
        let context = self.next_request_context();
        self.send(Command::NavigateDocument {
            context,
            url: url.into(),
        })?;

        let event = self.wait_for(
            |event| {
                matches!(
                    event,
                    Event::DocumentNavigated { context: c, .. } if *c == context
                ) || matches!(
                    event,
                    Event::RequestFailed { context: c, .. } if *c == context
                )
            },
            timeout,
        )?;

        match event {
            Event::DocumentNavigated { status: Ok(()), .. } => Ok(()),
            Event::DocumentNavigated {
                status: Err(err), ..
            } => Err(TestFailureKind::EngineError(err.message)),
            Event::RequestFailed { error, .. } => Err(TestFailureKind::EngineError(error.message)),
            _ => unreachable!(),
        }
    }

    pub fn click(&mut self, x: f32, y: f32) -> Result<(), TestFailureKind> {
        self.send(Command::Click { x, y })
    }

    pub fn assert_text(
        &mut self,
        selector: &str,
        expected: &str,
        timeout: Duration,
    ) -> Result<(), TestFailureKind> {
        let start = Instant::now();
        let query = format!(
            "document.querySelector({})?.textContent?.trim()",
            serde_json::to_string(selector).unwrap()
        );
        let mut last_actual = String::new();
        while start.elapsed() < timeout {
            {
                let res = self.evaluate(&query, Duration::from_secs(2))?;
                let text: Option<String> = serde_json::from_str(&res).map_err(|error| {
                    TestFailureKind::HarnessFailure(format!("Invalid text result: {error}"))
                })?;
                if text.as_deref() == Some(expected) {
                    return Ok(());
                }
                last_actual = text.unwrap_or_else(|| "<missing element>".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err(TestFailureKind::AssertionFailure(format!(
            "Selector '{selector}' expected '{expected}', found '{last_actual}'"
        )))
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        let mut list: Vec<_> = self.event_buffer.drain(..).collect();
        while let Ok(ev) = self.events.try_recv() {
            list.push(ev);
        }
        list
    }

    pub fn diagnostics(&self) -> Vec<protocol::DiagnosticRecord> {
        self.diagnostics.lock().unwrap().clone()
    }

    pub fn stderr(&self) -> Vec<String> {
        self.stderr_lines.lock().unwrap().clone()
    }
}

impl Drop for WorkerHarness {
    fn drop(&mut self) {
        // Attempt clean shutdown first
        let _ = self.send(Command::Shutdown);
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        // Force kill if worker failed to exit cleanly
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
