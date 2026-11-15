//! The inspector inside the *required* (sandboxed) content worker, the gap D1 left open.
//! Needs the native content helper and a supported sandbox host; run explicitly:
//!
//!     WEGA_CONTENT_HELPER=<bundled wega-engine> cargo test -p wega-engine \
//!         --test inspector_sandbox -- --ignored --nocapture
//!
//! Nothing here relaxes the sandbox: the worker is launched exactly as in
//! `inherited_broker.rs`, with the same inherited broker and frame grants.
#![cfg(any(unix, windows))]
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use wega_protocol::{
    self as protocol, Command as Request, DocumentContext, DocumentId, Event, RequestContext,
    RequestId,
    identity::{FrameId, ProcessSessionId, SessionIdentity, TabId},
    inspector::{
        EvalOutcome, GrantToken, INSPECTOR_VERSION, InspectorArgs, InspectorCommand,
        InspectorEvent, InspectorPayload, InspectorRequest, InspectorResponse, InspectorSessionId,
        TargetId,
    },
};

#[test]
#[ignore = "requires WEGA_CONTENT_HELPER and a supported native sandbox host"]
fn the_inspector_console_and_evaluation_work_inside_the_sandboxed_required_worker() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let origin = url.trim_end_matches('/').to_string();
    let serving = std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut header = Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        stream.read_exact(&mut byte).unwrap();
                        header.push(byte[0]);
                    }
                    let body = "<script>console.log('logged during sandboxed load', {n: 1})</script><p>sandbox</p>";
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < until, "fixture was never fetched");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("fixture: {e}"),
            }
        }
    });
    let profile = tempfile::tempdir().unwrap();
    let broker = wega_network::Broker::new().unwrap();
    let session = broker
        .open_bound_session(
            profile.path().into(),
            wega_network::Policy {
                allow_loopback: true,
                ..Default::default()
            },
        )
        .unwrap();
    session.begin_navigation(&url.parse().unwrap());
    let channels = session.inherited_channels(4).unwrap();
    let (_, token) = session.endpoint();
    let identity = SessionIdentity {
        tab: TabId(5),
        frame: FrameId(1),
        process: ProcessSessionId([5; 16]),
        service: Some(session.identity()),
    };
    let executable =
        std::env::var_os("WEGA_CONTENT_HELPER").expect("Missing native content helper");
    let mut command = Command::new(executable);
    // Required mode: the sandboxed launch path, not the development opt-in.
    let launch = wega_platform::launch::PreparedLaunch::required_fixture(&mut command).unwrap();
    assert!(
        !launch.is_development(),
        "this test must run the sandboxed worker"
    );
    let descriptors = launch
        .inherit_broker_channels(&mut command, &channels)
        .unwrap();
    let frames = std::sync::Arc::new(
        wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4)
            .unwrap()
            .into_reader(),
    );
    let frame_fd = launch
        .inherit_frame_file(&mut command, &frames.try_clone_file().unwrap())
        .unwrap();
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let child = launch.spawn(&mut command).unwrap();
    drop(command);
    drop(channels);
    let mut child = wega_platform::process::OwnedProcess::new(child, |_| {});
    launch.attach(&mut child, 512 * 1024 * 1024).unwrap();
    child.retain_until_exit(frames.clone());
    if let Some(stderr) = child.take_stderr() {
        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut reader = std::io::BufReader::new(stderr.take(64 * 1024));
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 0) {
                line.clear();
            }
        });
    }
    let stdin = std::cell::RefCell::new(child.take_stdin().unwrap());
    let mut stdout = child.take_stdout().unwrap();
    let (events, receiver) = mpsc::sync_channel(4096);
    std::thread::spawn(move || {
        while let Ok(Some(packet)) = protocol::read::<Event>(&mut stdout) {
            let event = identity.receive_event(packet).unwrap();
            if events.send(event).is_err() {
                break;
            }
        }
    });
    protocol::write(
        &mut *stdin.borrow_mut(),
        &Request::Init {
            identity: Some(Box::new(identity)),
            version: protocol::VERSION,
            url: url.clone(),
            offline: false,
            blocked_hosts: String::new(),
            allowed_hosts: String::new(),
            storage_dir: None,
            broker_address: None,
            broker_token: Some(token),
            broker_fds: descriptors,
            frame_path: None,
            frame_fd: Some(frame_fd),
        },
    )
    .unwrap();
    let seen = std::cell::RefCell::new(Vec::<InspectorEvent>::new());
    let wait = |predicate: &dyn Fn(&Event) -> bool| {
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            let event = receiver
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .expect("Worker transport deadline");
            if let Event::InspectorEvent { event, .. } = &event {
                seen.borrow_mut().push(event.clone());
            }
            if predicate(&event) {
                break event;
            }
        }
    };
    wait(&|e| matches!(e, Event::Ready { .. }));
    let revision = match wait(&|e| matches!(e, Event::DocumentCommitted { .. })) {
        Event::DocumentCommitted { revision, .. } => revision,
        _ => unreachable!(),
    };
    session.take_committed_principal(Some(&url));
    wait(&|e| matches!(e, Event::State { loading: false, .. }));
    let document = DocumentContext {
        document: DocumentId(0),
        revision,
    };
    let mut next = 0;
    let mut call = |command: InspectorCommand, args: InspectorArgs, id: InspectorSessionId| {
        next += 1;
        let context = RequestContext {
            document: document.document,
            revision,
            request: RequestId(next),
        };
        let request = InspectorRequest {
            version: INSPECTOR_VERSION,
            session: id,
            target: TargetId::of(&identity),
            command,
            args,
        };
        protocol::write(
            &mut *stdin.borrow_mut(),
            &identity.command(Request::Inspector {
                context,
                request: InspectorPayload::new(&request),
            }),
        )
        .unwrap();
        match wait(
            &|e| matches!(e, Event::InspectorResponse { context: c, .. } | Event::RequestFailed { context: c, .. } if *c == context),
        ) {
            Event::InspectorResponse { result, .. } => {
                result.map_err(|e| format!("{:?}: {}", e.code, e.message))
            }
            Event::RequestFailed { error, .. } => {
                Err(format!("failed {:?}: {}", error.code, error.message))
            }
            _ => unreachable!(),
        }
    };
    let id = InspectorSessionId(1);
    assert_eq!(
        call(InspectorCommand::Attach, InspectorArgs::None, id),
        Ok(InspectorResponse::Attached)
    );
    // Read-only DOM needs the existing session, but never an evaluation grant.
    let InspectorResponse::Dom(root) =
        call(InspectorCommand::GetDomRoot, InspectorArgs::None, id).unwrap()
    else {
        panic!("DOM root missing")
    };
    assert!(root.is_bounded());
    assert_eq!(root.document, document);
    let root_id = root.nodes[0].id;
    assert!(matches!(
        call(
            InspectorCommand::GetDomChildren,
            InspectorArgs::DomChildren {
                node: root_id,
                epoch: root.epoch,
                offset: 0,
                limit: 64
            },
            id
        ),
        Ok(InspectorResponse::Dom(_))
    ));
    let mut stale = root_id;
    stale.document.revision = protocol::DocumentRevision(u64::MAX);
    assert!(
        call(
            InspectorCommand::DescribeDomNode,
            InspectorArgs::DomNode { node: stale },
            id
        )
        .is_err_and(|e| e.starts_with("StaleDocument"))
    );
    assert!(matches!(
        call(InspectorCommand::EnableConsole, InspectorArgs::None, id),
        Ok(InspectorResponse::ConsoleEnabled { retained, .. }) if retained >= 1
    ));
    // Evaluation is refused until granted, inside the sandbox as anywhere else.
    let eval = |expression: &str, grant: GrantToken| InspectorArgs::Evaluate {
        expression: expression.into(),
        grant,
    };
    let secret = GrantToken([0x77; 16]);
    let refused = call(InspectorCommand::Evaluate, eval("6*7", secret), id);
    assert!(
        refused
            .as_ref()
            .is_err_and(|e| e.starts_with("Unauthorized")),
        "{refused:?}"
    );
    // The browser's claim is checked against the sandboxed engine's own document: a forged
    // origin, a claim for another port, and a missing claim without the user's override are all
    // refused, and refusing stores nothing.
    for (claimed, override_) in [
        (Some("http://127.0.0.1:1".to_string()), false),
        (Some("https://example.test".to_string()), false),
        (None, false),
    ] {
        let refused = call(
            InspectorCommand::GrantEvaluation,
            InspectorArgs::Grant {
                token: secret,
                opaque_override: override_,
                origin: claimed.clone(),
            },
            id,
        );
        assert!(
            refused
                .as_ref()
                .is_err_and(|e| e.starts_with("PolicyDenied")),
            "{claimed:?}: {refused:?}"
        );
    }
    let still_refused = call(InspectorCommand::Evaluate, eval("6*7", secret), id);
    assert!(
        still_refused
            .as_ref()
            .is_err_and(|e| e.starts_with("Unauthorized")),
        "{still_refused:?}"
    );
    // An http origin is a tuple origin: no override is needed.
    assert_eq!(
        call(
            InspectorCommand::GrantEvaluation,
            InspectorArgs::Grant {
                token: secret,
                opaque_override: false,
                origin: Some(origin.clone()),
            },
            id
        ),
        Ok(InspectorResponse::EvaluationGranted)
    );
    let result = call(InspectorCommand::Evaluate, eval("6*7", secret), id);
    assert!(
        matches!(&result, Ok(InspectorResponse::Evaluated(o)) if matches!(&**o, EvalOutcome::Value { preview } if preview.text == "42")),
        "{result:?}"
    );
    let thrown = call(InspectorCommand::Evaluate, eval("null.x", secret), id);
    assert!(
        matches!(&thrown, Ok(InspectorResponse::Evaluated(o)) if matches!(&**o, EvalOutcome::Exception { details } if details.name == "TypeError")),
        "{thrown:?}"
    );
    // Under the sandbox, the page still cannot reach files or processes through evaluation.
    let escape = call(
        InspectorCommand::Evaluate,
        eval(
            "typeof Deno === 'undefined' ? 'no Deno' : typeof Deno.readTextFileSync",
            secret,
        ),
        id,
    );
    assert!(
        matches!(&escape, Ok(InspectorResponse::Evaluated(o)) if matches!(&**o, EvalOutcome::Value { preview } if preview.text == "no Deno")),
        "{escape:?}"
    );
    // The log from the page load arrived, and so does output from an evaluation.
    call(
        InspectorCommand::Evaluate,
        eval("console.log('from sandboxed evaluation')", secret),
        id,
    )
    .expect("an evaluation that logs must succeed");
    std::thread::sleep(Duration::from_millis(300));
    while let Ok(event) = receiver.recv_timeout(Duration::from_millis(200)) {
        if let Event::InspectorEvent { event, .. } = event {
            seen.borrow_mut().push(event);
        }
    }
    let texts: Vec<String> = seen
        .borrow()
        .iter()
        .filter_map(|e| match e {
            InspectorEvent::ConsoleMessageAdded(m) => Some(m.text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        texts
            .iter()
            .any(|t| t.contains("logged during sandboxed load")),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "from sandboxed evaluation"),
        "{texts:?}"
    );
    // Detach revokes: the same grant is dead.
    protocol::write(
        &mut *stdin.borrow_mut(),
        &identity.command(Request::InspectorDetach { session: id }),
    )
    .unwrap();
    let after = call(InspectorCommand::Evaluate, eval("6*7", secret), id);
    assert!(
        after.as_ref().is_err_and(|e| e.starts_with("StaleSession")),
        "{after:?}"
    );
    session.revoke();
    protocol::write(
        &mut *stdin.borrow_mut(),
        &identity.command(Request::Shutdown),
    )
    .unwrap();
    wait(&|e| matches!(e, Event::ShutdownComplete));
    serving.join().unwrap();
}
