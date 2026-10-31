//! Real subprocess protocol: two frame credits, exact acknowledgements and freeze.
use protocol::{Command, Event};
use std::{
    process::{Child, ChildStdin, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use wega_protocol as protocol;
struct Worker {
    child: Child,
    input: ChildStdin,
    events: mpsc::Receiver<Event>,
}

#[test]
fn scoped_requests_cannot_read_or_execute_in_a_replacement_document() {
    use protocol::{DocumentId, ErrorCode, RequestContext, RequestId};
    let store = wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4).unwrap();
    let mut worker = Worker::start(store.path().unwrap());
    worker.send(Command::Capabilities);
    match worker.until(|event| matches!(event, Event::Capabilities { .. })) {
        Event::Capabilities { version, supported } => {
            assert_eq!(version, protocol::VERSION);
            assert!(supported.contains(&protocol::Capability::ScopedPageState));
        }
        _ => unreachable!(),
    }
    let old = RequestContext {
        document: DocumentId(0),
        request: RequestId(1),
    };
    worker.send(Command::Load {
        url: "about:blank".into(),
        serial: 1,
    });
    worker.until(|event| {
        matches!(
            event,
            Event::State {
                serial: 1,
                loading: false,
                ..
            }
        )
    });
    worker.send(Command::EvaluateDocument {
        context: old,
        source: "globalThis.staleMutation=true".into(),
    });
    match worker.until(|event| matches!(event, Event::RequestFailed { .. })) {
        Event::RequestFailed { context, error } => {
            assert_eq!(context, old);
            assert_eq!(error.code, ErrorCode::StaleDocument);
        }
        _ => unreachable!(),
    }
    worker.send(Command::CheckDocumentState(old));
    assert!(
        matches!(worker.until(|event| matches!(event, Event::RequestFailed { .. })), Event::RequestFailed { error, .. } if error.code == ErrorCode::StaleDocument)
    );
    let current = RequestContext {
        document: DocumentId(1),
        request: RequestId(2),
    };
    worker.send(Command::EvaluateDocument {
        context: current,
        source: "typeof globalThis.staleMutation".into(),
    });
    assert!(
        matches!(worker.until(|event| matches!(event, Event::DocumentEvaluated { .. })), Event::DocumentEvaluated { context, result: Ok(value) } if context == current && value == "\"undefined\"")
    );
    worker.send(Command::CheckDocumentState(RequestContext {
        request: RequestId(3),
        ..current
    }));
    assert!(
        matches!(worker.until(|event| matches!(event, Event::DocumentState { .. })), Event::DocumentState { context, dirty: false } if context.document == DocumentId(1))
    );
}

#[test]
fn oversized_scoped_evaluation_reports_an_error_and_worker_remains_usable() {
    use protocol::{DocumentId, ErrorCode, RequestContext, RequestId};
    let store = wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4).unwrap();
    let mut worker = Worker::start(store.path().unwrap());
    let context = RequestContext {
        document: DocumentId(0),
        request: RequestId(1),
    };
    worker.send(Command::EvaluateDocument {
        context,
        source: format!("'x'.repeat({})", protocol::MAX_RESULT_BYTES + 1),
    });
    assert!(
        matches!(worker.until(|event| matches!(event, Event::DocumentEvaluated { .. })), Event::DocumentEvaluated { result: Err(error), .. } if error.code == ErrorCode::ResourceLimit)
    );
    worker.send(Command::EvaluateDocument {
        context: RequestContext {
            request: RequestId(2),
            ..context
        },
        source: "6 * 7".into(),
    });
    assert!(
        matches!(worker.until(|event| matches!(event, Event::DocumentEvaluated { .. })), Event::DocumentEvaluated { result: Ok(value), .. } if value == "42")
    );
}
#[test]
fn page_state_requests_are_identified_and_reset_on_navigation() {
    let store = wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4).unwrap();
    let mut worker = Worker::start(store.path().unwrap());
    worker.send(Command::CheckPageState(11));
    assert!(matches!(
        worker.until(|e| matches!(e, Event::PageState { .. })),
        Event::PageState {
            request: 11,
            dirty: false
        }
    ));
    worker.send(Command::Evaluate("var field=document.createElement('textarea');document.body.append(field);field.focus();field.value='draft';field.dispatchEvent(new Event('input',{bubbles:true}));true".into()));
    worker.until(|e| matches!(e, Event::Evaluated { .. }));
    worker.send(Command::CheckPageState(12));
    assert!(matches!(
        worker.until(|e| matches!(e, Event::PageState { .. })),
        Event::PageState {
            request: 12,
            dirty: true
        }
    ));
    worker.send(Command::Load {
        url: "about:blank".into(),
        serial: 1,
    });
    worker.send(Command::CheckPageState(13));
    assert!(matches!(
        worker.until(|e| matches!(e, Event::PageState { .. })),
        Event::PageState {
            request: 13,
            dirty: false
        }
    ));
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Worker {
    fn start(path: &std::path::Path) -> Self {
        Self::start_with_stderr(path, Stdio::inherit())
    }
    fn start_with_stderr(path: &std::path::Path, stderr: Stdio) -> Self {
        Self::start_version(path, stderr, protocol::VERSION)
    }
    fn start_version(path: &std::path::Path, stderr: Stdio, version: u32) -> Self {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_wega-engine"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(Some(event)) = protocol::read::<Event>(&mut output) {
                if tx.send(event).is_err() {
                    break;
                }
            }
        });
        let mut worker = Self {
            child,
            input,
            events,
        };
        worker.send(Command::Init {
            version,
            url: "about:blank".into(),
            offline: true,
            blocked_hosts: String::new(),
            allowed_hosts: String::new(),
            storage_dir: None,
            broker_address: None,
            broker_token: None,
            frame_path: Some(path.to_string_lossy().into_owned()),
        });
        worker.until(|event| matches!(event,Event::Ready{version:actual} if *actual==version));
        worker
    }
    fn send(&mut self, command: Command) {
        protocol::write(&mut self.input, &command).unwrap();
    }
    fn until(&self, test: impl Fn(&Event) -> bool) -> Event {
        let until = Instant::now() + Duration::from_secs(10);
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
    fn frame(&self) -> (u8, u64) {
        match self.until(|event| matches!(event, Event::SharedFrame { .. })) {
            Event::SharedFrame {
                slot,
                sequence,
                width,
                height,
                ..
            } => {
                protocol::frame_bytes(width, height).unwrap();
                (slot, sequence)
            }
            _ => unreachable!(),
        }
    }
    fn no_frame(&self) {
        let until = Instant::now() + Duration::from_millis(150);
        while let Ok(event) = self
            .events
            .recv_timeout(until.saturating_duration_since(Instant::now()))
        {
            assert!(
                !matches!(event, Event::SharedFrame { .. }),
                "unexpected frame: {event:?}"
            );
            if Instant::now() >= until {
                break;
            }
        }
    }
}
#[test]
fn running_v4_browsers_can_still_start_and_use_an_updated_worker() {
    let store = wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4).unwrap();
    let mut worker = Worker::start_version(store.path().unwrap(), Stdio::inherit(), 4);
    let _ = worker.frame();
    worker.send(Command::Evaluate(
        "document.title='legacy client';document.title".into(),
    ));
    assert!(
        matches!(worker.until(|event|matches!(event,Event::Evaluated{..})),Event::Evaluated{value,error:None} if value=="\"legacy client\"")
    );
}
#[test]
fn error_bursts_reach_stderr_before_the_status_queue_is_capped() {
    use std::io::{Read, Seek, SeekFrom};
    let store = wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4).unwrap();
    let mut log = tempfile::tempfile().unwrap();
    let mut worker =
        Worker::start_with_stderr(store.path().unwrap(), Stdio::from(log.try_clone().unwrap()));
    worker.send(Command::Evaluate(
        "for(let i=0;i<40;i++)__wega_report(new Error('burst:'+i));for(let i=0;i<300;i++)Promise.reject('unhandled:'+i);true".into(),
    ));
    let event = worker.until(|e| matches!(e, Event::Evaluated { .. }));
    assert!(matches!(event, Event::Evaluated { error: None, .. }));
    worker.send(Command::Evaluate("true".into()));
    worker.until(|e| matches!(e, Event::Evaluated { .. }));
    log.seek(SeekFrom::Start(0)).unwrap();
    let mut text = String::new();
    log.read_to_string(&mut text).unwrap();
    let reports: Vec<wega_engine::diagnostics::Diagnostic> = text
        .lines()
        .filter_map(|line| line.strip_prefix(wega_engine::diagnostics::PREFIX))
        .map(|json| serde_json::from_str(json).unwrap())
        .collect();
    for i in 0..40 {
        assert!(
            reports
                .iter()
                .any(|r| r.message.contains(&format!("Error: burst:{i}\n"))),
            "missing error {i}"
        );
    }
    for i in 0..300 {
        assert!(
            reports
                .iter()
                .any(|r| r.message == format!("unhandled:{i}\n")),
            "missing rejection {i}; reports={} sample={:?}",
            reports.len(),
            reports.iter().rev().take(3).collect::<Vec<_>>()
        );
    }
}
#[test]
fn shared_frames_wait_for_exact_credits_and_resize_cannot_unfreeze_a_tab() {
    let store = wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4).unwrap();
    let mut worker = Worker::start(store.path().unwrap());
    let first = worker.frame();
    worker.send(Command::Resize {
        width: 64,
        height: 64,
        scale: 1.0,
        zoom: 1.0,
        dark: false,
    });
    let second = worker.frame();
    assert_ne!(first.0, second.0);
    worker.send(Command::Redraw);
    worker.no_frame();
    worker.send(Command::FrameConsumed {
        slot: first.0,
        sequence: first.1.wrapping_add(1),
    });
    worker.no_frame();
    worker.send(Command::FrameConsumed {
        slot: first.0,
        sequence: first.1,
    });
    let third = worker.frame();
    assert_eq!(third.0, first.0);
    assert_ne!(third.1, first.1);
    worker.send(Command::Freeze(true));
    worker.send(Command::FrameConsumed {
        slot: third.0,
        sequence: third.1,
    });
    worker.send(Command::FrameConsumed {
        slot: second.0,
        sequence: second.1,
    });
    worker.send(Command::Resize {
        width: 80,
        height: 64,
        scale: 1.0,
        zoom: 1.0,
        dark: false,
    });
    worker.send(Command::Redraw);
    worker.no_frame();
    worker.send(Command::Freeze(false));
    worker.frame();
    worker.send(Command::Load {
        url: "about:blank".into(),
        serial: 9,
    });
    worker
        .until(|event| matches!(event,Event::State{serial:9,rss_bytes:Some(bytes),..} if *bytes>0));
}

mod support;
#[test]
fn css_html_and_javascript_errors_are_structured_with_urls_before_ui_queue_limits() {
    use std::io::{Read, Seek, SeekFrom};
    use support::{Reply, Server};
    let server = Server::new(|request| {
        if request.path == "/bad.css" {
            Reply::ok(
                "text/css",
                b"@import '/nested.css';p { color: invalid-color; no-such-property: 42; }".to_vec(),
            )
        } else if request.path == "/nested.css" {
            Reply::ok("text/css", b"p{import-no-property:1}".to_vec())
        } else {
            Reply::html(
                "<!doctype html><title>diagnostic fixture</title><link rel=stylesheet href=/bad.css><style>p{inline-no-property:1}</style><p style='attribute-no-property:1'></div><script>document.body.innerHTML += '<p></div>';console.warn('post-warning');console.error('post-console');throw Error('post-js')</script>",
            )
        }
    });
    let store = wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4).unwrap();
    let mut log = tempfile::tempfile().unwrap();
    let mut worker =
        Worker::start_with_stderr(store.path().unwrap(), Stdio::from(log.try_clone().unwrap()));
    worker.send(Command::Policy {
        offline: false,
        blocked_hosts: String::new(),
        allowed_hosts: "127.0.0.1".into(),
    });
    worker.send(Command::Load {
        url: server.url("127.0.0.1", "/"),
        serial: 1,
    });
    worker.until(|e| matches!(e,Event::Notice(s) if s.contains("post-js")));
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
    log.seek(SeekFrom::Start(0)).unwrap();
    let mut text = String::new();
    log.read_to_string(&mut text).unwrap();
    let reports: Vec<wega_engine::diagnostics::Diagnostic> = text
        .lines()
        .filter_map(|line| line.strip_prefix(wega_engine::diagnostics::PREFIX))
        .map(|json| serde_json::from_str(json).unwrap())
        .collect();
    for category in [
        wega_engine::diagnostics::Category::Css,
        wega_engine::diagnostics::Category::Html,
        wega_engine::diagnostics::Category::Javascript,
    ] {
        assert!(
            reports.iter().any(|r| r.category == category
                && r.url
                    .as_ref()
                    .is_some_and(|u| u.starts_with("http://127.0.0.1:"))),
            "missing {category:?}: {reports:?}"
        );
    }
    assert!(
        reports
            .iter()
            .any(|r| r.category == wega_engine::diagnostics::Category::Css
                && r.url.as_ref().unwrap().ends_with("/bad.css")
                && r.line == Some(1))
    );
    for marker in [
        "import-no-property",
        "inline-no-property",
        "attribute-no-property",
    ] {
        assert!(
            reports
                .iter()
                .any(|r| r.category == wega_engine::diagnostics::Category::Css
                    && r.message.contains(marker)),
            "missing CSS diagnostic for {marker}: {reports:?}"
        );
    }
    assert!(
        reports
            .iter()
            .any(|r| r.message.contains("post-warning") && r.severity == "warning")
    );
    assert!(
        reports
            .iter()
            .any(|r| r.message.contains("post-console") && r.severity == "error")
    );
}
