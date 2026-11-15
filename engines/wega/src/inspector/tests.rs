use super::*;
use wega_protocol::{
    DocumentId, DocumentRevision,
    identity::{FrameId, ProcessSessionId, TabId},
    inspector::InspectorErrorCode,
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
fn input() -> StatusInput {
    StatusInput {
        url: "https://example.test/".into(),
        title: "Example".into(),
        loading: false,
        viewport: Some((800.0, 600.0, 2.0)),
        nodes: 12,
        js_heap_bytes: 4096,
        can_go_back: false,
        can_go_forward: true,
    }
}
fn request(session: u64, command: InspectorCommand) -> InspectorRequest {
    InspectorRequest {
        version: INSPECTOR_VERSION,
        session: InspectorSessionId(session),
        target: own(),
        command,
    }
}

#[test]
fn snapshot_reports_exactly_the_supplied_facts_and_identity() {
    let mut backend = Backend::new(own(), 22);
    backend
        .handle(
            &request(1, InspectorCommand::Attach),
            (doc(2, 3), false),
            input,
        )
        .unwrap();
    let InspectorResponse::Status(snapshot) = backend
        .handle(
            &request(1, InspectorCommand::GetStatusSnapshot),
            (doc(2, 3), false),
            input,
        )
        .unwrap()
    else {
        panic!("not a snapshot")
    };
    assert_eq!(snapshot.target, own());
    assert_eq!(snapshot.document, doc(2, 3));
    assert_eq!(snapshot.url, "https://example.test/");
    assert_eq!(snapshot.title, "Example");
    assert_eq!(snapshot.nodes, 12);
    assert_eq!(snapshot.js_heap_bytes, 4096);
    assert!(snapshot.can_go_forward && !snapshot.can_go_back);
    let viewport = snapshot.viewport.unwrap();
    assert_eq!(
        (viewport.width, viewport.height, viewport.device_pixel_ratio),
        (800.0, 600.0, 2.0)
    );
    assert_eq!(snapshot.protocol_version, 22);
    assert_eq!(snapshot.inspector_version, INSPECTOR_VERSION);
    assert!(snapshot.engine.starts_with("wega-engine "));
}

#[test]
fn snapshot_fields_are_bounded() {
    let mut backend = Backend::new(own(), 22);
    backend
        .handle(
            &request(1, InspectorCommand::Attach),
            (doc(0, 0), false),
            input,
        )
        .unwrap();
    let huge = || StatusInput {
        url: "u".repeat(1 << 20),
        title: "日".repeat(1 << 18),
        ..input()
    };
    let InspectorResponse::Status(snapshot) = backend
        .handle(
            &request(1, InspectorCommand::GetStatusSnapshot),
            (doc(0, 0), false),
            huge,
        )
        .unwrap()
    else {
        panic!()
    };
    assert!(snapshot.is_bounded());
    assert!(serde_json::to_vec(&snapshot).unwrap().len() < 16 * 1024);
}

#[test]
fn the_status_source_is_not_consulted_unless_a_snapshot_is_requested() {
    let mut backend = Backend::new(own(), 22);
    for command in [
        InspectorCommand::Attach,
        InspectorCommand::GetTargetInfo,
        InspectorCommand::SubscribeStatus,
        InspectorCommand::UnsubscribeStatus,
        InspectorCommand::Detach,
    ] {
        backend
            .handle(&request(1, command), (doc(0, 0), false), || {
                panic!("{command:?} must not gather a snapshot")
            })
            .unwrap();
        if command == InspectorCommand::Detach {
            break;
        }
    }
}

#[test]
fn requests_before_attach_after_detach_or_for_other_targets_are_refused() {
    let mut backend = Backend::new(own(), 22);
    let refuse = |backend: &mut Backend, req: &InspectorRequest| {
        backend
            .handle(req, (doc(0, 0), false), input)
            .unwrap_err()
            .code
    };
    assert_eq!(
        refuse(
            &mut backend,
            &request(1, InspectorCommand::GetStatusSnapshot)
        ),
        InspectorErrorCode::Unauthorized
    );
    let mut foreign = request(1, InspectorCommand::Attach);
    foreign.target.tab = TabId(8);
    assert_eq!(
        refuse(&mut backend, &foreign),
        InspectorErrorCode::UnknownTarget
    );
    foreign.target = own();
    foreign.target.process = ProcessSessionId([4; 16]);
    assert_eq!(
        refuse(&mut backend, &foreign),
        InspectorErrorCode::UnknownTarget
    );
    assert!(backend.live().is_none());
    backend
        .handle(
            &request(1, InspectorCommand::Attach),
            (doc(0, 0), false),
            input,
        )
        .unwrap();
    backend
        .handle(
            &request(1, InspectorCommand::Detach),
            (doc(0, 0), false),
            input,
        )
        .unwrap();
    assert_eq!(
        refuse(
            &mut backend,
            &request(1, InspectorCommand::GetStatusSnapshot)
        ),
        InspectorErrorCode::StaleSession
    );
    let mut old = request(1, InspectorCommand::Attach);
    old.version = 99;
    assert_eq!(
        refuse(&mut backend, &old),
        InspectorErrorCode::UnsupportedVersion
    );
}

#[test]
fn events_flow_only_while_subscribed_and_only_for_changes() {
    let mut backend = Backend::new(own(), 22);
    // No session: free.
    assert!(backend.observe(doc(0, 0), false).is_empty());
    backend
        .handle(
            &request(1, InspectorCommand::Attach),
            (doc(0, 0), false),
            input,
        )
        .unwrap();
    assert!(
        backend.observe(doc(1, 1), true).is_empty(),
        "attached but not subscribed"
    );
    backend
        .handle(
            &request(1, InspectorCommand::SubscribeStatus),
            (doc(1, 1), true),
            input,
        )
        .unwrap();
    assert!(
        backend.observe(doc(1, 1), true).is_empty(),
        "no change since the baseline"
    );
    let events = backend.observe(doc(2, 2), false);
    assert_eq!(events.len(), 2);
    assert!(
        matches!(events[0].2, InspectorEvent::DocumentChanged { document } if document == doc(2, 2))
    );
    assert!(matches!(
        events[1].2,
        InspectorEvent::LoadingChanged { loading: false }
    ));
    assert!(
        events[0].1 < events[1].1,
        "strictly increasing sequence numbers"
    );
    assert!(backend.observe(doc(2, 2), false).is_empty());
    // A revision-only change is a document change too.
    assert_eq!(backend.observe(doc(2, 3), false).len(), 1);
    backend
        .handle(
            &request(1, InspectorCommand::UnsubscribeStatus),
            (doc(2, 3), false),
            input,
        )
        .unwrap();
    assert!(backend.observe(doc(5, 5), true).is_empty());
}

#[test]
fn a_superseding_attach_stops_the_old_sessions_events_and_resets_sequencing() {
    let mut backend = Backend::new(own(), 22);
    backend
        .handle(
            &request(1, InspectorCommand::Attach),
            (doc(0, 0), false),
            input,
        )
        .unwrap();
    backend
        .handle(
            &request(1, InspectorCommand::SubscribeStatus),
            (doc(0, 0), false),
            input,
        )
        .unwrap();
    backend
        .handle(
            &request(2, InspectorCommand::Attach),
            (doc(0, 0), false),
            input,
        )
        .unwrap();
    assert!(
        backend.observe(doc(1, 1), true).is_empty(),
        "the new session has not subscribed"
    );
    backend
        .handle(
            &request(2, InspectorCommand::SubscribeStatus),
            (doc(1, 1), true),
            input,
        )
        .unwrap();
    let events = backend.observe(doc(2, 2), true);
    assert_eq!(events[0].0, InspectorSessionId(2));
    assert_eq!(events[0].1, 1);
    // The old session id is dead for requests and for detach.
    assert!(
        backend
            .handle(
                &request(1, InspectorCommand::GetTargetInfo),
                (doc(2, 2), true),
                input
            )
            .is_err()
    );
    backend.detach(InspectorSessionId(1));
    assert_eq!(backend.live(), Some(InspectorSessionId(2)));
    backend.detach(InspectorSessionId(2));
    assert!(backend.live().is_none());
    backend.detach(InspectorSessionId(2));
}

#[test]
fn the_backend_source_names_no_layout_or_script_engine() {
    for source in [include_str!("mod.rs")] {
        let code = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        for word in ["blitz_", "stylo", "v8::", "deno", "servo"] {
            assert!(!code.contains(word), "inspector backend mentions `{word}`");
        }
    }
}
