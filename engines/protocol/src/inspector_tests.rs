use super::session::*;
use super::*;
use crate::{
    Command, DocumentId, DocumentRevision, ErrorCode, Event, ProtocolError, RequestContext,
    RequestId,
    identity::{FrameId, ProcessSessionId, SessionIdentity, TabId},
    requests::{DocumentRequests, RequestKind},
};
use std::time::Duration;

fn identity(tab: u64, process: u8) -> SessionIdentity {
    SessionIdentity {
        tab: TabId(tab),
        frame: FrameId(1),
        process: ProcessSessionId([process; 16]),
        service: None,
    }
}
fn target(tab: u64, process: u8) -> TargetId {
    TargetId::of(&identity(tab, process))
}
fn request(session: u64, target: TargetId, command: InspectorCommand) -> InspectorRequest {
    InspectorRequest {
        version: INSPECTOR_VERSION,
        session: InspectorSessionId(session),
        target,
        command,
    }
}
fn code(result: Result<(), InspectorError>) -> InspectorErrorCode {
    result.unwrap_err().code
}
const ALL: [InspectorCommand; 6] = [
    InspectorCommand::Attach,
    InspectorCommand::Detach,
    InspectorCommand::GetTargetInfo,
    InspectorCommand::SubscribeStatus,
    InspectorCommand::UnsubscribeStatus,
    InspectorCommand::GetStatusSnapshot,
];

#[test]
fn every_command_round_trips_through_the_wire_envelope() {
    let target = target(3, 7);
    for command in ALL {
        let req = request(5, target, command);
        let packet = Command::Inspector {
            context: RequestContext {
                document: DocumentId(2),
                revision: DocumentRevision(4),
                request: RequestId(9),
            },
            request: InspectorPayload::new(&req),
        };
        let mut bytes = Vec::new();
        crate::write(&mut bytes, &packet).unwrap();
        let Command::Inspector { request: back, .. } =
            crate::read::<Command>(&mut bytes.as_slice())
                .unwrap()
                .unwrap()
        else {
            panic!("wrong variant")
        };
        assert_eq!(back.decode().unwrap(), req);
    }
}

#[test]
fn malformed_payloads_are_answered_with_invalid_payload_and_never_panic() {
    let good = serde_json::to_value(request(1, target(1, 1), InspectorCommand::Attach)).unwrap();
    let mut unknown_field = good.clone();
    unknown_field["extra"] = serde_json::json!(true);
    let mut unknown_command = good.clone();
    unknown_command["command"] = serde_json::json!("Evaluate");
    let mut wrong_type = good.clone();
    wrong_type["session"] = serde_json::json!("seven");
    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("target");
    let mut negative = good.clone();
    negative["session"] = serde_json::json!(-1);
    let mut command_with_payload = good.clone();
    command_with_payload["command"] = serde_json::json!({"Attach": {"expression": "1+1"}});
    for value in [
        unknown_field,
        unknown_command,
        wrong_type,
        missing,
        negative,
        command_with_payload,
        serde_json::Value::Null,
        serde_json::json!([]),
        serde_json::json!("Attach"),
        serde_json::json!(1),
    ] {
        let error = InspectorPayload(value.clone()).decode().unwrap_err();
        assert_eq!(error.code, InspectorErrorCode::InvalidPayload, "{value}");
        assert!(error.message.len() <= MAX_ERROR_MESSAGE * 4);
    }
    assert!(InspectorPayload(good).decode().is_ok());
}

#[test]
fn there_is_no_representable_arbitrary_command() {
    // The command enum is closed and parameterless: its serialised form is a bare string.
    for command in ALL {
        assert!(serde_json::to_value(command).unwrap().is_string());
    }
    for forged in ["Evaluate", "Eval", "RunScript", "Navigate", "attach", ""] {
        assert!(serde_json::from_value::<InspectorCommand>(serde_json::json!(forged)).is_err());
    }
}

#[test]
fn oversize_and_truncated_frames_are_rejected_before_use() {
    let big = InspectorPayload(serde_json::json!({"pad": "x".repeat(crate::MAX_MESSAGE)}));
    let packet = Command::Inspector {
        context: RequestContext {
            document: DocumentId(0),
            revision: DocumentRevision(0),
            request: RequestId(1),
        },
        request: big,
    };
    assert!(crate::write(&mut Vec::new(), &packet).is_err());
    let mut bytes = Vec::new();
    crate::write(
        &mut bytes,
        &Command::InspectorDetach {
            session: InspectorSessionId(1),
        },
    )
    .unwrap();
    for cut in 1..bytes.len() {
        assert!(
            crate::read::<Command>(&mut &bytes[..cut]).is_err(),
            "cut {cut}"
        );
    }
}

#[test]
fn unsupported_inspector_versions_are_refused() {
    let mut sessions = ContentSessions::default();
    let own = target(1, 1);
    for version in [0, INSPECTOR_VERSION + 1, u32::MAX] {
        let mut req = request(1, own, InspectorCommand::Attach);
        req.version = version;
        assert_eq!(
            code(sessions.authorize(&req, own)),
            InspectorErrorCode::UnsupportedVersion
        );
    }
    assert!(sessions.live().is_none());
}

#[test]
fn the_capability_and_version_are_negotiated() {
    assert_eq!(crate::VERSION, 22);
    assert!(!crate::capabilities(21).contains(&crate::Capability::Inspector));
    assert!(crate::capabilities(22).contains(&crate::Capability::Inspector));
    assert!(crate::supports_version(21) && crate::supports_version(22));
    assert!(!crate::supports_version(23));
}

#[test]
fn issuer_is_monotonic_unique_across_threads_and_fails_at_exhaustion() {
    let issuer = SessionIssuer::new();
    assert_eq!(issuer.next().unwrap(), InspectorSessionId(1));
    assert_eq!(issuer.next().unwrap(), InspectorSessionId(2));
    let shared = std::sync::Arc::new(SessionIssuer::new());
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let shared = shared.clone();
            std::thread::spawn(move || {
                (0..250)
                    .map(|_| shared.next().unwrap().0)
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut all: Vec<u64> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 1000, "an id was issued twice");
    assert!(all.iter().all(|id| *id != 0));
    let exhausted = SessionIssuer::starting_at(u64::MAX - 1);
    assert_eq!(exhausted.next().unwrap(), InspectorSessionId(u64::MAX));
    assert_eq!(
        exhausted.next().unwrap_err().code,
        InspectorErrorCode::Unavailable
    );
    assert_eq!(
        exhausted.next().unwrap_err().code,
        InspectorErrorCode::Unavailable
    );
}

#[test]
fn registry_authorises_only_the_live_session_on_its_own_target() {
    let issuer = SessionIssuer::new();
    let mine = target(1, 1);
    let mut registry = SessionRegistry::new(mine);
    let (first, superseded) = registry.open(&issuer).unwrap();
    assert!(superseded.is_none());
    registry.authorize(first, mine).unwrap();
    // Cross-tab and cross-process targets.
    assert_eq!(
        code(registry.authorize(first, target(2, 1))),
        InspectorErrorCode::UnknownTarget
    );
    assert_eq!(
        code(registry.authorize(first, target(1, 2))),
        InspectorErrorCode::UnknownTarget
    );
    // Forged ids.
    assert_eq!(
        code(registry.authorize(InspectorSessionId(first.0 + 1000), mine)),
        InspectorErrorCode::Unauthorized
    );
    assert_eq!(
        code(registry.authorize(InspectorSessionId(0), mine)),
        InspectorErrorCode::Unauthorized
    );
    // Supersede: the old id becomes stale, the new one is live.
    let (second, superseded) = registry.open(&issuer).unwrap();
    assert_eq!(superseded, Some(first));
    assert_eq!(
        code(registry.authorize(first, mine)),
        InspectorErrorCode::StaleSession
    );
    registry.authorize(second, mine).unwrap();
    // Revoke.
    assert!(registry.revoke(second));
    assert!(!registry.revoke(second), "revocation is idempotent");
    assert_eq!(
        code(registry.authorize(second, mine)),
        InspectorErrorCode::StaleSession
    );
    assert!(registry.live().is_none());
}

#[test]
fn a_closed_registry_never_authorises_again_and_cannot_open() {
    let issuer = SessionIssuer::new();
    let mine = target(1, 1);
    let mut registry = SessionRegistry::new(mine);
    let (id, _) = registry.open(&issuer).unwrap();
    assert_eq!(registry.close(), Some(id));
    assert_eq!(
        code(registry.authorize(id, mine)),
        InspectorErrorCode::Unavailable
    );
    assert_eq!(
        registry.open(&issuer).unwrap_err().code,
        InspectorErrorCode::Unavailable
    );
    assert!(!registry.set_subscribed(id, true));
}

#[test]
fn sessions_of_different_tabs_never_collide() {
    let issuer = SessionIssuer::new();
    let (mut a, mut b) = (
        SessionRegistry::new(target(1, 1)),
        SessionRegistry::new(target(2, 2)),
    );
    let (sa, _) = a.open(&issuer).unwrap();
    let (sb, _) = b.open(&issuer).unwrap();
    assert_ne!(sa, sb);
    assert!(a.authorize(sb, a.target()).is_err());
    assert!(b.authorize(sa, b.target()).is_err());
    assert!(a.authorize(sa, b.target()).is_err());
}

#[test]
fn content_mirror_requires_attach_and_rejects_replays() {
    let own = target(4, 9);
    let mut content = ContentSessions::default();
    // Nothing is authorised before attach.
    assert_eq!(
        code(content.authorize(&request(5, own, InspectorCommand::GetStatusSnapshot), own)),
        InspectorErrorCode::Unauthorized
    );
    content
        .authorize(&request(5, own, InspectorCommand::Attach), own)
        .unwrap();
    // Idempotent attach (retry after a lost reply).
    content
        .authorize(&request(5, own, InspectorCommand::Attach), own)
        .unwrap();
    content
        .authorize(&request(5, own, InspectorCommand::GetStatusSnapshot), own)
        .unwrap();
    // Wrong target.
    assert_eq!(
        code(content.authorize(
            &request(5, target(5, 9), InspectorCommand::GetTargetInfo),
            own
        )),
        InspectorErrorCode::UnknownTarget
    );
    // A newer attach supersedes; the old id is stale for everything including attach.
    content
        .authorize(&request(8, own, InspectorCommand::Attach), own)
        .unwrap();
    for command in ALL {
        assert_eq!(
            code(content.authorize(&request(5, own, command), own)),
            InspectorErrorCode::StaleSession,
            "{command:?}"
        );
    }
    // Forged ahead-of-time id never attached.
    assert_eq!(
        code(content.authorize(&request(99, own, InspectorCommand::GetStatusSnapshot), own)),
        InspectorErrorCode::Unauthorized
    );
    // Detach is idempotent and revokes.
    assert!(content.detach(InspectorSessionId(8)));
    assert!(!content.detach(InspectorSessionId(8)));
    assert_eq!(
        code(content.authorize(&request(8, own, InspectorCommand::GetStatusSnapshot), own)),
        InspectorErrorCode::StaleSession
    );
    assert!(
        !content.detach(InspectorSessionId(5)),
        "a stale detach changes nothing"
    );
}

fn snapshot(target: TargetId) -> InspectorResponse {
    InspectorResponse::Status(Box::new(
        StatusSnapshot {
            target,
            document: DocumentContext {
                document: DocumentId(1),
                revision: DocumentRevision(1),
            },
            url: "https://example.test/".into(),
            title: "t".into(),
            loading: false,
            viewport: None,
            nodes: 1,
            js_heap_bytes: 1,
            can_go_back: false,
            can_go_forward: false,
            protocol_version: crate::VERSION,
            inspector_version: INSPECTOR_VERSION,
            engine: "wega-engine".into(),
        }
        .bounded(),
    ))
}

#[test]
fn host_drops_late_replies_after_detach_and_replies_for_other_targets() {
    let issuer = SessionIssuer::new();
    let mine = target(1, 1);
    let mut host = HostSessions::new(mine);
    let (id, _) = host.open(&issuer).unwrap();
    assert!(host.complete(id, InspectorCommand::GetStatusSnapshot, Ok(snapshot(mine))));
    // A snapshot that names another target is never delivered.
    assert!(!host.complete(
        id,
        InspectorCommand::GetStatusSnapshot,
        Ok(snapshot(target(2, 2)))
    ));
    // An oversize snapshot is refused rather than stored.
    let InspectorResponse::Status(mut big) = snapshot(mine) else {
        unreachable!()
    };
    big.url = "u".repeat(MAX_FIELD * 4);
    assert!(!host.complete(
        id,
        InspectorCommand::GetStatusSnapshot,
        Ok(InspectorResponse::Status(big))
    ));
    assert_eq!(host.take_results(id).len(), 1);
    // Revoked: the delayed response is dropped and nothing is queued.
    assert!(host.revoke(id));
    assert!(!host.complete(id, InspectorCommand::GetStatusSnapshot, Ok(snapshot(mine))));
    assert!(host.take_results(id).is_empty());
    assert_eq!(host.queued(), (0, 0));
}

#[test]
fn events_require_a_subscription_reject_replays_and_overflow_is_counted_in_order() {
    let issuer = SessionIssuer::new();
    let mine = target(1, 1);
    let mut host = HostSessions::new(mine);
    let (id, _) = host.open(&issuer).unwrap();
    let loading = |value| InspectorEvent::LoadingChanged { loading: value };
    assert!(!host.event(id, 1, loading(true)), "not subscribed yet");
    assert!(host.complete(
        id,
        InspectorCommand::SubscribeStatus,
        Ok(InspectorResponse::Subscribed)
    ));
    assert!(host.event(id, 1, loading(true)));
    assert!(!host.event(id, 1, loading(false)), "replayed sequence");
    assert!(!host.event(id, 0, loading(false)), "old sequence");
    assert!(
        !host.event(InspectorSessionId(id.0 + 1), 2, loading(false)),
        "foreign session"
    );
    // A slow consumer: 10x the ring.
    for seq in 2..(2 + EVENT_RING as u64 * 10) {
        host.event(id, seq, loading(seq % 2 == 0));
        assert!(host.queued().0 <= EVENT_RING, "ring exceeded its bound");
    }
    let events = host.take_events(id);
    assert_eq!(events.len(), EVENT_RING + 1);
    let InspectorEvent::EventsDropped { count } = events[0] else {
        panic!("overflow must be reported first: {events:?}")
    };
    assert_eq!(count, 1 + EVENT_RING as u64 * 9);
    assert_eq!(count as usize + EVENT_RING, 1 + EVENT_RING * 10);
    assert!(
        host.take_events(id).is_empty(),
        "the marker is delivered once"
    );
    assert!(host.complete(
        id,
        InspectorCommand::UnsubscribeStatus,
        Ok(InspectorResponse::Unsubscribed)
    ));
    assert!(!host.event(id, 100_000, loading(true)));
}

#[test]
fn closing_the_host_discards_queues_and_refuses_everything() {
    let issuer = SessionIssuer::new();
    let mine = target(1, 1);
    let mut host = HostSessions::new(mine);
    let (id, _) = host.open(&issuer).unwrap();
    host.complete(
        id,
        InspectorCommand::SubscribeStatus,
        Ok(InspectorResponse::Subscribed),
    );
    host.event(id, 1, InspectorEvent::LoadingChanged { loading: true });
    assert_eq!(host.close(), Some(id));
    assert_eq!(host.queued(), (0, 0));
    assert!(host.take_events(id).is_empty());
    assert!(!host.event(id, 2, InspectorEvent::LoadingChanged { loading: false }));
    assert!(host.open(&issuer).is_err());
}

#[test]
fn opening_a_new_session_discards_the_old_sessions_queue() {
    let issuer = SessionIssuer::new();
    let mut host = HostSessions::new(target(1, 1));
    let (a, _) = host.open(&issuer).unwrap();
    host.complete(
        a,
        InspectorCommand::SubscribeStatus,
        Ok(InspectorResponse::Subscribed),
    );
    host.event(a, 1, InspectorEvent::LoadingChanged { loading: true });
    let (b, superseded) = host.open(&issuer).unwrap();
    assert_eq!(superseded, Some(a));
    assert_eq!(host.queued(), (0, 0));
    assert!(
        host.take_events(a).is_empty(),
        "the stale session reads nothing"
    );
    assert!(
        !host.event(b, 1, InspectorEvent::LoadingChanged { loading: true }),
        "b is not subscribed"
    );
}

#[test]
fn inspector_requests_are_capped_below_the_shared_pending_limit() {
    let mut requests = DocumentRequests::default();
    let session = InspectorSessionId(1);
    let mut contexts = Vec::new();
    for _ in 0..MAX_INSPECTOR_PENDING {
        contexts.push(
            requests
                .begin_inspector(
                    session,
                    InspectorCommand::GetStatusSnapshot,
                    Duration::from_secs(1),
                )
                .unwrap(),
        );
    }
    let error = requests
        .begin_inspector(
            session,
            InspectorCommand::GetStatusSnapshot,
            Duration::from_secs(1),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourceLimit);
    // Other request kinds are unaffected.
    assert!(
        requests
            .begin(RequestKind::Evaluate, Duration::from_secs(1))
            .is_ok()
    );
    // Cancelling the session frees the sub-cap; late replies for cancelled requests are ignored.
    let cancelled = requests.cancel_inspector(session);
    assert_eq!(cancelled.len(), MAX_INSPECTOR_PENDING);
    assert!(requests.take(contexts[0]).is_none());
    assert!(
        requests
            .begin_inspector(session, InspectorCommand::Attach, Duration::from_secs(1))
            .is_ok()
    );
}

#[test]
fn navigation_cancels_pending_inspector_requests_and_stales_their_replies() {
    let mut requests = DocumentRequests::default();
    let session = InspectorSessionId(1);
    let context = requests
        .begin_inspector(
            session,
            InspectorCommand::GetStatusSnapshot,
            Duration::from_secs(1),
        )
        .unwrap();
    let next = requests.next_document().unwrap();
    let cancelled = requests.navigate(next).unwrap();
    assert_eq!(cancelled.len(), 1);
    assert!(matches!(cancelled[0].kind, RequestKind::Inspector { .. }));
    assert!(
        requests.take(context).is_none(),
        "a reply for the old document is ignored"
    );
    assert_eq!(requests.inspector_pending(), 0);
}

#[test]
fn deadlines_expire_inspector_requests() {
    let mut requests = DocumentRequests::default();
    let session = InspectorSessionId(1);
    requests
        .begin_inspector(
            session,
            InspectorCommand::GetStatusSnapshot,
            Duration::from_millis(0),
        )
        .unwrap();
    let expired = requests.expired(std::time::Instant::now() + Duration::from_millis(1));
    assert_eq!(expired.len(), 1);
}

#[test]
fn protocol_errors_map_to_inspector_errors() {
    for (from, to) in [
        (ErrorCode::StaleDocument, InspectorErrorCode::StaleDocument),
        (ErrorCode::Timeout, InspectorErrorCode::Timeout),
        (ErrorCode::Cancelled, InspectorErrorCode::Cancelled),
        (ErrorCode::ResourceLimit, InspectorErrorCode::Busy),
    ] {
        assert_eq!(
            InspectorError::from(&ProtocolError::new(from, "x")).code,
            to
        );
    }
}

#[test]
fn snapshots_are_bounded_and_truncate_on_character_boundaries() {
    let InspectorResponse::Status(mut s) = snapshot(target(1, 1)) else {
        unreachable!()
    };
    s.url = "é".repeat(MAX_FIELD);
    s.title = "日".repeat(MAX_FIELD);
    let s = s.bounded();
    assert!(s.is_bounded());
    assert!(s.url.is_char_boundary(s.url.len()) && !s.url.is_empty());
    let mut bytes = Vec::new();
    crate::write(
        &mut bytes,
        &Event::InspectorResponse {
            context: RequestContext {
                document: DocumentId(0),
                revision: DocumentRevision(0),
                request: RequestId(1),
            },
            session: InspectorSessionId(1),
            result: Ok(InspectorResponse::Status(Box::new(s))),
        },
    )
    .unwrap();
    assert!(
        bytes.len() < 16 * 1024,
        "a snapshot is a small, bounded message"
    );
}

#[test]
fn identity_wrapping_still_rejects_a_foreign_inspector_command() {
    let mine = identity(1, 1);
    let mut theirs = mine;
    theirs.process = ProcessSessionId([9; 16]);
    let packet = theirs.command(Command::InspectorDetach {
        session: InspectorSessionId(1),
    });
    assert!(mine.receive_command(packet).is_err());
    let ok = mine.command(Command::InspectorDetach {
        session: InspectorSessionId(1),
    });
    assert!(matches!(
        mine.receive_command(ok),
        Ok(Command::InspectorDetach { .. })
    ));
}

#[test]
fn protocol_inspector_module_names_no_engine_types() {
    let sources = [
        include_str!("inspector.rs"),
        include_str!("inspector/session.rs"),
    ];
    let forbidden = [
        "wega_dom",
        "blitz",
        "stylo",
        "v8::",
        "deno",
        "servo",
        "html5ever",
        "skia",
    ];
    for (index, source) in sources.iter().enumerate() {
        // Ignore this file's own doc comment mentioning the policy.
        let code: String = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        for word in forbidden {
            assert!(!code.contains(word), "source {index} mentions `{word}`");
        }
    }
}
