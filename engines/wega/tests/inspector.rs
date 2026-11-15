//! The inspector over the real pipe, against a real `wega-engine` worker process.
//! The host side uses the same `HostSessions`/`DocumentRequests` types the browser uses,
//! so these tests exercise the actual authority and correlation code, not a mock.
use protocol::{
    Command, DocumentContext, DocumentId, DocumentRevision, ErrorCode, Event, RequestContext,
    RequestId,
    identity::{FrameId, ProcessSessionId, SessionIdentity, TabId},
    inspector::{
        INSPECTOR_VERSION, InspectorCommand, InspectorErrorCode, InspectorEvent, InspectorPayload,
        InspectorRequest, InspectorResponse, InspectorSessionId, StatusSnapshot, TargetId,
        session::{HostSessions, SessionIssuer},
    },
};
use std::{
    process::{Child, ChildStdin, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use wega_protocol as protocol;

struct Worker {
    child: Child,
    input: ChildStdin,
    events: mpsc::Receiver<Event>,
    identity: SessionIdentity,
    context: Arc<Mutex<DocumentContext>>,
    next_request: u64,
    issuer: SessionIssuer,
    _store: wega_platform::FrameStore,
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Worker {
    fn start() -> Self {
        Self::start_with(7, 7, protocol::VERSION)
    }
    fn start_with(tab: u64, process: u8, version: u32) -> Self {
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
    fn target(&self) -> TargetId {
        TargetId::of(&self.identity)
    }
    fn send(&mut self, command: Command) {
        let packet = if matches!(command, Command::Init { .. }) {
            command
        } else {
            self.identity.command(command)
        };
        protocol::write(&mut self.input, &packet).unwrap();
    }
    fn document(&self) -> DocumentContext {
        *self.context.lock().unwrap()
    }
    fn context(&mut self) -> RequestContext {
        self.next_request += 1;
        let owner = self.document();
        RequestContext {
            document: owner.document,
            revision: owner.revision,
            request: RequestId(self.next_request),
        }
    }
    fn until(&self, test: impl Fn(&Event) -> bool) -> Event {
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
    fn request_in(
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
    fn call(
        &mut self,
        session: InspectorSessionId,
        command: InspectorCommand,
    ) -> Result<InspectorResponse, String> {
        let context = self.context();
        self.request_in(context, session, command);
        self.reply(context)
    }
    fn reply(&mut self, context: RequestContext) -> Result<InspectorResponse, String> {
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
    fn attach(&mut self) -> InspectorSessionId {
        let session = self.issuer.next().unwrap();
        assert_eq!(
            self.call(session, InspectorCommand::Attach),
            Ok(InspectorResponse::Attached)
        );
        session
    }
    fn snapshot(&mut self, session: InspectorSessionId) -> StatusSnapshot {
        match self.call(session, InspectorCommand::GetStatusSnapshot) {
            Ok(InspectorResponse::Status(snapshot)) => *snapshot,
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }
    fn evaluate(&mut self, source: &str) -> Result<String, String> {
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
    fn navigate(&mut self, serial: u64, url: &str) -> DocumentContext {
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
    fn alive(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }
}

#[test]
fn attach_snapshot_and_detach_over_the_real_pipe() {
    let mut worker = Worker::start();
    let session = worker.attach();
    assert_eq!(
        worker.evaluate("document.title='Inspected';true"),
        Ok("true".into())
    );
    worker.send(Command::Resize {
        width: 800,
        height: 600,
        scale: 2.0,
        zoom: 1.0,
        dark: false,
    });
    // The snapshot reflects the committed document; poll briefly for the title to land.
    let deadline = Instant::now() + Duration::from_secs(10);
    let snapshot = loop {
        let snapshot = worker.snapshot(session);
        if snapshot.title == "Inspected" && snapshot.viewport.is_some() || Instant::now() > deadline
        {
            break snapshot;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(snapshot.target, worker.target());
    assert_eq!(snapshot.document, worker.document());
    assert_eq!(snapshot.url, "about:blank");
    assert_eq!(snapshot.title, "Inspected");
    assert!(!snapshot.loading);
    let viewport = snapshot.viewport.expect("viewport after resize");
    assert_eq!(
        (viewport.width, viewport.height, viewport.device_pixel_ratio),
        (400.0, 300.0, 2.0)
    );
    assert!(
        snapshot.nodes >= 1,
        "node count is the engine's sampled count"
    );
    assert_eq!(snapshot.protocol_version, protocol::VERSION);
    assert_eq!(snapshot.inspector_version, INSPECTOR_VERSION);
    assert!(snapshot.is_bounded());
    match worker.call(session, InspectorCommand::GetTargetInfo) {
        Ok(InspectorResponse::TargetInfo(info)) => {
            assert_eq!(info.target, worker.target());
            assert_eq!(info.document, worker.document());
            assert!(info.engine.starts_with("wega-engine"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        worker.call(session, InspectorCommand::Detach),
        Ok(InspectorResponse::Detached)
    );
    assert_eq!(
        worker.call(session, InspectorCommand::GetStatusSnapshot),
        Err("StaleSession".into())
    );
    assert!(worker.alive());
}

#[test]
fn the_capability_is_advertised() {
    let mut worker = Worker::start();
    worker.send(Command::Capabilities);
    match worker.until(|e| matches!(e, Event::Capabilities { .. })) {
        Event::Capabilities { supported, .. } => {
            assert!(supported.contains(&protocol::Capability::Inspector))
        }
        _ => unreachable!(),
    }
}

#[test]
fn forged_unknown_and_cross_tab_requests_are_refused_without_effect() {
    let mut worker = Worker::start();
    // Never attached.
    let forged = InspectorSessionId(424_242);
    for command in [
        InspectorCommand::GetStatusSnapshot,
        InspectorCommand::GetTargetInfo,
        InspectorCommand::SubscribeStatus,
        InspectorCommand::Detach,
    ] {
        assert_eq!(
            worker.call(forged, command),
            Err("Unauthorized".into()),
            "{command:?}"
        );
    }
    assert_eq!(
        worker.call(InspectorSessionId(0), InspectorCommand::Attach),
        Err("Unauthorized".into())
    );
    let session = worker.attach();
    // The envelope is valid for this worker, but the request names another tab or process.
    for target in [
        TargetId {
            tab: TabId(8),
            ..worker.target()
        },
        TargetId {
            process: ProcessSessionId([9; 16]),
            ..worker.target()
        },
        TargetId {
            frame: FrameId(2),
            ..worker.target()
        },
    ] {
        let context = worker.context();
        worker.send(Command::Inspector {
            context,
            request: InspectorPayload::new(&InspectorRequest {
                version: INSPECTOR_VERSION,
                session,
                target,
                command: InspectorCommand::GetStatusSnapshot,
            }),
        });
        assert_eq!(worker.reply(context), Err("UnknownTarget".into()));
    }
    // The legitimate session is untouched.
    assert_eq!(worker.snapshot(session).target, worker.target());
    // A foreign envelope kills the worker (existing identity policy), it is never executed.
    let mut wrong = worker.identity;
    wrong.tab = TabId(99);
    protocol::write(
        &mut worker.input,
        &wrong.command(Command::InspectorDetach { session }),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while worker.alive() {
        assert!(Instant::now() < deadline, "foreign envelope was accepted");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_newer_attach_supersedes_and_the_old_session_is_stale_everywhere() {
    let mut worker = Worker::start();
    let first = worker.attach();
    let second = worker.attach();
    for command in [
        InspectorCommand::Attach,
        InspectorCommand::GetStatusSnapshot,
        InspectorCommand::SubscribeStatus,
        InspectorCommand::Detach,
    ] {
        assert_eq!(
            worker.call(first, command),
            Err("StaleSession".into()),
            "{command:?}"
        );
    }
    worker.snapshot(second);
    // A stale unscoped detach changes nothing; the live session's detach is idempotent.
    worker.send(Command::InspectorDetach { session: first });
    worker.snapshot(second);
    worker.send(Command::InspectorDetach { session: second });
    worker.send(Command::InspectorDetach { session: second });
    assert_eq!(
        worker.call(second, InspectorCommand::GetStatusSnapshot),
        Err("StaleSession".into())
    );
    // Re-attach with the same id is idempotent only while it is live; a replay is refused.
    assert_eq!(
        worker.call(second, InspectorCommand::Attach),
        Err("StaleSession".into())
    );
    let third = worker.attach();
    worker.snapshot(third);
}

#[test]
fn attach_is_idempotent_for_a_retry_of_the_same_session() {
    let mut worker = Worker::start();
    let session = worker.attach();
    assert_eq!(
        worker.call(session, InspectorCommand::Attach),
        Ok(InspectorResponse::Attached)
    );
    worker.snapshot(session);
}

#[test]
fn stale_document_requests_are_rejected_and_never_answered_for_the_new_page() {
    let mut worker = Worker::start();
    let session = worker.attach();
    let old = worker.context();
    let stamp = worker.navigate(1, "about:blank");
    assert_ne!(stamp.document, old.document);
    worker.request_in(old, session, InspectorCommand::GetStatusSnapshot);
    assert_eq!(worker.reply(old), Err("failed:StaleDocument".into()));
    // The same session keeps working in the new document, and reports the new stamp.
    worker.until(|e| {
        matches!(
            e,
            Event::State {
                serial: 1,
                loading: false,
                ..
            }
        )
    });
    let snapshot = worker.snapshot(session);
    assert_eq!(snapshot.document, stamp);
    // An old-revision stamp with the right document is stale too.
    let mut skewed = worker.context();
    skewed.revision = DocumentRevision(skewed.revision.0 + 1);
    worker.request_in(skewed, session, InspectorCommand::GetTargetInfo);
    assert_eq!(worker.reply(skewed), Err("failed:StaleDocument".into()));
}

#[test]
fn a_replayed_request_id_is_not_executed_twice() {
    let mut worker = Worker::start();
    let session = worker.attach();
    let context = worker.context();
    worker.request_in(context, session, InspectorCommand::GetTargetInfo);
    assert!(worker.reply(context).is_ok());
    worker.request_in(context, session, InspectorCommand::GetTargetInfo);
    assert_eq!(worker.reply(context), Err("failed:InvalidRequest".into()));
    worker.snapshot(session);
}

#[test]
fn unsupported_versions_and_malformed_payloads_are_answered_and_the_worker_survives() {
    let mut worker = Worker::start();
    let session = worker.attach();
    for version in [0, INSPECTOR_VERSION + 1, u32::MAX] {
        let context = worker.context();
        worker.send(Command::Inspector {
            context,
            request: InspectorPayload::new(&InspectorRequest {
                version,
                session,
                target: worker.target(),
                command: InspectorCommand::GetStatusSnapshot,
            }),
        });
        assert_eq!(worker.reply(context), Err("UnsupportedVersion".into()));
    }
    let good = serde_json::to_value(InspectorRequest {
        version: INSPECTOR_VERSION,
        session,
        target: worker.target(),
        command: InspectorCommand::Attach,
    })
    .unwrap();
    let mut variants = vec![
        serde_json::Value::Null,
        serde_json::json!("Attach"),
        serde_json::json!([1, 2, 3]),
        serde_json::json!({"command": "Evaluate"}),
    ];
    let mut extra = good.clone();
    extra["expression"] = serde_json::json!("globalThis.pwned=1");
    variants.push(extra);
    let mut evaluate = good.clone();
    evaluate["command"] = serde_json::json!({"Evaluate": "globalThis.pwned=1"});
    variants.push(evaluate);
    for value in variants {
        let context = worker.context();
        worker.send(Command::Inspector {
            context,
            request: InspectorPayload(value.clone()),
        });
        assert_eq!(
            worker.reply(context),
            Err("failed:InvalidRequest".into()),
            "{value}"
        );
    }
    assert!(worker.alive());
    assert_eq!(
        worker.evaluate("typeof globalThis.pwned"),
        Ok("\"undefined\"".into())
    );
    worker.snapshot(session);
}

#[test]
fn status_events_follow_navigation_and_stop_after_unsubscribe_or_detach() {
    let mut worker = Worker::start();
    let session = worker.attach();
    assert_eq!(
        worker.call(session, InspectorCommand::SubscribeStatus),
        Ok(InspectorResponse::Subscribed)
    );
    let stamp = worker.navigate(1, "about:blank");
    let event = worker.until(|e| {
        matches!(
            e,
            Event::InspectorEvent {
                event: InspectorEvent::DocumentChanged { .. },
                ..
            }
        )
    });
    let Event::InspectorEvent {
        context,
        session: s,
        seq,
        event,
    } = event
    else {
        unreachable!()
    };
    assert_eq!(s, session);
    assert_eq!(event, InspectorEvent::DocumentChanged { document: stamp });
    assert_eq!(context, stamp);
    assert!(seq >= 1);
    // Sequence numbers only grow.
    worker.navigate(2, "about:blank");
    let next = worker.until(|e| {
        matches!(
            e,
            Event::InspectorEvent {
                event: InspectorEvent::DocumentChanged { .. },
                ..
            }
        )
    });
    let Event::InspectorEvent { seq: next_seq, .. } = next else {
        unreachable!()
    };
    assert!(next_seq > seq);
    assert_eq!(
        worker.call(session, InspectorCommand::UnsubscribeStatus),
        Ok(InspectorResponse::Unsubscribed)
    );
    while worker.events.try_recv().is_ok() {}
    worker.navigate(3, "about:blank");
    worker.until(|e| {
        matches!(
            e,
            Event::State {
                serial: 3,
                loading: false,
                ..
            }
        )
    });
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        !worker
            .events
            .try_iter()
            .any(|e| matches!(e, Event::InspectorEvent { .. })),
        "events after unsubscribe"
    );
    // Detach also silences a resubscribed session.
    assert_eq!(
        worker.call(session, InspectorCommand::SubscribeStatus),
        Ok(InspectorResponse::Subscribed)
    );
    worker.send(Command::InspectorDetach { session });
    while worker.events.try_recv().is_ok() {}
    worker.navigate(4, "about:blank");
    worker.until(|e| {
        matches!(
            e,
            Event::State {
                serial: 4,
                loading: false,
                ..
            }
        )
    });
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        !worker
            .events
            .try_iter()
            .any(|e| matches!(e, Event::InspectorEvent { .. }))
    );
}

#[test]
fn no_events_are_produced_when_nothing_is_subscribed() {
    let mut worker = Worker::start();
    let _session = worker.attach();
    worker.navigate(1, "about:blank");
    worker.until(|e| {
        matches!(
            e,
            Event::State {
                serial: 1,
                loading: false,
                ..
            }
        )
    });
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        !worker
            .events
            .try_iter()
            .any(|e| matches!(e, Event::InspectorEvent { .. }))
    );
}

#[test]
fn rapid_attach_and_detach_leave_one_consistent_session() {
    let mut worker = Worker::start();
    let mut last = None;
    for round in 0..150 {
        let session = worker.issuer.next().unwrap();
        let context = worker.context();
        worker.request_in(context, session, InspectorCommand::Attach);
        if round % 3 == 0 {
            worker.send(Command::InspectorDetach { session });
        } else {
            last = Some(session);
        }
        if round % 10 == 0 {
            assert!(worker.reply(context).is_ok());
        }
    }
    // Drain remaining replies: every one is either success or a stale/detached refusal.
    std::thread::sleep(Duration::from_millis(200));
    while let Ok(event) = worker.events.try_recv() {
        if let Event::InspectorResponse { result: Err(e), .. } = event {
            assert!(matches!(e.code, InspectorErrorCode::StaleSession), "{e:?}");
        }
    }
    let last = last.unwrap();
    // Everything before the last attach is stale, and the last one still works.
    worker.snapshot(last);
    assert!(worker.alive());
}

#[test]
fn page_javascript_cannot_reach_or_discover_inspector_authority() {
    let mut worker = Worker::start();
    let session = worker.attach();
    assert_eq!(
        worker.call(session, InspectorCommand::SubscribeStatus),
        Ok(InspectorResponse::Subscribed)
    );
    // 1. Nothing in the page's global object or prototype chain names the inspector.
    let names = worker
        .evaluate(
            "(()=>{const seen=new Set();let o=globalThis;while(o){for(const n of Reflect.ownKeys(o))seen.add(String(n));o=Object.getPrototypeOf(o)}\
             return JSON.stringify([...seen].filter(n=>/inspect|devtool|wega_|__wega|session|ipc|stdin|stdout|pipe/i.test(n)))})()",
        )
        .unwrap();
    let hits: Vec<String> =
        serde_json::from_str(&serde_json::from_str::<String>(&names).unwrap()).unwrap();
    // Existing, unrelated internals may match the broad regex; none may mention the inspector.
    assert!(
        hits.iter()
            .all(|name| !name.to_lowercase().contains("inspector")
                && !name.to_lowercase().contains("devtool")),
        "page-visible names: {hits:?}"
    );
    // 2. Script cannot produce protocol traffic or act on a session: try every plausible route.
    let attempts = worker
        .evaluate(
            "(()=>{const r=[];const t=(f)=>{try{f();r.push('ran')}catch(e){r.push('threw')}};\
             t(()=>process.stdout.write('x'));t(()=>Deno.stdout.writeSync(new Uint8Array([1])));\
             t(()=>Deno.core.ops.op_inspector_dispatch('{}'));t(()=>globalThis.__wega_inspector());\
             t(()=>postMessage({Inspector:{}}));t(()=>fetch('wega-inspector://session'));\
             return r.join(',')})()",
        )
        .unwrap();
    assert!(!attempts.is_empty());
    // 3. After all of that, no unsolicited inspector traffic exists and the session is intact.
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        !worker
            .events
            .try_iter()
            .any(|e| matches!(e, Event::InspectorResponse { .. })),
        "page script triggered an inspector response"
    );
    let snapshot = worker.snapshot(session);
    assert_eq!(snapshot.target, worker.target());
    // 4. The page cannot make the engine accept a session: only a Command on stdin can.
    assert_eq!(
        worker.call(
            InspectorSessionId(session.0 + 1),
            InspectorCommand::GetStatusSnapshot
        ),
        Err("Unauthorized".into())
    );
}

#[test]
fn queued_inspector_requests_expire_and_cancel_instead_of_waiting_forever() {
    let mut worker = Worker::start();
    let session = worker.attach();
    // Block the DOM thread (below the 5 s script watchdog), then queue behind it.
    let busy = worker.context();
    worker.send(Command::EvaluateDocument {
        context: busy,
        source: "(()=>{const t=Date.now();while(Date.now()-t<3000){}return 1})()".into(),
    });
    let cancelled = worker.context();
    worker.request_in(cancelled, session, InspectorCommand::GetStatusSnapshot);
    worker.send(Command::CancelRequest(cancelled));
    let expiring = worker.context();
    worker.request_in(expiring, session, InspectorCommand::GetStatusSnapshot);
    let start = Instant::now();
    assert_eq!(worker.reply(cancelled), Err("failed:Cancelled".into()));
    assert_eq!(worker.reply(expiring), Err("failed:Timeout".into()));
    assert!(start.elapsed() < Duration::from_secs(10));
    // The worker recovers and serves fresh requests.
    worker.snapshot(session);
}

#[test]
fn an_older_protocol_session_is_never_offered_the_inspector() {
    let mut worker = Worker::start_with(7, 7, 21);
    let session = worker.issuer.next().unwrap();
    let context = worker.context();
    worker.request_in(context, session, InspectorCommand::Attach);
    assert_eq!(worker.reply(context), Err("failed:Unsupported".into()));
    worker.send(Command::Capabilities);
    match worker.until(|e| matches!(e, Event::Capabilities { .. })) {
        Event::Capabilities { supported, .. } => {
            assert!(!supported.contains(&protocol::Capability::Inspector))
        }
        _ => unreachable!(),
    }
    assert!(worker.alive());
    let _ = ErrorCode::Unsupported;
}

#[test]
fn host_registry_and_a_real_worker_agree_when_a_tab_restarts() {
    // Two processes for the same tab id: a replacement worker has a new process identity,
    // so the old target and session can never address it.
    let mut first = Worker::start_with(5, 1, protocol::VERSION);
    let mut host = HostSessions::new(first.target());
    let (session, _) = host.open(&first.issuer).unwrap();
    assert_eq!(
        first.call(session, InspectorCommand::Attach),
        Ok(InspectorResponse::Attached)
    );
    let mut second = Worker::start_with(5, 2, protocol::VERSION);
    assert_ne!(first.target(), second.target());
    // The old session id is not attached to the replacement.
    let request = InspectorRequest {
        version: INSPECTOR_VERSION,
        session,
        target: first.target(),
        command: InspectorCommand::GetStatusSnapshot,
    };
    assert!(
        host.registry()
            .authorize(request.session, second.target())
            .is_err()
    );
    let context = second.context();
    second.send(Command::Inspector {
        context,
        request: InspectorPayload::new(&request),
    });
    assert_eq!(second.reply(context), Err("UnknownTarget".into()));
    // The browser-side registry dies with the first process.
    host.close();
    assert!(host.registry().authorize(session, first.target()).is_err());
}
