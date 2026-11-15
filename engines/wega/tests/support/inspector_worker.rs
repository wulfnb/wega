//! A real `wega-engine` worker process driven over the real pipe, for the inspector tests
//! and benchmark. The host side uses the browser's own `HostSessions`/`SessionIssuer` types.
#![allow(dead_code)]
use protocol::{
    Command, DocumentContext, DocumentId, DocumentRevision, Event, RequestContext, RequestId,
    identity::{FrameId, ProcessSessionId, SessionIdentity, TabId},
    inspector::{
        INSPECTOR_VERSION, InspectorCommand, InspectorPayload, InspectorRequest, InspectorResponse,
        InspectorSessionId, StatusSnapshot, TargetId, session::SessionIssuer,
    },
};
use std::{
    process::{Child, ChildStdin, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use wega_protocol as protocol;

pub struct Worker {
    pub child: Child,
    pub input: ChildStdin,
    pub events: mpsc::Receiver<Event>,
    pub identity: SessionIdentity,
    pub context: Arc<Mutex<DocumentContext>>,
    pub next_request: u64,
    pub issuer: SessionIssuer,
    pub _store: wega_platform::FrameStore,
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Worker {
    pub fn start() -> Self {
        Self::start_with(7, 7, protocol::VERSION)
    }
    pub fn start_with(tab: u64, process: u8, version: u32) -> Self {
        let store = wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4).unwrap();
        let path = store.path().unwrap().to_string_lossy().into_owned();
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_wega-engine"))
            .arg(wega_platform::launch::DEVELOPMENT_ARGUMENT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        let identity = SessionIdentity {
            tab: TabId(tab),
            frame: FrameId(1),
            process: ProcessSessionId([process; 16]),
            service: None,
        };
        let (tx, events) = mpsc::channel();
        let context = Arc::new(Mutex::new(DocumentContext {
            document: DocumentId(0),
            revision: DocumentRevision(0),
        }));
        let committed = context.clone();
        std::thread::spawn(move || {
            while let Ok(Some(event)) = protocol::read::<Event>(&mut output) {
                let event = identity.receive_event(event).expect("worker event session");
                if let Event::DocumentCommitted {
                    serial, revision, ..
                } = &event
                {
                    *committed.lock().unwrap() = DocumentContext {
                        document: DocumentId(*serial),
                        revision: *revision,
                    };
                }
                if tx.send(event).is_err() {
                    break;
                }
            }
        });
        let mut worker = Self {
            child,
            input,
            events,
            identity,
            context,
            next_request: 0,
            issuer: SessionIssuer::new(),
            _store: store,
        };
        worker.send(Command::Init {
            frame_fd: None,
            broker_fds: Vec::new(),
            identity: Some(Box::new(identity)),
            version,
            url: "about:blank".into(),
            offline: true,
            blocked_hosts: String::new(),
            allowed_hosts: String::new(),
            storage_dir: None,
            broker_address: None,
            broker_token: None,
            frame_path: Some(path),
        });
        worker.until(|e| matches!(e, Event::Ready { version: v } if *v == version));
        worker.until(|e| matches!(e, Event::DocumentCommitted { .. }));
        worker
    }
    pub fn target(&self) -> TargetId {
        TargetId::of(&self.identity)
    }
    pub fn send(&mut self, command: Command) {
        let packet = if matches!(command, Command::Init { .. }) {
            command
        } else {
            self.identity.command(command)
        };
        protocol::write(&mut self.input, &packet).unwrap();
    }
    pub fn document(&self) -> DocumentContext {
        *self.context.lock().unwrap()
    }
    pub fn context(&mut self) -> RequestContext {
        self.next_request += 1;
        let owner = self.document();
        RequestContext {
            document: owner.document,
            revision: owner.revision,
            request: RequestId(self.next_request),
        }
    }
    pub fn until(&self, test: impl Fn(&Event) -> bool) -> Event {
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            let event = self
                .events
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .expect("worker event deadline");
            if test(&event) {
                return event;
            }
        }
    }
    pub fn request_in(
        &mut self,
        context: RequestContext,
        session: InspectorSessionId,
        command: InspectorCommand,
    ) -> RequestContext {
        let request = InspectorRequest {
            version: INSPECTOR_VERSION,
            session,
            target: self.target(),
            command,
        };
        self.send(Command::Inspector {
            context,
            request: InspectorPayload::new(&request),
        });
        context
    }
    /// Send and wait for the matching reply (response or failure).
    pub fn call(
        &mut self,
        session: InspectorSessionId,
        command: InspectorCommand,
    ) -> Result<InspectorResponse, String> {
        let context = self.context();
        self.request_in(context, session, command);
        self.reply(context)
    }
    pub fn reply(&mut self, context: RequestContext) -> Result<InspectorResponse, String> {
        match self.until(|e| {
            matches!(e, Event::InspectorResponse { context: c, .. } | Event::RequestFailed { context: c, .. } if *c == context)
        }) {
            Event::InspectorResponse { result, session, .. } => {
                let _ = session;
                result.map_err(|e| format!("{:?}", e.code))
            }
            Event::RequestFailed { error, .. } => Err(format!("failed:{:?}", error.code)),
            _ => unreachable!(),
        }
    }
    pub fn attach(&mut self) -> InspectorSessionId {
        let session = self.issuer.next().unwrap();
        assert_eq!(
            self.call(session, InspectorCommand::Attach),
            Ok(InspectorResponse::Attached)
        );
        session
    }
    pub fn snapshot(&mut self, session: InspectorSessionId) -> StatusSnapshot {
        match self.call(session, InspectorCommand::GetStatusSnapshot) {
            Ok(InspectorResponse::Status(snapshot)) => *snapshot,
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }
    pub fn evaluate(&mut self, source: &str) -> Result<String, String> {
        let context = self.context();
        self.send(Command::EvaluateDocument {
            context,
            source: source.into(),
        });
        match self.until(|e| {
            matches!(e, Event::DocumentEvaluated { context: c, .. } | Event::RequestFailed { context: c, .. } if *c == context)
        }) {
            Event::DocumentEvaluated { result, .. } => result.map_err(|e| e.message),
            Event::RequestFailed { error, .. } => Err(format!("{:?}", error.code)),
            _ => unreachable!(),
        }
    }
    pub fn navigate(&mut self, serial: u64, url: &str) -> DocumentContext {
        self.send(Command::Load {
            url: url.into(),
            serial,
            bypass_cache: false,
        });
        match self
            .until(|e| matches!(e, Event::DocumentCommitted { serial: s, .. } if *s == serial))
        {
            Event::DocumentCommitted {
                serial, revision, ..
            } => DocumentContext {
                document: DocumentId(serial),
                revision,
            },
            _ => unreachable!(),
        }
    }
    pub fn alive(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }
}
