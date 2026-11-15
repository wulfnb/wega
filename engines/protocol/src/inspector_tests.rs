use super::policy;
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
        args: InspectorArgs::None,
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
    for forged in [
        "Eval",
        "RunScript",
        "Navigate",
        "attach",
        "evaluate",
        "RunCommand",
        "",
    ] {
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
    assert_eq!(crate::VERSION, 25);
    assert!(!crate::capabilities(21).contains(&crate::Capability::Inspector));
    assert!(crate::capabilities(22).contains(&crate::Capability::Inspector));
    assert!(
        crate::supports_version(21) && crate::supports_version(22) && crate::supports_version(23)
    );
    assert!(crate::supports_version(24));
    assert!(crate::supports_version(25));
    assert!(!crate::supports_version(26));
    assert_eq!(INSPECTOR_VERSION, 4);
    assert!(supports_inspector_version(1) && supports_inspector_version(2));
    assert!(supports_inspector_version(3));
    assert!(!supports_inspector_version(5) && !supports_inspector_version(0));
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
            mutation_epoch: 0,
            layout_pending: false,
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

// ---- D2: console, evaluation, grants -------------------------------------------------

fn tuple() -> crate::authority::Principal {
    crate::authority::Principal {
        origin: crate::authority::Origin::Tuple("https://e.test".into()),
        top_level_site: None,
    }
}
fn opaque() -> crate::authority::Principal {
    crate::authority::Principal {
        origin: crate::authority::Origin::Opaque,
        top_level_site: None,
    }
}
fn scope() -> policy::GrantScope {
    policy::GrantScope {
        origin: Some("https://e.test".into()),
        opaque_override: false,
    }
}
fn token(byte: u8) -> GrantToken {
    GrantToken([byte; 16])
}
fn stamp(document: u64, revision: u64) -> DocumentContext {
    DocumentContext {
        document: DocumentId(document),
        revision: DocumentRevision(revision),
    }
}
fn with_args(command: InspectorCommand, args: InspectorArgs) -> InspectorRequest {
    InspectorRequest {
        args,
        ..request(1, target(1, 1), command)
    }
}
fn message(seq: u64, text: &str) -> ConsoleMessage {
    ConsoleMessage {
        seq,
        timestamp_ms: 1,
        level: ConsoleLevel::Log,
        method: "log".into(),
        args: vec![Preview::simple(PreviewKind::String, text)],
        text: text.into(),
        source: None,
        exception: None,
    }
}
fn added(seq: u64, text: &str) -> InspectorEvent {
    InspectorEvent::ConsoleMessageAdded(Box::new(message(seq, text)))
}
fn live_console_host() -> (HostSessions, InspectorSessionId) {
    let mut host = HostSessions::new(target(1, 1));
    let (session, _) = host.open(&SessionIssuer::new()).unwrap();
    assert!(host.complete(
        session,
        InspectorCommand::EnableConsole,
        Ok(InspectorResponse::ConsoleEnabled {
            retained: 0,
            first_seq: 1
        })
    ));
    (host, session)
}

#[test]
fn arguments_must_match_the_command_and_stay_within_limits() {
    use InspectorCommand as C;
    let ok = |c, a| with_args(c, a).validate().is_ok();
    assert!(ok(C::EnableConsole, InspectorArgs::None));
    assert!(ok(
        C::GetConsoleHistory,
        InspectorArgs::History {
            after: 0,
            limit: 100
        }
    ));
    assert!(ok(
        C::GrantEvaluation,
        InspectorArgs::Grant {
            token: token(1),
            opaque_override: false,
            origin: None
        }
    ));
    assert!(ok(
        C::Evaluate,
        InspectorArgs::Evaluate {
            expression: "1+1".into(),
            grant: token(1)
        }
    ));
    // Mismatches.
    assert!(!ok(C::Evaluate, InspectorArgs::None));
    assert!(!ok(
        C::EnableConsole,
        InspectorArgs::Evaluate {
            expression: "x".into(),
            grant: token(1)
        }
    ));
    assert!(!ok(
        C::Attach,
        InspectorArgs::History { after: 0, limit: 1 }
    ));
    assert!(!ok(C::GetConsoleHistory, InspectorArgs::None));
    // Limits.
    assert!(!ok(
        C::GetConsoleHistory,
        InspectorArgs::History { after: 0, limit: 0 }
    ));
    assert!(!ok(
        C::GetConsoleHistory,
        InspectorArgs::History {
            after: 0,
            limit: MAX_HISTORY_PAGE + 1
        }
    ));
    assert!(!ok(
        C::GrantEvaluation,
        InspectorArgs::Grant {
            token: GrantToken([0; 16]),
            opaque_override: false,
            origin: None
        }
    ));
    assert!(!ok(
        C::Evaluate,
        InspectorArgs::Evaluate {
            expression: "x".repeat(MAX_EXPRESSION_BYTES + 1),
            grant: token(1)
        }
    ));
    assert!(!ok(
        C::Evaluate,
        InspectorArgs::Evaluate {
            expression: "   ".into(),
            grant: token(1)
        }
    ));
}

#[test]
fn console_commands_need_inspector_version_two_and_v1_requests_still_decode() {
    let mut old = with_args(InspectorCommand::EnableConsole, InspectorArgs::None);
    old.version = 1;
    assert_eq!(
        old.validate().unwrap_err().code,
        InspectorErrorCode::UnsupportedVersion
    );
    // A D1 payload without `args` (what a v22 browser would send) is still accepted.
    let legacy = serde_json::json!({
        "version": 1,
        "session": 4,
        "target": serde_json::to_value(target(1, 1)).unwrap(),
        "command": "GetStatusSnapshot",
    });
    assert_eq!(
        InspectorPayload(legacy).decode().unwrap().args,
        InspectorArgs::None
    );
    for command in ALL {
        let mut request = request(1, target(1, 1), command);
        request.version = 1;
        assert!(
            request.validate().is_ok(),
            "{command:?} must stay valid at version 1"
        );
    }
}

#[test]
fn only_evaluate_gets_the_long_deadline_and_payloads_can_be_peeked_without_decoding() {
    for command in [
        InspectorCommand::Attach,
        InspectorCommand::GetStatusSnapshot,
        InspectorCommand::EnableConsole,
        InspectorCommand::GrantEvaluation,
    ] {
        assert_eq!(command.timeout_ms(), INSPECTOR_TIMEOUT_MS);
    }
    assert_eq!(InspectorCommand::Evaluate.timeout_ms(), EVALUATE_TIMEOUT_MS);
    const { assert!(EVALUATE_TIMEOUT_MS > crate::EVALUATION_TIMEOUT_MS) };
    let payload = InspectorPayload::new(&with_args(
        InspectorCommand::Evaluate,
        InspectorArgs::Evaluate {
            expression: "1".into(),
            grant: token(2),
        },
    ));
    assert_eq!(payload.command_hint(), Some(InspectorCommand::Evaluate));
    assert_eq!(
        InspectorPayload(serde_json::json!({"command": "Nope"})).command_hint(),
        None
    );
    assert_eq!(InspectorPayload(serde_json::json!(5)).command_hint(), None);
}

#[test]
fn the_grant_token_never_appears_in_debug_output_and_compares_fully() {
    let secret = GrantToken([0xAB; 16]);
    let rendered = format!(
        "{secret:?} {:?}",
        with_args(
            InspectorCommand::GrantEvaluation,
            InspectorArgs::Grant {
                token: secret,
                opaque_override: false,
                origin: None
            },
        )
    );
    assert!(
        !rendered.contains("171") && !rendered.to_lowercase().contains("ab, ab"),
        "{rendered}"
    );
    assert_eq!(secret, GrantToken([0xAB; 16]));
    let mut other = [0xAB; 16];
    other[15] ^= 1;
    assert_ne!(secret, GrantToken(other));
    other = [0xAB; 16];
    other[0] ^= 0x80;
    assert_ne!(secret, GrantToken(other));
}

#[test]
fn content_grant_checks_session_token_and_document_independently() {
    let own = target(1, 1);
    let mut content = ContentSessions::default();
    content
        .authorize(&request(5, own, InspectorCommand::Attach), own)
        .unwrap();
    let session = InspectorSessionId(5);
    let doc = stamp(2, 3);
    // Nothing granted.
    assert_eq!(
        content
            .check_grant(session, doc, token(9))
            .unwrap_err()
            .code,
        InspectorErrorCode::Unauthorized
    );
    content.store_grant(session, doc, token(9));
    content.check_grant(session, doc, token(9)).unwrap();
    // Forged token.
    assert_eq!(
        content
            .check_grant(session, doc, token(8))
            .unwrap_err()
            .code,
        InspectorErrorCode::Unauthorized
    );
    // Another document (navigation) or revision.
    assert_eq!(
        content
            .check_grant(session, stamp(3, 3), token(9))
            .unwrap_err()
            .code,
        InspectorErrorCode::StaleDocument
    );
    assert_eq!(
        content
            .check_grant(session, stamp(2, 4), token(9))
            .unwrap_err()
            .code,
        InspectorErrorCode::StaleDocument
    );
    // Another session id.
    assert_eq!(
        content
            .check_grant(InspectorSessionId(6), doc, token(9))
            .unwrap_err()
            .code,
        InspectorErrorCode::StaleSession
    );
    // A grant cannot be stored for a session that is not live, or with an empty token.
    let mut fresh = ContentSessions::default();
    fresh.store_grant(session, doc, token(9));
    assert!(!fresh.has_grant());
    content.store_grant(session, doc, GrantToken([0; 16]));
    content.check_grant(session, doc, token(9)).unwrap();
}

#[test]
fn content_grant_dies_with_detach_and_with_a_superseding_attach() {
    let own = target(1, 1);
    let doc = stamp(0, 1);
    let mut content = ContentSessions::default();
    content
        .authorize(&request(5, own, InspectorCommand::Attach), own)
        .unwrap();
    content.store_grant(InspectorSessionId(5), doc, token(1));
    content
        .authorize(&request(8, own, InspectorCommand::Attach), own)
        .unwrap();
    assert!(
        !content.has_grant(),
        "a newer session never inherits authority"
    );
    assert!(
        content
            .check_grant(InspectorSessionId(5), doc, token(1))
            .is_err()
    );
    content.store_grant(InspectorSessionId(8), doc, token(2));
    assert!(content.detach(InspectorSessionId(8)));
    assert!(!content.has_grant());
    content.clear_grant();
}

#[test]
fn host_grant_is_bound_to_session_and_document_and_cleared_on_every_boundary() {
    let issuer = SessionIssuer::new();
    let mine = target(1, 1);
    let mut host = HostSessions::new(mine);
    let (a, _) = host.open(&issuer).unwrap();
    let doc = stamp(1, 1);
    assert!(
        host.install_grant(InspectorSessionId(77), doc, token(3), tuple(), scope())
            .is_err(),
        "not live"
    );
    assert!(
        host.install_grant(a, doc, GrantToken([0; 16]), tuple(), scope())
            .is_err()
    );
    host.install_grant(a, doc, token(3), tuple(), scope())
        .unwrap();
    assert_eq!(host.grant_for(a, doc, &tuple()), Some(token(3)));
    assert_eq!(
        host.grant_for(a, stamp(1, 2), &tuple()),
        None,
        "another revision"
    );
    assert_eq!(
        host.grant_for(a, stamp(2, 1), &tuple()),
        None,
        "another document"
    );
    assert_eq!(
        host.grant_for(InspectorSessionId(a.0 + 1), doc, &tuple()),
        None
    );
    assert_eq!(host.grant_state(a), Some((doc, false)));
    // Navigation.
    host.document_changed();
    assert_eq!(host.grant_for(a, doc, &tuple()), None);
    // Supersede.
    host.install_grant(a, doc, token(4), tuple(), scope())
        .unwrap();
    let (b, _) = host.open(&issuer).unwrap();
    assert_eq!(host.grant_for(a, doc, &tuple()), None);
    assert_eq!(host.grant_for(b, doc, &tuple()), None);
    // Revoke.
    host.install_grant(b, doc, token(5), tuple(), scope())
        .unwrap();
    assert!(host.revoke(b));
    assert_eq!(host.grant_for(b, doc, &tuple()), None);
    // Process death.
    let (c, _) = host.open(&issuer).unwrap();
    host.install_grant(c, doc, token(6), tuple(), scope())
        .unwrap();
    host.close();
    assert_eq!(host.grant_for(c, doc, &tuple()), None);
    // An explicit revoke response and a refused grant both clear it.
    let mut host = HostSessions::new(mine);
    let (s, _) = host.open(&issuer).unwrap();
    host.install_grant(s, doc, token(7), tuple(), scope())
        .unwrap();
    host.complete(
        s,
        InspectorCommand::RevokeEvaluation,
        Ok(InspectorResponse::EvaluationRevoked),
    );
    assert_eq!(host.grant_for(s, doc, &tuple()), None);
    host.install_grant(s, doc, token(7), tuple(), scope())
        .unwrap();
    host.complete(
        s,
        InspectorCommand::GrantEvaluation,
        Err(InspectorError::new(InspectorErrorCode::Unauthorized, "no")),
    );
    assert_eq!(host.grant_for(s, doc, &tuple()), None);
}

#[test]
fn console_events_need_an_enabled_live_session_and_respect_the_sequence() {
    let mut host = HostSessions::new(target(1, 1));
    let (session, _) = host.open(&SessionIssuer::new()).unwrap();
    assert!(
        !host.event(session, 1, added(1, "a")),
        "console not enabled"
    );
    host.complete(
        session,
        InspectorCommand::EnableConsole,
        Ok(InspectorResponse::ConsoleEnabled {
            retained: 0,
            first_seq: 1,
        }),
    );
    assert!(host.event(session, 1, added(1, "a")));
    assert!(!host.event(session, 1, added(1, "replay")));
    assert!(!host.event(InspectorSessionId(session.0 + 9), 2, added(2, "foreign")));
    host.complete(
        session,
        InspectorCommand::DisableConsole,
        Ok(InspectorResponse::ConsoleDisabled),
    );
    assert!(!host.event(session, 3, added(3, "after disable")));
    assert_eq!(host.take_console(session).len(), 1);
    host.revoke(session);
    assert!(host.take_console(session).is_empty());
}

#[test]
fn the_console_queue_is_bounded_by_entries_and_bytes_and_reports_loss_first() {
    let (mut host, session) = live_console_host();
    for seq in 1..=(CONSOLE_QUEUE_ENTRIES as u64 * 10) {
        host.event(session, seq, added(seq, "x"));
        let (entries, bytes) = host.console_queued();
        assert!(entries <= CONSOLE_QUEUE_ENTRIES && bytes <= CONSOLE_QUEUE_BYTES);
    }
    let events = host.take_console(session);
    let InspectorEvent::ConsoleMessagesDropped { count } = events[0] else {
        panic!("{:?}", events[0])
    };
    assert_eq!(
        count as usize + CONSOLE_QUEUE_ENTRIES,
        CONSOLE_QUEUE_ENTRIES * 10
    );
    assert_eq!(events.len(), CONSOLE_QUEUE_ENTRIES + 1);
    assert_eq!(host.console_queued(), (0, 0));
    // Byte budget: large messages evict by size before the entry cap is reached.
    let big = "y".repeat(3000);
    for seq in 5000..5200u64 {
        host.event(session, seq, added(seq, &big));
        assert!(host.console_queued().1 <= CONSOLE_QUEUE_BYTES);
    }
    assert!(
        host.console_queued().0 < 100,
        "bytes must bound the queue before entries do"
    );
    assert!(matches!(
        host.take_console(session)[0],
        InspectorEvent::ConsoleMessagesDropped { .. }
    ));
}

#[test]
fn oversized_or_unbounded_console_events_are_dropped_not_stored() {
    let (mut host, session) = live_console_host();
    let mut huge = message(1, "x");
    huge.args = (0..(MAX_ARGS + 1))
        .map(|_| Preview::simple(PreviewKind::Null, "null"))
        .collect();
    assert!(!host.event(
        session,
        1,
        InspectorEvent::ConsoleMessageAdded(Box::new(huge))
    ));
    let mut deep = Preview::simple(PreviewKind::Object, "{}");
    for _ in 0..(MAX_PREVIEW_DEPTH + 2) {
        deep = Preview {
            props: vec![PreviewProp {
                key: "k".into(),
                value: deep,
            }],
            ..Preview::simple(PreviewKind::Object, "{}")
        };
    }
    assert!(!deep.is_bounded());
    let mut message = message(2, "x");
    message.args = vec![deep];
    assert!(!host.event(
        session,
        2,
        InspectorEvent::ExceptionThrown(Box::new(message))
    ));
    assert_eq!(host.console_queued().0, 0);
    // The loss is visible.
    assert!(matches!(
        host.take_console(session)[0],
        InspectorEvent::ConsoleMessagesDropped { count: 2 }
    ));
}

#[test]
fn previews_enforce_depth_width_node_and_text_budgets() {
    let leaf = |n: usize| Preview::simple(PreviewKind::String, "x".repeat(n));
    assert!(leaf(MAX_PREVIEW_TEXT).is_bounded());
    assert!(!leaf(MAX_PREVIEW_TEXT * 4 + 1).is_bounded());
    let wide = |n: usize| Preview {
        props: (0..n)
            .map(|i| PreviewProp {
                key: i.to_string(),
                value: leaf(1),
            })
            .collect(),
        ..Preview::simple(PreviewKind::Object, "{}")
    };
    assert!(wide(MAX_PREVIEW_PROPS).is_bounded());
    assert!(!wide(MAX_PREVIEW_PROPS + 1).is_bounded());
    // Node budget: 8 x 8 = 64 leaves + roots exceeds 64 nodes.
    let many = Preview {
        props: (0..8)
            .map(|i| PreviewProp {
                key: i.to_string(),
                value: wide(8),
            })
            .collect(),
        ..Preview::simple(PreviewKind::Object, "{}")
    };
    assert!(!many.is_bounded());
}

#[test]
fn console_and_evaluation_messages_round_trip_through_the_wire() {
    let mut bytes = Vec::new();
    let outcome = EvalOutcome::Exception {
        details: ExceptionDetails {
            kind: ExceptionKind::Evaluation,
            name: "TypeError".into(),
            message: "x is not a function".into(),
            stack: Some("TypeError: x\n    at <anonymous>:1:1".into()),
            source: Some(SourceLocation {
                url: "https://e.test/a.js".into(),
                line: 3,
                column: 9,
            }),
        },
    };
    crate::write(
        &mut bytes,
        &Event::InspectorResponse {
            context: RequestContext {
                document: DocumentId(1),
                revision: DocumentRevision(1),
                request: RequestId(2),
            },
            session: InspectorSessionId(1),
            result: Ok(InspectorResponse::Evaluated(Box::new(outcome.clone()))),
        },
    )
    .unwrap();
    let Event::InspectorResponse {
        result: Ok(InspectorResponse::Evaluated(back)),
        ..
    } = crate::read::<Event>(&mut bytes.as_slice())
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(*back, outcome);
    assert!(back.is_bounded());
    let mut exception = message(4, "boom");
    exception.exception = outcome_details(&outcome);
    let event = InspectorEvent::ExceptionThrown(Box::new(exception));
    let mut bytes = Vec::new();
    crate::write(
        &mut bytes,
        &Event::InspectorEvent {
            context: stamp(1, 1),
            session: InspectorSessionId(1),
            seq: 4,
            event: event.clone(),
        },
    )
    .unwrap();
    let Event::InspectorEvent { event: back, .. } = crate::read::<Event>(&mut bytes.as_slice())
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(back, event);
}

fn outcome_details(outcome: &EvalOutcome) -> Option<ExceptionDetails> {
    match outcome {
        EvalOutcome::Exception { details } => Some(details.clone()),
        EvalOutcome::Value { .. } => None,
    }
}

#[test]
fn an_unbounded_evaluation_result_or_history_page_is_refused() {
    let (mut host, session) = live_console_host();
    let mut deep = Preview::simple(PreviewKind::Object, "{}");
    for _ in 0..6 {
        deep = Preview {
            props: vec![PreviewProp {
                key: "k".into(),
                value: deep,
            }],
            ..Preview::simple(PreviewKind::Object, "{}")
        };
    }
    assert!(!host.complete(
        session,
        InspectorCommand::Evaluate,
        Ok(InspectorResponse::Evaluated(Box::new(EvalOutcome::Value {
            preview: deep
        })))
    ));
    let mut bad = message(1, "x");
    bad.method = "m".repeat(100);
    assert!(!host.complete(
        session,
        InspectorCommand::GetConsoleHistory,
        Ok(InspectorResponse::ConsoleHistory {
            messages: vec![bad],
            next_seq: 2,
            dropped: 0
        })
    ));
    assert!(host.complete(
        session,
        InspectorCommand::GetConsoleHistory,
        Ok(InspectorResponse::ConsoleHistory {
            messages: vec![message(1, "ok")],
            next_seq: 2,
            dropped: 0
        })
    ));
}

#[test]
fn context_and_drop_markers_are_console_stream_events() {
    for event in [
        InspectorEvent::ExecutionContextCreated {
            document: stamp(1, 1),
        },
        InspectorEvent::ExecutionContextDestroyed {
            document: stamp(1, 1),
        },
        InspectorEvent::ConsoleMessagesDropped { count: 3 },
    ] {
        assert!(event.is_console());
    }
    assert!(!InspectorEvent::LoadingChanged { loading: true }.is_console());
    assert!(
        !InspectorEvent::DocumentChanged {
            document: stamp(1, 1)
        }
        .is_console()
    );
}

#[test]
fn printing_a_command_or_payload_never_reveals_a_grant_token_or_expression() {
    let secret = GrantToken([0xC3; 16]);
    let request = with_args(
        InspectorCommand::Evaluate,
        InspectorArgs::Evaluate {
            expression: "document.cookie".into(),
            grant: secret,
        },
    );
    let payload = InspectorPayload::new(&request);
    let packet = Command::Inspector {
        context: RequestContext {
            document: DocumentId(0),
            revision: DocumentRevision(0),
            request: RequestId(1),
        },
        request: payload.clone(),
    };
    for rendered in [
        format!("{payload:?}"),
        format!("{packet:?}"),
        format!("{request:?}"),
        format!("{secret:?}"),
    ] {
        assert!(!rendered.contains("195"), "{rendered}");
        assert!(
            !rendered.contains("document.cookie") || rendered.contains("Evaluate {"),
            "{rendered}"
        );
    }
    assert!(!format!("{payload:?}").contains("cookie"));
    assert!(!format!("{packet:?}").contains("cookie"));
    // The grant request is just as quiet.
    let grant = InspectorPayload::new(&with_args(
        InspectorCommand::GrantEvaluation,
        InspectorArgs::Grant {
            token: secret,
            opaque_override: true,
            origin: None,
        },
    ));
    assert!(!format!("{grant:?}").contains("195") && !format!("{grant:?}").contains("true"));
}

// ---- D3.0: principal-based evaluation policy ------------------------------------------

fn policy_code<T: std::fmt::Debug>(result: Result<T, InspectorError>) -> InspectorErrorCode {
    result.unwrap_err().code
}

#[test]
fn the_browser_decides_from_the_committed_principal_only() {
    // Established tuple origin: allowed, scoped to that origin, a stray override is dropped.
    for stray in [false, true] {
        let scope = policy::authorise(Some(&tuple()), stray).unwrap();
        assert_eq!(scope.origin.as_deref(), Some("https://e.test"));
        assert!(
            !scope.opaque_override,
            "an override is meaningless for a tuple origin"
        );
    }
    // Opaque (file:, data:, about:, uncorroborated commit, internal): refused without the
    // user's explicit override, allowed (and recorded as an override) with it.
    assert_eq!(
        policy_code(policy::authorise(Some(&opaque()), false)),
        InspectorErrorCode::PolicyDenied
    );
    let scope = policy::authorise(Some(&opaque()), true).unwrap();
    assert_eq!((scope.origin, scope.opaque_override), (None, true));
    // No principal (not committed, replaced, unbound): refused even with the override.
    for flag in [false, true] {
        assert_eq!(
            policy_code(policy::authorise(None, flag)),
            InspectorErrorCode::PolicyDenied
        );
    }
}

#[test]
fn the_engine_checks_consistency_with_its_own_document_and_never_decides() {
    let ok = |origin: Option<&str>, over, engine: Option<&str>| {
        policy::verify_scope(origin, over, engine)
    };
    // A claim matching the engine's document.
    assert!(ok(Some("https://e.test"), false, Some("https://e.test")).is_ok());
    assert!(
        ok(
            Some("http://127.0.0.1:8080"),
            false,
            Some("http://127.0.0.1:8080")
        )
        .is_ok()
    );
    // A forged or stale claim: another origin, or a tuple claim for a document with none.
    assert_eq!(
        policy_code(ok(Some("https://evil.test"), false, Some("https://e.test"))),
        InspectorErrorCode::PolicyDenied
    );
    assert_eq!(
        policy_code(ok(Some("https://e.test"), true, None)),
        InspectorErrorCode::PolicyDenied
    );
    assert_eq!(
        policy_code(ok(
            Some("https://e.test:8443"),
            false,
            Some("https://e.test")
        )),
        InspectorErrorCode::PolicyDenied
    );
    // Malformed claims are invalid payloads, not policy decisions.
    for bad in [
        "",
        "e.test",
        "https://",
        "https://e.test/path",
        "https://e.test?x",
        "https://e.test#x",
        "https://e t",
        "://x",
        "https://e.test\n",
    ] {
        assert_eq!(
            policy_code(ok(Some(bad), false, Some(bad))),
            InspectorErrorCode::InvalidPayload,
            "{bad:?}"
        );
    }
    assert_eq!(
        policy_code(ok(Some(&"a".repeat(3000)), false, None)),
        InspectorErrorCode::InvalidPayload
    );
    // No origin: needs the override. The browser's principal may be stricter than the engine's
    // URL (an https page the broker could not corroborate), and the override is honoured then.
    assert_eq!(
        policy_code(ok(None, false, None)),
        InspectorErrorCode::PolicyDenied
    );
    assert_eq!(
        policy_code(ok(None, false, Some("https://e.test"))),
        InspectorErrorCode::PolicyDenied
    );
    assert!(ok(None, true, None).is_ok());
    assert!(ok(None, true, Some("https://e.test")).is_ok());
}

#[test]
fn origin_shapes_are_validated_in_the_grant_arguments() {
    let grant = |origin: Option<&str>| {
        with_args(
            InspectorCommand::GrantEvaluation,
            InspectorArgs::Grant {
                token: token(1),
                opaque_override: origin.is_none(),
                origin: origin.map(str::to_string),
            },
        )
        .validate()
    };
    assert!(grant(None).is_ok() && grant(Some("https://e.test")).is_ok());
    assert_eq!(
        grant(Some("https://e.test/x")).unwrap_err().code,
        InspectorErrorCode::InvalidPayload
    );
    assert_eq!(
        grant(Some("")).unwrap_err().code,
        InspectorErrorCode::InvalidPayload
    );
}

#[test]
fn a_grant_belongs_to_the_principal_it_was_authorised_for() {
    let issuer = SessionIssuer::new();
    let mut host = HostSessions::new(target(1, 1));
    let (session, _) = host.open(&issuer).unwrap();
    let doc = stamp(1, 1);
    host.install_grant(session, doc, token(2), tuple(), scope())
        .unwrap();
    assert_eq!(host.grant_for(session, doc, &tuple()), Some(token(2)));
    // The same document context re-bound to another principal (or to opaque): no token.
    let other = crate::authority::Principal {
        origin: crate::authority::Origin::Tuple("https://other.test".into()),
        top_level_site: None,
    };
    assert_eq!(host.grant_for(session, doc, &other), None);
    assert_eq!(host.grant_for(session, doc, &opaque()), None);
    // A different top-level site (partition) is a different principal too.
    let mut partitioned = tuple();
    partitioned.top_level_site = Some(("https".into(), "e.test".into()));
    assert_eq!(host.grant_for(session, doc, &partitioned), None);
    // The override is reported for the indicator, and only for an opaque grant.
    assert_eq!(host.grant_state(session), Some((doc, false)));
    host.install_grant(
        session,
        doc,
        token(3),
        opaque(),
        policy::authorise(Some(&opaque()), true).unwrap(),
    )
    .unwrap();
    assert_eq!(host.grant_state(session), Some((doc, true)));
    assert_eq!(host.grant_for(session, doc, &opaque()), Some(token(3)));
    assert_eq!(host.grant_for(session, doc, &tuple()), None);
}

#[test]
fn grants_of_different_tabs_never_cross() {
    let issuer = SessionIssuer::new();
    let (mut a, mut b) = (
        HostSessions::new(target(1, 1)),
        HostSessions::new(target(2, 2)),
    );
    let (sa, _) = a.open(&issuer).unwrap();
    let (sb, _) = b.open(&issuer).unwrap();
    let doc = stamp(1, 1);
    a.install_grant(sa, doc, token(9), tuple(), scope())
        .unwrap();
    // B has no grant; A's session id and document stamp do not unlock anything in B.
    assert_eq!(b.grant_for(sb, doc, &tuple()), None);
    assert_eq!(b.grant_for(sa, doc, &tuple()), None);
    assert!(
        b.install_grant(sa, doc, token(9), tuple(), scope())
            .is_err(),
        "A's session is not live in B"
    );
    assert_eq!(a.grant_for(sb, doc, &tuple()), None);
}

#[test]
fn policy_denied_survives_the_wire() {
    let error = InspectorError::new(InspectorErrorCode::PolicyDenied, "no");
    let json = serde_json::to_string(&error).unwrap();
    assert_eq!(
        serde_json::from_str::<InspectorError>(&json).unwrap(),
        error
    );
}
