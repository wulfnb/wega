//! Console, evaluation and grants over the real pipe against a real `wega-engine` process.
mod support;
use protocol::{
    Command, Event,
    inspector::{
        CONSOLE_BATCH_ENTRIES, ConsoleLevel, ConsoleMessage, EvalOutcome, ExceptionKind,
        GrantToken, InspectorArgs, InspectorCommand, InspectorEvent, InspectorPayload,
        InspectorRequest, InspectorResponse, InspectorSessionId, MAX_MESSAGE_BYTES, PreviewKind,
        TargetId, session::HostSessions,
    },
};
use std::time::{Duration, Instant};
use support::inspector_worker::Worker;
use wega_protocol as protocol;

const TOKEN: GrantToken = GrantToken([0x5A; 16]);
const OTHER: GrantToken = GrantToken([0xA5; 16]);
type Detail = Result<InspectorResponse, (String, String)>;

fn load(worker: &mut Worker, serial: u64, html: &str) {
    let url = format!(
        "data:text/html;charset=utf-8,{}",
        html.replace('%', "%25").replace('#', "%23")
    );
    worker.navigate(serial, &url);
    worker.until(|e| matches!(e, Event::State { serial: s, loading: false, .. } if *s == serial));
}
fn enable(worker: &mut Worker, session: InspectorSessionId) {
    assert!(matches!(
        worker.call_with(
            session,
            InspectorCommand::EnableConsole,
            InspectorArgs::None
        ),
        Ok(InspectorResponse::ConsoleEnabled { .. })
    ));
}
fn grant(
    worker: &mut Worker,
    session: InspectorSessionId,
    token: GrantToken,
    opaque: bool,
) -> Detail {
    worker.call_detail(
        session,
        InspectorCommand::GrantEvaluation,
        InspectorArgs::Grant {
            token,
            opaque_override: opaque,
            origin: None,
        },
    )
}
fn eval(
    worker: &mut Worker,
    session: InspectorSessionId,
    token: GrantToken,
    expression: &str,
) -> Detail {
    worker.call_detail(
        session,
        InspectorCommand::Evaluate,
        InspectorArgs::Evaluate {
            expression: expression.into(),
            grant: token,
        },
    )
}
fn outcome(result: Detail) -> EvalOutcome {
    match result {
        Ok(InspectorResponse::Evaluated(outcome)) => *outcome,
        other => panic!("expected an evaluation outcome, got {other:?}"),
    }
}
fn text_of(result: Detail) -> String {
    match outcome(result) {
        EvalOutcome::Value { preview } => preview.text,
        EvalOutcome::Exception { details } => panic!("threw {details:?}"),
    }
}
fn code(result: &Detail) -> String {
    result
        .as_ref()
        .err()
        .map(|e| e.0.clone())
        .unwrap_or_else(|| panic!("expected a refusal, got {result:?}"))
}
fn history(worker: &mut Worker, session: InspectorSessionId) -> Vec<ConsoleMessage> {
    let mut all = Vec::new();
    let mut after = 0;
    loop {
        let Ok(InspectorResponse::ConsoleHistory {
            messages, next_seq, ..
        }) = worker.call_with(
            session,
            InspectorCommand::GetConsoleHistory,
            InspectorArgs::History { after, limit: 100 },
        )
        else {
            panic!("history failed")
        };
        if messages.is_empty() {
            return all;
        }
        after = next_seq;
        all.extend(messages);
    }
}
fn console_events(worker: &Worker) -> Vec<ConsoleMessage> {
    worker
        .take_seen()
        .into_iter()
        .filter_map(|(_, _, e)| match e {
            InspectorEvent::ConsoleMessageAdded(m) | InspectorEvent::ExceptionThrown(m) => Some(*m),
            _ => None,
        })
        .collect()
}

#[test]
fn every_console_level_arrives_with_arguments_source_and_ordering() {
    let mut worker = Worker::start();
    let session = worker.attach();
    enable(&mut worker, session);
    worker.evaluate("console.log('a',1,{x:[2]});console.info('i');console.warn('w');console.error('e',new Error('boom'));console.debug('d');true").unwrap();
    worker.pump(Duration::from_millis(300));
    let got = console_events(&worker);
    let methods: Vec<_> = got.iter().map(|m| (m.method.as_str(), m.level)).collect();
    assert_eq!(
        methods,
        [
            ("log", ConsoleLevel::Log),
            ("info", ConsoleLevel::Info),
            ("warn", ConsoleLevel::Warn),
            ("error", ConsoleLevel::Error),
            ("debug", ConsoleLevel::Debug)
        ]
    );
    assert_eq!(got[0].args.len(), 3, "multiple arguments");
    assert_eq!(got[0].args[2].props[0].key, "x");
    assert_eq!(got[3].args[1].kind, PreviewKind::Error);
    assert_eq!(got[3].args[1].text, "Error: boom");
    assert!(got.windows(2).all(|w| w[0].seq < w[1].seq));
    assert!(
        got.iter()
            .all(|m| m.source.as_ref().is_some_and(|s| s.line >= 1)),
        "{got:?}"
    );
    // The history command returns the same entries.
    assert_eq!(history(&mut worker, session).len(), 5);
}

#[test]
fn output_logged_during_the_initial_load_and_before_devtools_opened_is_backfilled() {
    let mut worker = Worker::start();
    load(
        &mut worker,
        1,
        "<script>console.log('during load');console.warn('also during load');null.broken</script><p>x</p>",
    );
    worker
        .evaluate("console.log('after load, before DevTools')")
        .unwrap();
    let session = worker.attach();
    worker.pump(Duration::from_millis(200));
    assert!(
        console_events(&worker).is_empty(),
        "nothing flows before the console is enabled"
    );
    enable(&mut worker, session);
    worker.pump(Duration::from_millis(300));
    let got = console_events(&worker);
    let texts: Vec<_> = got.iter().map(|m| m.text.as_str()).collect();
    assert!(
        texts.contains(&"during load") && texts.contains(&"also during load"),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.starts_with("after load")),
        "{texts:?}"
    );
    let exception = got
        .iter()
        .find_map(|m| m.exception.as_ref())
        .expect("the uncaught error");
    assert_eq!(exception.kind, ExceptionKind::Uncaught);
    assert_eq!(exception.name, "TypeError");
    assert!(exception.message.contains("null"), "{exception:?}");
    let source = exception.source.as_ref().expect("source");
    assert!(source.line >= 1 && !source.url.is_empty());
}

#[test]
fn uncaught_exceptions_from_callbacks_and_rejections_carry_kind_stack_and_location() {
    let mut worker = Worker::start();
    let session = worker.attach();
    enable(&mut worker, session);
    load(
        &mut worker,
        1,
        "<script>setTimeout(function later(){throw new RangeError('from timer')},0);Promise.reject(new Error('unhandled'))</script>",
    );
    worker.pump(Duration::from_millis(500));
    let got = console_events(&worker);
    let timer = got
        .iter()
        .filter_map(|m| m.exception.as_ref())
        .find(|e| e.message.contains("from timer"))
        .expect("timer error");
    assert_eq!(
        (timer.kind, timer.name.as_str()),
        (ExceptionKind::Uncaught, "RangeError")
    );
    assert!(
        timer.stack.as_deref().unwrap_or("").contains("later"),
        "{timer:?}"
    );
    assert!(timer.source.as_ref().unwrap().line >= 1);
    let rejection = got
        .iter()
        .filter_map(|m| m.exception.as_ref())
        .find(|e| e.message.contains("unhandled"))
        .expect("rejection");
    assert_eq!(rejection.kind, ExceptionKind::UnhandledRejection);
    let events = worker.take_seen();
    assert!(events.is_empty());
}

#[test]
fn circular_and_oversized_values_arrive_bounded() {
    let mut worker = Worker::start();
    let session = worker.attach();
    enable(&mut worker, session);
    worker
        .evaluate("const c={};c.me=c;c.l=[c,c];console.log(c);console.log('x'.repeat(5e6));console.log(Object.fromEntries(Array.from({length:3000},(_, i)=>['k'+i,'v'.repeat(500)])));console.log(new Array(1e9),new Uint8Array(1e6));true")
        .unwrap();
    worker.pump(Duration::from_millis(400));
    let got = console_events(&worker);
    assert_eq!(got.len(), 4);
    assert_eq!(
        got[0].args[0]
            .props
            .iter()
            .find(|p| p.key == "me")
            .unwrap()
            .value
            .kind,
        PreviewKind::Circular
    );
    assert!(got[1].args[0].truncated && got[1].args[0].text.len() <= 256 * 4);
    assert!(got[2].args[0].props.len() <= 8 && got[2].args[0].more >= 2900);
    for m in &got {
        assert!(
            m.is_bounded() && m.approx_bytes() <= MAX_MESSAGE_BYTES,
            "{}",
            m.approx_bytes()
        );
    }
}

#[test]
fn history_and_delivery_are_bounded_and_losses_are_reported() {
    let mut worker = Worker::start();
    let session = worker.attach();
    worker
        .evaluate("for(let i=0;i<5000;i++)console.log('early',i,{i});true")
        .unwrap();
    // Nothing was listening; the ring kept only the newest entries, within budget.
    let kept = history(&mut worker, session);
    assert!(kept.len() <= 1000 && kept.len() >= 100, "{}", kept.len());
    assert_eq!(kept.last().unwrap().text, "early 4999 [object]");
    let Ok(InspectorResponse::ConsoleHistory { dropped, .. }) = worker.call_with(
        session,
        InspectorCommand::GetConsoleHistory,
        InspectorArgs::History { after: 0, limit: 1 },
    ) else {
        panic!()
    };
    assert_eq!(
        dropped as usize + kept.len(),
        5000,
        "the engine counts what it evicted"
    );
    // Enabling now delivers the survivors in order, exactly once, and nothing else.
    enable(&mut worker, session);
    worker.pump(Duration::from_millis(600));
    let seen = worker.take_seen();
    let delivered: Vec<u64> = seen
        .iter()
        .filter_map(|(_, _, e)| match e {
            InspectorEvent::ConsoleMessageAdded(m) => Some(m.seq),
            _ => None,
        })
        .collect();
    assert_eq!(delivered, kept.iter().map(|m| m.seq).collect::<Vec<_>>());
    // Session event sequence numbers are strictly increasing.
    assert!(seen.windows(2).all(|w| w[0].1 < w[1].1));
}

#[test]
fn a_console_flood_cannot_starve_commands_or_navigation() {
    let mut worker = Worker::start();
    let session = worker.attach();
    enable(&mut worker, session);
    // 10,000 messages in one task, then immediately ask for a snapshot and navigate.
    worker
        .evaluate("for(let i=0;i<10000;i++)console.log('flood',i);true")
        .unwrap();
    let started = Instant::now();
    worker.snapshot(session);
    let snapshot_latency = started.elapsed();
    let nav = Instant::now();
    load(&mut worker, 1, "<p>after the flood</p>");
    let navigation = nav.elapsed();
    assert!(
        snapshot_latency < Duration::from_secs(5),
        "{snapshot_latency:?}"
    );
    assert!(navigation < Duration::from_secs(10), "{navigation:?}");
    worker.pump(Duration::from_millis(500));
    let seen = worker.take_seen();
    let messages: Vec<_> = seen
        .iter()
        .filter_map(|(_, _, e)| match e {
            InspectorEvent::ConsoleMessageAdded(m) => Some(m.seq),
            _ => None,
        })
        .collect();
    let drops: u64 = seen
        .iter()
        .filter_map(|(_, _, e)| match e {
            InspectorEvent::ConsoleMessagesDropped { count } => Some(*count),
            _ => None,
        })
        .sum();
    assert!(
        messages.len() <= 1000 + CONSOLE_BATCH_ENTRIES,
        "{}",
        messages.len()
    );
    assert!(
        messages.windows(2).all(|w| w[0] < w[1]),
        "ordered, none repeated"
    );
    // Everything is accounted for: delivered, reported dropped, or evicted before delivery.
    assert!(messages.len() as u64 + drops <= 10_000);
    // The new document announced itself and the old history is gone.
    assert!(
        seen.iter()
            .any(|(_, _, e)| matches!(e, InspectorEvent::ExecutionContextCreated { .. }))
    );
    assert!(
        seen.iter()
            .any(|(_, _, e)| matches!(e, InspectorEvent::ExecutionContextDestroyed { .. }))
    );
    assert!(history(&mut worker, session).is_empty());
}

#[test]
fn a_browser_side_queue_stays_bounded_through_a_real_flood() {
    let mut worker = Worker::start();
    let mut host = HostSessions::new(worker.target());
    let (session, _) = host.open(&worker.issuer).unwrap();
    assert!(
        worker
            .call_with(session, InspectorCommand::Attach, InspectorArgs::None)
            .is_ok()
    );
    host.complete(
        session,
        InspectorCommand::EnableConsole,
        Ok(InspectorResponse::ConsoleEnabled {
            retained: 0,
            first_seq: 1,
        }),
    );
    assert!(
        worker
            .call_with(
                session,
                InspectorCommand::EnableConsole,
                InspectorArgs::None
            )
            .is_ok()
    );
    worker
        .evaluate("for(let i=0;i<20000;i++)console.log('flood',i,{i});true")
        .unwrap();
    let mut peak = (0, 0);
    let quiet = Duration::from_millis(400);
    while let Ok(event) = worker.events.recv_timeout(quiet) {
        if let Event::InspectorEvent {
            session: s,
            seq,
            event,
            ..
        } = event
        {
            host.event(s, seq, event);
            let q = host.console_queued();
            peak = (peak.0.max(q.0), peak.1.max(q.1));
        }
    }
    assert!(
        peak.0 <= protocol::inspector::CONSOLE_QUEUE_ENTRIES
            && peak.1 <= protocol::inspector::CONSOLE_QUEUE_BYTES,
        "{peak:?}"
    );
    assert!(host.console_queued().0 > 0);
}

#[test]
fn evaluation_is_denied_without_authorisation_and_never_runs() {
    let mut worker = Worker::start();
    let session = worker.attach();
    worker.evaluate("globalThis.ran=0;true").unwrap();
    let denied = eval(&mut worker, session, TOKEN, "globalThis.ran=1");
    assert_eq!(code(&denied), "Unauthorized");
    assert!(denied.unwrap_err().1.contains("not been authorised"));
    // Opening the console grants nothing.
    enable(&mut worker, session);
    assert_eq!(
        code(&eval(&mut worker, session, TOKEN, "globalThis.ran=2")),
        "Unauthorized"
    );
    assert_eq!(worker.evaluate("globalThis.ran"), Ok("0".into()));
}

#[test]
fn a_forged_principal_claim_is_refused_by_the_engine_in_a_real_process() {
    // about:blank has no tuple origin. A grant that *claims* one (a forged or stale principal
    // from the browser side) contradicts the engine's own document and is refused, and a refused
    // grant leaves nothing behind. The engine cannot authenticate the browser; it can refuse a
    // claim that is inconsistent with its own document, and it does.
    let mut worker = Worker::start();
    let session = worker.attach();
    for claim in [
        "https://example.test",
        "http://127.0.0.1:80",
        "https://evil.test:1",
    ] {
        let result = worker.call_detail(
            session,
            InspectorCommand::GrantEvaluation,
            InspectorArgs::Grant {
                token: TOKEN,
                opaque_override: false,
                origin: Some(claim.into()),
            },
        );
        assert_eq!(code(&result), "PolicyDenied", "{claim}: {result:?}");
        // Even with the user's override flag set, a *tuple* claim is checked against the document.
        let result = worker.call_detail(
            session,
            InspectorCommand::GrantEvaluation,
            InspectorArgs::Grant {
                token: TOKEN,
                opaque_override: true,
                origin: Some(claim.into()),
            },
        );
        assert_eq!(
            code(&result),
            "PolicyDenied",
            "{claim} + override: {result:?}"
        );
    }
    assert_eq!(
        code(&eval(&mut worker, session, TOKEN, "globalThis.pwned=1")),
        "Unauthorized"
    );
    assert_eq!(
        worker.evaluate("typeof globalThis.pwned"),
        Ok("\"undefined\"".into())
    );
    // Malformed claims never reach the policy.
    let result = worker.call_detail(
        session,
        InspectorCommand::GrantEvaluation,
        InspectorArgs::Grant {
            token: TOKEN,
            opaque_override: false,
            origin: Some("https://example.test/path".into()),
        },
    );
    assert_eq!(code(&result), "failed:InvalidRequest", "{result:?}");
    // Without any claim and without the override: denied by policy, not by luck.
    assert_eq!(
        code(&grant(&mut worker, session, TOKEN, false)),
        "PolicyDenied"
    );
    // The explicit override still works on this opaque document.
    assert!(matches!(
        grant(&mut worker, session, TOKEN, true),
        Ok(InspectorResponse::EvaluationGranted)
    ));
    assert_eq!(text_of(eval(&mut worker, session, TOKEN, "1+1")), "2");
}

#[test]
fn an_opaque_document_needs_the_explicit_override_and_then_evaluation_runs() {
    let mut worker = Worker::start(); // about:blank is opaque
    let session = worker.attach();
    let refused = grant(&mut worker, session, TOKEN, false);
    assert_eq!(code(&refused), "PolicyDenied");
    assert!(refused.unwrap_err().1.contains("opaque"));
    assert_eq!(
        code(&eval(&mut worker, session, TOKEN, "1")),
        "Unauthorized",
        "a refused grant stores nothing"
    );
    assert!(matches!(
        grant(&mut worker, session, TOKEN, true),
        Ok(InspectorResponse::EvaluationGranted)
    ));
    assert_eq!(text_of(eval(&mut worker, session, TOKEN, "6*7")), "42");
}

#[test]
fn evaluation_returns_previews_exceptions_and_runs_in_the_page_main_world() {
    let mut worker = Worker::start();
    let session = worker.attach();
    grant(&mut worker, session, TOKEN, true).unwrap();
    worker
        .evaluate("globalThis.shared='from the page'")
        .unwrap();
    assert_eq!(
        text_of(eval(&mut worker, session, TOKEN, "shared")),
        "from the page"
    );
    eval(&mut worker, session, TOKEN, "globalThis.fromDevtools=7").unwrap();
    assert_eq!(
        worker.evaluate("fromDevtools"),
        Ok("7".into()),
        "same world, same globals"
    );
    let EvalOutcome::Exception { details } = outcome(eval(
        &mut worker,
        session,
        TOKEN,
        "(function boom(){ undefinedFunction() })()",
    )) else {
        panic!()
    };
    assert_eq!(
        (details.kind, details.name.as_str()),
        (ExceptionKind::Evaluation, "ReferenceError")
    );
    assert!(details.stack.unwrap().contains("boom"));
    assert!(details.source.unwrap().line >= 1);
    // A hostile result is described without running it.
    worker.evaluate("globalThis.hits=0").unwrap();
    eval(
        &mut worker,
        session,
        TOKEN,
        "({get a(){hits++},toJSON(){hits++},toString(){hits++}})",
    )
    .unwrap();
    assert_eq!(worker.evaluate("hits"), Ok("0".into()));
    // Console output produced by an evaluation is captured like any other.
    enable(&mut worker, session);
    eval(
        &mut worker,
        session,
        TOKEN,
        "console.log('from evaluation')",
    )
    .unwrap();
    worker.pump(Duration::from_millis(300));
    assert!(
        console_events(&worker)
            .iter()
            .any(|m| m.text == "from evaluation")
    );
}

#[test]
fn a_forged_wrong_or_zero_grant_is_refused() {
    let mut worker = Worker::start();
    let session = worker.attach();
    grant(&mut worker, session, TOKEN, true).unwrap();
    for forged in [
        OTHER,
        GrantToken([0; 16]),
        GrantToken(
            [0x5A; 15]
                .iter()
                .copied()
                .chain([0])
                .collect::<Vec<u8>>()
                .try_into()
                .unwrap(),
        ),
    ] {
        let result = eval(&mut worker, session, forged, "globalThis.pwned=1");
        assert!(
            matches!(code(&result).as_str(), "Unauthorized" | "InvalidPayload"),
            "{result:?}"
        );
    }
    assert_eq!(
        worker.evaluate("typeof globalThis.pwned"),
        Ok("\"undefined\"".into())
    );
    // A guessed session id gets nothing, with or without the right token.
    let guessed = InspectorSessionId(session.0 + 100);
    assert!(eval(&mut worker, guessed, TOKEN, "globalThis.pwned=2").is_err());
    assert_eq!(
        worker.evaluate("typeof globalThis.pwned"),
        Ok("\"undefined\"".into())
    );
}

#[test]
fn navigation_revokes_the_grant_and_a_late_evaluate_is_rejected() {
    let mut worker = Worker::start();
    let session = worker.attach();
    grant(&mut worker, session, TOKEN, true).unwrap();
    assert_eq!(text_of(eval(&mut worker, session, TOKEN, "1")), "1");
    // A request stamped before the navigation is rejected as stale by the existing machinery.
    let old = worker.context();
    load(
        &mut worker,
        1,
        "<p>new document</p><script>globalThis.marker='new'</script>",
    );
    worker.request_with(
        old,
        session,
        InspectorCommand::Evaluate,
        InspectorArgs::Evaluate {
            expression: "globalThis.stale=1".into(),
            grant: TOKEN,
        },
    );
    assert_eq!(worker.reply(old), Err("failed:StaleDocument".into()));
    // Even with a fresh stamp the grant of the previous document is useless.
    let result = eval(&mut worker, session, TOKEN, "globalThis.stale=2");
    assert!(
        matches!(code(&result).as_str(), "Unauthorized" | "StaleDocument"),
        "{result:?}"
    );
    assert_eq!(
        worker.evaluate("typeof globalThis.stale"),
        Ok("\"undefined\"".into())
    );
    // The user can grant again for the new document.
    grant(&mut worker, session, OTHER, true).unwrap();
    assert_eq!(text_of(eval(&mut worker, session, OTHER, "marker")), "new");
}

#[test]
fn detach_supersede_and_revoke_each_end_evaluation() {
    let mut worker = Worker::start();
    let a = worker.attach();
    grant(&mut worker, a, TOKEN, true).unwrap();
    // Revoke.
    assert!(matches!(
        worker.call_with(a, InspectorCommand::RevokeEvaluation, InspectorArgs::None),
        Ok(InspectorResponse::EvaluationRevoked)
    ));
    assert_eq!(
        code(&eval(&mut worker, a, TOKEN, "globalThis.x=1")),
        "Unauthorized"
    );
    // Supersede: the new session never inherits it.
    grant(&mut worker, a, TOKEN, true).unwrap();
    let b = worker.attach();
    assert_eq!(
        code(&eval(&mut worker, a, TOKEN, "globalThis.x=2")),
        "StaleSession"
    );
    assert_eq!(
        code(&eval(&mut worker, b, TOKEN, "globalThis.x=3")),
        "Unauthorized"
    );
    // Detach.
    grant(&mut worker, b, TOKEN, true).unwrap();
    worker.send(Command::InspectorDetach { session: b });
    assert_eq!(
        code(&eval(&mut worker, b, TOKEN, "globalThis.x=4")),
        "StaleSession"
    );
    let c = worker.attach();
    assert_eq!(
        code(&eval(&mut worker, c, TOKEN, "globalThis.x=5")),
        "Unauthorized"
    );
    assert_eq!(
        worker.evaluate("typeof globalThis.x"),
        Ok("\"undefined\"".into())
    );
}

#[test]
fn a_session_and_grant_of_one_tab_cannot_evaluate_in_another() {
    let mut a = Worker::start_with(1, 1, protocol::VERSION);
    let mut b = Worker::start_with(2, 2, protocol::VERSION);
    let session_a = a.attach();
    grant(&mut a, session_a, TOKEN, true).unwrap();
    b.evaluate("globalThis.victim='untouched'").unwrap();
    // Tab A's session id and token, addressed to tab B's pipe, both with A's target and B's.
    let sent_to_b = |b: &mut Worker, target: TargetId| {
        let context = b.context();
        b.send(Command::Inspector {
            context,
            request: InspectorPayload::new(&InspectorRequest {
                version: protocol::inspector::INSPECTOR_VERSION,
                session: session_a,
                target,
                command: InspectorCommand::Evaluate,
                args: InspectorArgs::Evaluate {
                    expression: "globalThis.victim='owned'".into(),
                    grant: TOKEN,
                },
            }),
        });
        b.reply(context)
    };
    assert_eq!(sent_to_b(&mut b, a.target()), Err("UnknownTarget".into()));
    let own = b.target();
    assert_eq!(sent_to_b(&mut b, own), Err("Unauthorized".into()));
    assert_eq!(b.evaluate("victim"), Ok("\"untouched\"".into()));
    // B's own session needs its own grant.
    let session_b = b.attach();
    assert_eq!(
        code(&eval(&mut b, session_b, TOKEN, "globalThis.victim='owned'")),
        "Unauthorized"
    );
    assert_eq!(b.evaluate("victim"), Ok("\"untouched\"".into()));
}

#[test]
fn a_replacement_process_never_honours_the_old_sessions_authority() {
    let mut old = Worker::start_with(5, 1, protocol::VERSION);
    let session = old.attach();
    grant(&mut old, session, TOKEN, true).unwrap();
    assert_eq!(text_of(eval(&mut old, session, TOKEN, "1")), "1");
    let mut replacement = Worker::start_with(5, 2, protocol::VERSION);
    replacement.evaluate("globalThis.v='replacement'").unwrap();
    // The old target addressed at the new process of the same tab.
    let context = replacement.context();
    replacement.send(Command::Inspector {
        context,
        request: InspectorPayload::new(&InspectorRequest {
            version: protocol::inspector::INSPECTOR_VERSION,
            session,
            target: old.target(),
            command: InspectorCommand::Evaluate,
            args: InspectorArgs::Evaluate {
                expression: "globalThis.v='hijacked'".into(),
                grant: TOKEN,
            },
        }),
    });
    assert_eq!(replacement.reply(context), Err("UnknownTarget".into()));
    assert_eq!(
        code(&eval(
            &mut replacement,
            session,
            TOKEN,
            "globalThis.v='hijacked'"
        )),
        "Unauthorized"
    );
    assert_eq!(replacement.evaluate("v"), Ok("\"replacement\"".into()));
    // The browser-side registry dies with the old process.
    let mut host = HostSessions::new(old.target());
    let (host_session, _) = host.open(&old.issuer).unwrap();
    let principal = protocol::authority::Principal {
        origin: protocol::authority::Origin::Opaque,
        top_level_site: None,
    };
    let scope = protocol::inspector::policy::authorise(Some(&principal), true).unwrap();
    host.install_grant(
        host_session,
        protocol::DocumentContext {
            document: protocol::DocumentId(0),
            revision: protocol::DocumentRevision(0),
        },
        TOKEN,
        principal.clone(),
        scope,
    )
    .unwrap();
    host.close();
    assert!(
        host.grant_for(host_session, old.document(), &principal)
            .is_none()
    );
}

#[test]
fn object_handles_and_promise_awaiting_are_deliberately_not_offered() {
    let mut worker = Worker::start();
    let session = worker.attach();
    for command in [
        "GetObjectProperties",
        "ReleaseObject",
        "AwaitPromise",
        "CallFunctionOn",
    ] {
        let context = worker.context();
        worker.send(Command::Inspector {
            context,
            request: InspectorPayload(serde_json::json!({
                "version": 2, "session": session, "target": worker.target(), "command": command, "args": "None"
            })),
        });
        assert_eq!(
            worker.reply(context),
            Err("failed:InvalidRequest".into()),
            "{command}"
        );
    }
    assert!(worker.alive());
}

#[test]
fn the_script_watchdog_ends_a_runaway_evaluation_and_the_failure_is_reported_as_confirmed() {
    let mut worker = Worker::start();
    let session = worker.attach();
    grant(&mut worker, session, TOKEN, true).unwrap();
    let result = eval(&mut worker, session, TOKEN, "while(true){}");
    let (code, message) = result.unwrap_err();
    assert_eq!(code, "Unavailable");
    assert!(
        message.contains("terminated") && message.contains("deadline"),
        "{message}"
    );
    // The engine itself reported the stop and exited: nothing is claimed beyond that.
    worker.pump(Duration::from_millis(300));
    assert!(
        worker
            .notes
            .borrow()
            .iter()
            .any(|n| matches!(n, protocol::ContentEvent::ScriptStopped(_)))
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while worker.alive() {
        assert!(
            Instant::now() < deadline,
            "the worker should exit after a fatal script stop"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[test]
fn a_stalled_engine_answers_after_resume_and_never_panics() {
    let mut worker = Worker::start();
    let session = worker.attach();
    grant(&mut worker, session, TOKEN, true).unwrap();
    let pid = worker.child.id().to_string();
    assert!(
        std::process::Command::new("kill")
            .args(["-STOP", &pid])
            .status()
            .unwrap()
            .success()
    );
    // Queue an evaluation and a status request while the process is stopped.
    let evaluation = worker.context();
    worker.request_with(
        evaluation,
        session,
        InspectorCommand::Evaluate,
        InspectorArgs::Evaluate {
            expression: "21*2".into(),
            grant: TOKEN,
        },
    );
    let status = worker.context();
    worker.request_in(status, session, InspectorCommand::GetStatusSnapshot);
    std::thread::sleep(Duration::from_millis(1200));
    assert!(
        std::process::Command::new("kill")
            .args(["-CONT", &pid])
            .status()
            .unwrap()
            .success()
    );
    // The evaluate deadline (6 s) outlives a short stall; the 2 s status deadline may not, and
    // either outcome is a proper reply, never a hang.
    match worker.reply(evaluation) {
        Ok(InspectorResponse::Evaluated(outcome)) => {
            assert!(matches!(*outcome, EvalOutcome::Value { .. }))
        }
        other => panic!("{other:?}"),
    }
    let status_result = worker.reply(status);
    assert!(
        status_result.is_ok() || status_result == Err("failed:Timeout".into()),
        "{status_result:?}"
    );
    assert_eq!(
        text_of(eval(&mut worker, session, TOKEN, "'recovered'")),
        "recovered"
    );
}

#[test]
fn with_devtools_closed_the_engine_collects_but_emits_nothing_and_old_workers_refuse_console() {
    let mut worker = Worker::start();
    worker
        .evaluate("for(let i=0;i<500;i++)console.log('quiet',i);true")
        .unwrap();
    worker.pump(Duration::from_millis(300));
    assert!(
        worker.take_seen().is_empty(),
        "no inspector traffic without a session"
    );
    let mut old = Worker::start_with(7, 7, 22);
    let session = old.attach();
    let result = old.call_detail(
        session,
        InspectorCommand::EnableConsole,
        InspectorArgs::None,
    );
    assert_eq!(
        code(&result),
        "failed:Unsupported",
        "protocol 22 has no console: {result:?}"
    );
    let result = grant(&mut old, session, TOKEN, true);
    assert_eq!(code(&result), "failed:Unsupported");
}
