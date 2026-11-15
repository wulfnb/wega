use super::*;
use std::cell::{Cell, RefCell};
use wega_protocol::{
    DocumentId, DocumentRevision,
    identity::{FrameId, ProcessSessionId, TabId},
    inspector::{
        ConsoleLevel, ConsoleMessage, ExceptionDetails, ExceptionKind, GrantToken, InspectorArgs,
        InspectorErrorCode, Preview, PreviewKind,
    },
};

fn own() -> TargetId {
    TargetId {
        tab: TabId(7),
        frame: FrameId(1),
        process: ProcessSessionId([3; 16]),
    }
}
fn doc(document: u64, revision: u64) -> DocumentContext {
    DocumentContext {
        document: DocumentId(document),
        revision: DocumentRevision(revision),
    }
}
fn token(byte: u8) -> GrantToken {
    GrantToken([byte; 16])
}

struct Port {
    status_calls: Cell<u32>,
    reads: Cell<u32>,
    evaluations: RefCell<Vec<String>>,
    origin: Option<String>,
    outcome: RefCell<Result<EvalOutcome, String>>,
    messages: RefCell<Vec<ConsoleMessage>>,
    first: Cell<u64>,
    next: Cell<u64>,
    dropped: Cell<u64>,
}
impl Port {
    fn new() -> Self {
        Self {
            status_calls: Cell::new(0),
            reads: Cell::new(0),
            evaluations: RefCell::new(Vec::new()),
            origin: Some("https://example.test".into()),
            outcome: RefCell::new(Ok(EvalOutcome::Value {
                preview: Preview::simple(PreviewKind::Number, "2"),
            })),
            messages: RefCell::new(Vec::new()),
            first: Cell::new(1),
            next: Cell::new(1),
            dropped: Cell::new(0),
        }
    }
    fn log(&self, text: &str, exception: bool) {
        let seq = self.next.get();
        self.next.set(seq + 1);
        self.messages.borrow_mut().push(ConsoleMessage {
            seq,
            timestamp_ms: 1,
            level: if exception {
                ConsoleLevel::Error
            } else {
                ConsoleLevel::Log
            },
            method: "log".into(),
            args: vec![],
            text: text.into(),
            source: None,
            exception: exception.then(|| ExceptionDetails {
                kind: ExceptionKind::Uncaught,
                name: "Error".into(),
                message: text.into(),
                stack: None,
                source: None,
            }),
        });
    }
    /// The ring evicts everything below `first`.
    fn evict_before(&self, first: u64) {
        self.messages.borrow_mut().retain(|m| m.seq >= first);
        self.dropped
            .set(self.dropped.get() + (first - self.first.get()));
        self.first.set(first);
    }
}
impl PagePort for Port {
    fn status_input(&self, sampled: Sampled) -> StatusInput {
        self.status_calls.set(self.status_calls.get() + 1);
        StatusInput {
            url: "https://example.test/".into(),
            title: "Example".into(),
            loading: false,
            viewport: sampled.viewport,
            nodes: sampled.nodes,
            js_heap_bytes: sampled.js_heap_bytes,
            can_go_back: false,
            can_go_forward: true,
            mutation_epoch: 7,
            layout_pending: false,
        }
    }
    fn console_first_seq(&self) -> u64 {
        self.first.get()
    }
    fn console_next_seq(&self) -> u64 {
        self.next.get()
    }
    fn console_len(&self) -> usize {
        self.messages.borrow().len()
    }
    fn console_dropped(&self) -> u64 {
        self.dropped.get()
    }
    fn console_read(
        &self,
        after: u64,
        max_entries: usize,
        _max_bytes: usize,
    ) -> Vec<ConsoleMessage> {
        self.reads.set(self.reads.get() + 1);
        self.messages
            .borrow()
            .iter()
            .filter(|m| m.seq > after)
            .take(max_entries)
            .cloned()
            .collect()
    }
    fn console_clear(&mut self) {
        self.messages.borrow_mut().clear();
        self.first.set(self.next.get());
    }
    fn document_origin(&self) -> Option<String> {
        self.origin.clone()
    }
    fn evaluate(&mut self, expression: &str) -> Result<EvalOutcome, String> {
        self.evaluations.borrow_mut().push(expression.into());
        self.outcome.borrow().clone()
    }
}

fn request(session: u64, command: InspectorCommand, args: InspectorArgs) -> InspectorRequest {
    InspectorRequest {
        version: INSPECTOR_VERSION,
        session: InspectorSessionId(session),
        target: own(),
        command,
        args,
    }
}
fn run(
    backend: &mut Backend,
    port: &mut Port,
    session: u64,
    command: InspectorCommand,
    args: InspectorArgs,
    current: (DocumentContext, bool),
) -> Result<InspectorResponse, InspectorError> {
    backend.handle(
        &request(session, command, args),
        current,
        Sampled::default(),
        port,
    )
}
fn simple(
    backend: &mut Backend,
    port: &mut Port,
    session: u64,
    command: InspectorCommand,
) -> Result<InspectorResponse, InspectorError> {
    run(
        backend,
        port,
        session,
        command,
        InspectorArgs::None,
        (doc(0, 0), false),
    )
}
fn code<T>(result: Result<T, InspectorError>) -> InspectorErrorCode {
    result.err().expect("expected a refusal").code
}
fn evaluate(_session: u64, grant: GrantToken) -> (InspectorCommand, InspectorArgs) {
    (
        InspectorCommand::Evaluate,
        InspectorArgs::Evaluate {
            expression: "1+1".into(),
            grant,
        },
    )
}
fn granted(backend: &mut Backend, port: &mut Port, at: DocumentContext) {
    simple(backend, port, 1, InspectorCommand::Attach).unwrap();
    run(
        backend,
        port,
        1,
        InspectorCommand::GrantEvaluation,
        InspectorArgs::Grant {
            token: token(9),
            opaque_override: false,
            origin: Some("https://example.test".into()),
        },
        (at, false),
    )
    .unwrap();
}

#[test]
fn snapshot_reports_exactly_the_supplied_facts_and_identity() {
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    simple(&mut backend, &mut port, 1, InspectorCommand::Attach).unwrap();
    let sampled = Sampled {
        nodes: 12,
        js_heap_bytes: 4096,
        viewport: Some((800.0, 600.0, 2.0)),
    };
    let InspectorResponse::Status(snapshot) = backend
        .handle(
            &request(1, InspectorCommand::GetStatusSnapshot, InspectorArgs::None),
            (doc(2, 3), false),
            sampled,
            &mut port,
        )
        .unwrap()
    else {
        panic!("not a snapshot")
    };
    assert_eq!(snapshot.target, own());
    assert_eq!(snapshot.document, doc(2, 3));
    assert_eq!((snapshot.nodes, snapshot.js_heap_bytes), (12, 4096));
    assert!(snapshot.can_go_forward && !snapshot.can_go_back);
    assert_eq!(snapshot.protocol_version, 23);
    assert_eq!(snapshot.inspector_version, INSPECTOR_VERSION);
    assert!(snapshot.engine.starts_with("wega-engine "));
}

#[test]
fn the_page_is_not_consulted_unless_a_snapshot_is_requested() {
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    for command in [
        InspectorCommand::Attach,
        InspectorCommand::GetTargetInfo,
        InspectorCommand::SubscribeStatus,
        InspectorCommand::UnsubscribeStatus,
    ] {
        simple(&mut backend, &mut port, 1, command).unwrap();
    }
    assert_eq!(port.status_calls.get(), 0);
    assert_eq!(port.reads.get(), 0);
}

#[test]
fn requests_before_attach_after_detach_or_for_other_targets_are_refused() {
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    assert_eq!(
        code(simple(
            &mut backend,
            &mut port,
            1,
            InspectorCommand::GetStatusSnapshot
        )),
        InspectorErrorCode::Unauthorized
    );
    let mut foreign = request(1, InspectorCommand::Attach, InspectorArgs::None);
    foreign.target.tab = TabId(8);
    assert_eq!(
        code(backend.handle(&foreign, (doc(0, 0), false), Sampled::default(), &mut port)),
        InspectorErrorCode::UnknownTarget
    );
    simple(&mut backend, &mut port, 1, InspectorCommand::Attach).unwrap();
    simple(&mut backend, &mut port, 1, InspectorCommand::Detach).unwrap();
    assert_eq!(
        code(simple(
            &mut backend,
            &mut port,
            1,
            InspectorCommand::GetStatusSnapshot
        )),
        InspectorErrorCode::StaleSession
    );
}

#[test]
fn status_events_flow_only_while_subscribed_and_only_for_changes() {
    let mut backend = Backend::new(own(), 23);
    let port = Port::new();
    assert!(backend.observe(doc(0, 0), false, &port).is_empty());
    let mut mutable = Port::new();
    simple(&mut backend, &mut mutable, 1, InspectorCommand::Attach).unwrap();
    assert!(
        backend.observe(doc(1, 1), true, &port).is_empty(),
        "attached but not subscribed"
    );
    run(
        &mut backend,
        &mut mutable,
        1,
        InspectorCommand::SubscribeStatus,
        InspectorArgs::None,
        (doc(1, 1), true),
    )
    .unwrap();
    assert!(backend.observe(doc(1, 1), true, &port).is_empty());
    let events = backend.observe(doc(2, 2), false, &port);
    assert_eq!(events.len(), 2);
    assert!(
        matches!(&events[0].2, InspectorEvent::DocumentChanged { document } if *document == doc(2, 2))
    );
    assert!(matches!(
        events[1].2,
        InspectorEvent::LoadingChanged { loading: false }
    ));
    assert!(events[0].1 < events[1].1);
    assert!(backend.observe(doc(2, 2), false, &port).is_empty());
}

#[test]
fn a_superseding_attach_stops_the_old_sessions_events() {
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    simple(&mut backend, &mut port, 1, InspectorCommand::Attach).unwrap();
    simple(
        &mut backend,
        &mut port,
        1,
        InspectorCommand::SubscribeStatus,
    )
    .unwrap();
    simple(&mut backend, &mut port, 2, InspectorCommand::Attach).unwrap();
    assert!(backend.observe(doc(1, 1), true, &port).is_empty());
    simple(
        &mut backend,
        &mut port,
        2,
        InspectorCommand::SubscribeStatus,
    )
    .unwrap();
    let events = backend.observe(doc(2, 2), true, &port);
    assert_eq!((events[0].0, events[0].1), (InspectorSessionId(2), 1));
    backend.detach(InspectorSessionId(1));
    assert_eq!(backend.live(), Some(InspectorSessionId(2)));
    backend.detach(InspectorSessionId(2));
    assert!(backend.live().is_none());
}

// ---- evaluation authority -------------------------------------------------------------

#[test]
fn evaluation_is_refused_without_a_grant_and_never_reaches_the_page() {
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    simple(&mut backend, &mut port, 1, InspectorCommand::Attach).unwrap();
    let (command, args) = evaluate(1, token(9));
    assert_eq!(
        code(run(
            &mut backend,
            &mut port,
            1,
            command,
            args,
            (doc(0, 0), false)
        )),
        InspectorErrorCode::Unauthorized
    );
    assert!(port.evaluations.borrow().is_empty());
}

#[test]
fn evaluation_runs_only_with_the_exact_grant_for_the_current_document() {
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    granted(&mut backend, &mut port, doc(1, 1));
    let at = (doc(1, 1), false);
    // Forged and zero tokens.
    for forged in [token(8), GrantToken([0; 16])] {
        let (command, args) = evaluate(1, forged);
        assert_eq!(
            code(run(&mut backend, &mut port, 1, command, args, at)),
            InspectorErrorCode::Unauthorized
        );
    }
    assert!(port.evaluations.borrow().is_empty());
    // The right token works, and runs the exact expression.
    let (command, args) = evaluate(1, token(9));
    let response = run(&mut backend, &mut port, 1, command, args, at).unwrap();
    assert!(matches!(response, InspectorResponse::Evaluated(_)));
    assert_eq!(port.evaluations.borrow().as_slice(), ["1+1"]);
}

#[test]
fn a_grant_is_revalidated_at_use_against_the_current_document_even_before_observe_runs() {
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    granted(&mut backend, &mut port, doc(1, 1));
    for moved in [doc(2, 1), doc(1, 2), doc(2, 2)] {
        let (command, args) = evaluate(1, token(9));
        assert_eq!(
            code(run(
                &mut backend,
                &mut port,
                1,
                command,
                args,
                (moved, false)
            )),
            InspectorErrorCode::StaleDocument
        );
    }
    assert!(port.evaluations.borrow().is_empty());
}

#[test]
fn navigation_detach_revoke_and_supersede_each_end_the_grant() {
    let ends = ["navigation", "revoke", "detach", "supersede"];
    for how in ends {
        let mut backend = Backend::new(own(), 23);
        let mut port = Port::new();
        granted(&mut backend, &mut port, doc(1, 1));
        match how {
            "navigation" => {
                backend.observe(doc(2, 2), false, &port);
            }
            "revoke" => {
                simple(
                    &mut backend,
                    &mut port,
                    1,
                    InspectorCommand::RevokeEvaluation,
                )
                .unwrap();
            }
            "detach" => backend.detach(InspectorSessionId(1)),
            _ => {
                simple(&mut backend, &mut port, 5, InspectorCommand::Attach).unwrap();
            }
        }
        let (command, args) = evaluate(1, token(9));
        let at = (
            if how == "navigation" {
                doc(2, 2)
            } else {
                doc(1, 1)
            },
            false,
        );
        assert!(
            run(&mut backend, &mut port, 1, command, args, at).is_err(),
            "{how}"
        );
        // The new session never inherits it either.
        if how == "supersede" {
            let (command, args) = evaluate(5, token(9));
            assert_eq!(
                code(run(&mut backend, &mut port, 5, command, args, at)),
                InspectorErrorCode::Unauthorized
            );
        }
        assert!(port.evaluations.borrow().is_empty(), "{how}");
    }
}

#[test]
fn a_grant_for_one_session_is_useless_to_another() {
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    granted(&mut backend, &mut port, doc(1, 1));
    let (command, args) = evaluate(2, token(9));
    let result = run(
        &mut backend,
        &mut port,
        2,
        command,
        args,
        (doc(1, 1), false),
    );
    assert!(matches!(
        code(result),
        InspectorErrorCode::Unauthorized | InspectorErrorCode::StaleSession
    ));
    assert!(port.evaluations.borrow().is_empty());
}

fn grant_args(origin: Option<&str>, opaque_override: bool) -> InspectorArgs {
    InspectorArgs::Grant {
        token: token(4),
        opaque_override,
        origin: origin.map(str::to_string),
    }
}
fn try_grant(
    backend: &mut Backend,
    port: &mut Port,
    origin: Option<&str>,
    opaque_override: bool,
) -> Result<InspectorResponse, InspectorError> {
    run(
        backend,
        port,
        1,
        InspectorCommand::GrantEvaluation,
        grant_args(origin, opaque_override),
        (doc(0, 0), false),
    )
}
fn evaluates(backend: &mut Backend, port: &mut Port) -> bool {
    let (command, args) = evaluate(1, token(4));
    run(backend, port, 1, command, args, (doc(0, 0), false)).is_ok()
}

#[test]
fn an_opaque_document_needs_the_users_override_and_a_tuple_claim_for_it_is_a_forgery() {
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    port.origin = None; // the engine's document has no tuple origin (about:, data:, file:...)
    simple(&mut backend, &mut port, 1, InspectorCommand::Attach).unwrap();
    // No override.
    assert_eq!(
        code(try_grant(&mut backend, &mut port, None, false)),
        InspectorErrorCode::PolicyDenied
    );
    // The browser claims a tuple origin for a document the engine knows has none.
    for claim in [
        "https://example.test",
        "https://evil.test",
        "http://127.0.0.1",
    ] {
        assert_eq!(
            code(try_grant(&mut backend, &mut port, Some(claim), false)),
            InspectorErrorCode::PolicyDenied,
            "{claim}"
        );
    }
    assert!(
        !evaluates(&mut backend, &mut port),
        "a refused grant stores nothing"
    );
    // The user's explicit override.
    try_grant(&mut backend, &mut port, None, true).unwrap();
    assert!(evaluates(&mut backend, &mut port));
    assert_eq!(port.evaluations.borrow().len(), 1);
    // It does not survive navigation.
    backend.observe(doc(1, 1), false, &port);
    let (command, args) = evaluate(1, token(4));
    assert!(
        run(
            &mut backend,
            &mut port,
            1,
            command,
            args,
            (doc(1, 1), false)
        )
        .is_err()
    );
    assert_eq!(port.evaluations.borrow().len(), 1);
}

#[test]
fn a_tuple_grant_must_match_the_engines_own_origin_at_grant_time_and_at_execution() {
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    simple(&mut backend, &mut port, 1, InspectorCommand::Attach).unwrap();
    // Wrong origin, wrong port, wrong scheme: refused; nothing stored.
    for claim in [
        "https://evil.test",
        "https://example.test:8443",
        "http://example.test",
    ] {
        assert_eq!(
            code(try_grant(&mut backend, &mut port, Some(claim), false)),
            InspectorErrorCode::PolicyDenied,
            "{claim}"
        );
    }
    assert!(!evaluates(&mut backend, &mut port));
    // A tuple claim without an override is enough for a matching tuple document...
    try_grant(&mut backend, &mut port, Some("https://example.test"), false).unwrap();
    assert!(evaluates(&mut backend, &mut port));
    // ...but if the document's origin is no longer what the grant covered (same stamp, e.g. the
    // engine's URL moved), execution is refused and the page is not touched.
    port.origin = Some("https://other.test".into());
    let (command, args) = evaluate(1, token(4));
    assert_eq!(
        code(run(
            &mut backend,
            &mut port,
            1,
            command,
            args,
            (doc(0, 0), false)
        )),
        InspectorErrorCode::PolicyDenied
    );
    assert_eq!(port.evaluations.borrow().len(), 1);
    port.origin = None;
    let (command, args) = evaluate(1, token(4));
    assert_eq!(
        code(run(
            &mut backend,
            &mut port,
            1,
            command,
            args,
            (doc(0, 0), false)
        )),
        InspectorErrorCode::PolicyDenied
    );
}

#[test]
fn the_override_is_honoured_when_the_browsers_principal_is_stricter_than_the_engines_url() {
    // An https document the broker could not corroborate has an opaque principal in the
    // browser; the engine sees a tuple URL. The user's override must still work.
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    simple(&mut backend, &mut port, 1, InspectorCommand::Attach).unwrap();
    assert_eq!(
        code(try_grant(&mut backend, &mut port, None, false)),
        InspectorErrorCode::PolicyDenied
    );
    try_grant(&mut backend, &mut port, None, true).unwrap();
    assert!(evaluates(&mut backend, &mut port));
}

#[test]
fn a_malformed_origin_in_a_grant_is_an_invalid_payload() {
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    simple(&mut backend, &mut port, 1, InspectorCommand::Attach).unwrap();
    // The protocol rejects it before the backend sees it; the backend also refuses it directly.
    let request = request(
        1,
        InspectorCommand::GrantEvaluation,
        grant_args(Some("https://example.test/x"), false),
    );
    assert_eq!(
        request.validate().unwrap_err().code,
        InspectorErrorCode::InvalidPayload
    );
    assert_eq!(
        code(backend.handle(&request, (doc(0, 0), false), Sampled::default(), &mut port)),
        InspectorErrorCode::InvalidPayload
    );
}

#[test]
fn a_loading_document_is_not_evaluated_and_a_fatal_failure_reports_what_is_known() {
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    granted(&mut backend, &mut port, doc(1, 1));
    let (command, args) = evaluate(1, token(9));
    assert_eq!(
        code(run(
            &mut backend,
            &mut port,
            1,
            command,
            args,
            (doc(1, 1), true)
        )),
        InspectorErrorCode::Busy
    );
    assert!(port.evaluations.borrow().is_empty());
    *port.outcome.borrow_mut() = Err("The page's script runtime was terminated".into());
    let (command, args) = evaluate(1, token(9));
    let error = run(
        &mut backend,
        &mut port,
        1,
        command,
        args,
        (doc(1, 1), false),
    )
    .unwrap_err();
    assert_eq!(error.code, InspectorErrorCode::Unavailable);
    assert!(error.message.contains("terminated"));
}

#[test]
fn evaluation_exceptions_are_results_not_errors() {
    let mut backend = Backend::new(own(), 23);
    let mut port = Port::new();
    granted(&mut backend, &mut port, doc(1, 1));
    *port.outcome.borrow_mut() = Ok(EvalOutcome::Exception {
        details: ExceptionDetails {
            kind: ExceptionKind::Evaluation,
            name: "TypeError".into(),
            message: "x".into(),
            stack: None,
            source: None,
        },
    });
    let (command, args) = evaluate(1, token(9));
    let InspectorResponse::Evaluated(outcome) = run(
        &mut backend,
        &mut port,
        1,
        command,
        args,
        (doc(1, 1), false),
    )
    .unwrap() else {
        panic!()
    };
    assert!(matches!(*outcome, EvalOutcome::Exception { .. }));
}

// ---- console delivery ----------------------------------------------------------------

fn enabled(port: &mut Port) -> Backend {
    let mut backend = Backend::new(own(), 23);
    simple(&mut backend, port, 1, InspectorCommand::Attach).unwrap();
    simple(&mut backend, port, 1, InspectorCommand::EnableConsole).unwrap();
    backend
}
fn seqs(events: &[(InspectorSessionId, u64, InspectorEvent)]) -> Vec<u64> {
    events
        .iter()
        .filter_map(|(_, _, e)| match e {
            InspectorEvent::ConsoleMessageAdded(m) | InspectorEvent::ExceptionThrown(m) => {
                Some(m.seq)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn enabling_backfills_messages_logged_before_the_panel_opened_in_budgeted_batches() {
    let mut port = Port::new();
    for i in 0..100 {
        port.log(&format!("early {i}"), i == 40);
    }
    let mut backend = enabled(&mut port);
    let mut delivered = Vec::new();
    let mut batches = 0;
    loop {
        let events = backend.observe(doc(0, 0), false, &port);
        if events.is_empty() {
            break;
        }
        assert!(
            seqs(&events).len() <= CONSOLE_BATCH_ENTRIES,
            "a batch exceeded its budget"
        );
        delivered.extend(seqs(&events));
        batches += 1;
    }
    assert_eq!(
        delivered,
        (1..=100).collect::<Vec<u64>>(),
        "in order, none lost, none repeated"
    );
    assert_eq!(batches, 4, "100 messages at 32 per iteration");
    // The exception arrived as an exception event.
    let mut again = enabled(&mut port);
    let events = again.observe(doc(0, 0), false, &port);
    assert!(
        events
            .iter()
            .all(|(_, _, e)| matches!(e, InspectorEvent::ConsoleMessageAdded(_)))
    );
    port.log("later", true);
    let all: Vec<_> = (0..4)
        .flat_map(|_| again.observe(doc(0, 0), false, &port))
        .collect();
    assert!(
        all.iter().any(
            |(_, _, e)| matches!(e, InspectorEvent::ExceptionThrown(m) if m.text == "early 40")
        )
    );
    assert!(
        all.iter()
            .any(|(_, _, e)| matches!(e, InspectorEvent::ExceptionThrown(m) if m.text == "later"))
    );
}

#[test]
fn new_messages_arrive_once_and_a_disabled_console_costs_nothing() {
    let mut port = Port::new();
    let mut backend = enabled(&mut port);
    assert!(backend.observe(doc(0, 0), false, &port).is_empty());
    port.log("a", false);
    assert_eq!(seqs(&backend.observe(doc(0, 0), false, &port)), [1]);
    assert!(
        backend.observe(doc(0, 0), false, &port).is_empty(),
        "no repeats"
    );
    simple(&mut backend, &mut port, 1, InspectorCommand::DisableConsole).unwrap();
    let reads = port.reads.get();
    port.log("b", false);
    assert!(backend.observe(doc(0, 0), false, &port).is_empty());
    assert_eq!(
        port.reads.get(),
        reads,
        "the ring is not touched while disabled"
    );
    // Without any session the backend never looks at the page at all.
    let idle = Backend::new(own(), 23);
    let mut idle = idle;
    assert!(idle.observe(doc(0, 0), false, &port).is_empty());
    assert_eq!(port.reads.get(), reads);
}

#[test]
fn a_consumer_that_falls_behind_the_ring_is_told_how_much_was_lost() {
    let mut port = Port::new();
    let mut backend = enabled(&mut port);
    for i in 0..10 {
        port.log(&format!("m{i}"), false);
    }
    assert_eq!(seqs(&backend.observe(doc(0, 0), false, &port)).len(), 10);
    for i in 0..300 {
        port.log(&format!("n{i}"), false);
    }
    port.evict_before(200); // the ring kept 200..=310 only
    let events = backend.observe(doc(0, 0), false, &port);
    assert!(
        matches!(events[0].2, InspectorEvent::ConsoleMessagesDropped { count } if count == 189),
        "{:?}",
        events[0].2
    );
    assert_eq!(seqs(&events)[0], 200);
    // Drain the rest; nothing repeats and nothing is skipped after the marker.
    let mut rest = seqs(&events);
    loop {
        let more = backend.observe(doc(0, 0), false, &port);
        if more.is_empty() {
            break;
        }
        rest.extend(seqs(&more));
    }
    assert_eq!(rest, (200..=310).collect::<Vec<u64>>());
}

#[test]
fn clearing_discards_history_without_delivering_it_or_reporting_drops() {
    let mut port = Port::new();
    for i in 0..5 {
        port.log(&format!("old {i}"), false);
    }
    let mut backend = enabled(&mut port);
    simple(&mut backend, &mut port, 1, InspectorCommand::ClearConsole).unwrap();
    assert!(backend.observe(doc(0, 0), false, &port).is_empty());
    port.log("after", false);
    let events = backend.observe(doc(0, 0), false, &port);
    assert_eq!(seqs(&events), [6]);
    assert!(
        !events
            .iter()
            .any(|(_, _, e)| matches!(e, InspectorEvent::ConsoleMessagesDropped { .. }))
    );
}

#[test]
fn a_new_document_gets_context_events_and_a_fresh_cursor() {
    let mut port = Port::new();
    let mut backend = enabled(&mut port);
    port.log("old document", false);
    assert_eq!(seqs(&backend.observe(doc(1, 1), false, &port)), [1]);
    // The document is replaced: a fresh ring starts again at sequence 1.
    port.messages.borrow_mut().clear();
    port.first.set(1);
    port.next.set(1);
    port.log("new document", false);
    let events = backend.observe(doc(2, 2), false, &port);
    assert!(
        matches!(events[0].2, InspectorEvent::ExecutionContextDestroyed { document } if document == doc(1, 1))
    );
    assert!(
        matches!(events[1].2, InspectorEvent::ExecutionContextCreated { document } if document == doc(2, 2))
    );
    assert_eq!(seqs(&events), [1]);
}

#[test]
fn history_pages_are_bounded_and_resumable() {
    let mut port = Port::new();
    for i in 0..250 {
        port.log(&format!("h{i}"), false);
    }
    let mut backend = Backend::new(own(), 23);
    simple(&mut backend, &mut port, 1, InspectorCommand::Attach).unwrap();
    let mut after = 0;
    let mut total = 0;
    loop {
        let InspectorResponse::ConsoleHistory {
            messages, next_seq, ..
        } = run(
            &mut backend,
            &mut port,
            1,
            InspectorCommand::GetConsoleHistory,
            InspectorArgs::History { after, limit: 100 },
            (doc(0, 0), false),
        )
        .unwrap()
        else {
            panic!()
        };
        if messages.is_empty() {
            break;
        }
        assert!(messages.len() <= 100);
        total += messages.len();
        after = next_seq;
    }
    assert_eq!(total, 250);
}

#[test]
fn the_backend_source_names_no_layout_or_script_engine() {
    let code = include_str!("mod.rs")
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    for word in ["blitz_", "stylo", "v8::", "deno", "servo"] {
        assert!(!code.contains(word), "inspector backend mentions `{word}`");
    }
}
