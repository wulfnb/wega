//! Inspector performance measurements against a real worker process (ignored by default):
//!
//!     cargo test --release -p wega-engine --test inspector_bench -- --ignored --nocapture
//!
//! Prints `BENCH name: value unit` lines (the format `scripts/engine-bench.py` parses) plus
//! `NOTE` lines for context. Medians and p95 come from the samples of one run; repeat runs to
//! see the spread. Nothing here asserts a performance verdict: numbers without a comparable
//! baseline are reported as measurements only.
mod support;
use protocol::{
    Command, Event,
    inspector::{
        EVENT_RING, InspectorCommand, InspectorEvent, InspectorResponse,
        session::{HostSessions, SessionIssuer},
    },
};
use std::time::{Duration, Instant};
use support::inspector_worker::Worker;
use wega_protocol as protocol;

fn percentile(samples: &mut [f64], q: f64) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[((samples.len() - 1) as f64 * q).round() as usize]
}
fn report(name: &str, samples: &mut [f64], unit: &str) {
    println!(
        "BENCH {name} (median): {:.3} {unit}",
        percentile(samples, 0.5)
    );
    println!(
        "BENCH {name} (p95): {:.3} {unit}",
        percentile(samples, 0.95)
    );
    println!("BENCH {name} (max): {:.3} {unit}", percentile(samples, 1.0));
}
fn cpu_seconds(pid: u32) -> f64 {
    // `ps -o time=` prints [[dd-]hh:]mm:ss.cc of cumulative CPU time.
    let out = std::process::Command::new("ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    text.split(':').fold(0.0, |total, part| {
        total * 60.0 + part.parse::<f64>().unwrap_or(0.0)
    })
}
fn wait_idle(worker: &mut Worker) {
    worker.until(|e| matches!(e, Event::State { loading: false, .. }));
}

#[test]
#[ignore = "BENCH: inspector round trips, navigation overhead, idle CPU, queue bounds"]
fn inspector_paths() {
    let mut worker = Worker::start();
    wait_idle(&mut worker);
    println!(
        "NOTE protocol {} inspector {}",
        protocol::VERSION,
        protocol::inspector::INSPECTOR_VERSION
    );

    // Attach / detach latency (host send → engine reply), distinct session each time.
    let (mut attach, mut detach) = (Vec::new(), Vec::new());
    for _ in 0..300 {
        let session = worker.issuer.next().unwrap();
        let start = Instant::now();
        worker.call(session, InspectorCommand::Attach).unwrap();
        attach.push(start.elapsed().as_secs_f64() * 1e3);
        let start = Instant::now();
        worker.call(session, InspectorCommand::Detach).unwrap();
        detach.push(start.elapsed().as_secs_f64() * 1e3);
    }
    report("inspector attach round trip", &mut attach, "ms");
    report("inspector detach round trip", &mut detach, "ms");

    // Status snapshot round trip, and its wire size.
    let session = worker.attach();
    let mut status = Vec::new();
    for _ in 0..500 {
        let start = Instant::now();
        worker.snapshot(session);
        status.push(start.elapsed().as_secs_f64() * 1e3);
    }
    report("inspector status snapshot round trip", &mut status, "ms");
    let snapshot = worker.snapshot(session);
    println!(
        "BENCH inspector status snapshot wire size: {} bytes",
        serde_json::to_vec(&snapshot).unwrap().len()
    );

    // Navigation overhead: commit-to-settled time without and with a subscribed session.
    let seen_events = std::cell::Cell::new(0_usize);
    let navigate = |worker: &mut Worker, serial: u64| {
        let start = Instant::now();
        worker.send(Command::Load {
            url: "about:blank".into(),
            serial,
            bypass_cache: false,
        });
        worker.until(|e| {
            if matches!(e, Event::InspectorEvent { .. }) {
                seen_events.set(seen_events.get() + 1);
            }
            matches!(e, Event::State { serial: s, loading: false, .. } if *s == serial)
        });
        start.elapsed().as_secs_f64() * 1e3
    };
    let (mut closed, mut subscribed) = (Vec::new(), Vec::new());
    let mut serial = 0;
    worker.send(Command::InspectorDetach { session });
    for _ in 0..40 {
        serial += 1;
        closed.push(navigate(&mut worker, serial));
    }
    let session = worker.attach();
    assert_eq!(
        worker.call(session, InspectorCommand::SubscribeStatus),
        Ok(InspectorResponse::Subscribed)
    );
    for _ in 0..40 {
        serial += 1;
        subscribed.push(navigate(&mut worker, serial));
    }
    let events = seen_events.get();
    report("navigation, DevTools closed", &mut closed, "ms");
    report(
        "navigation, DevTools attached+subscribed",
        &mut subscribed,
        "ms",
    );
    println!("NOTE inspector events delivered during 40 subscribed navigations: {events}");

    // Idle CPU of the content process over 10 s: closed, attached, subscribed, polling at 1 Hz.
    let pid = worker.child.id();
    let window = |worker: &mut Worker, poll: bool| {
        let before = cpu_seconds(pid);
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if poll {
                worker.snapshot(session);
            }
            std::thread::sleep(Duration::from_millis(if poll { 1000 } else { 100 }));
        }
        (cpu_seconds(pid) - before) * 100.0 / 10.0
    };
    worker.send(Command::InspectorDetach { session });
    let closed_cpu = window(&mut worker, false);
    let session = worker.attach();
    worker
        .call(session, InspectorCommand::SubscribeStatus)
        .unwrap();
    let attached_cpu = window(&mut worker, false);
    let polling_cpu = {
        let before = cpu_seconds(pid);
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            worker.snapshot(session);
            std::thread::sleep(Duration::from_millis(1000));
        }
        (cpu_seconds(pid) - before) * 100.0 / 10.0
    };
    println!("BENCH idle engine CPU, DevTools closed: {closed_cpu:.2} %");
    println!("BENCH idle engine CPU, attached+subscribed: {attached_cpu:.2} %");
    println!("BENCH idle engine CPU, attached+1 Hz snapshots: {polling_cpu:.2} %");
    println!("NOTE `ps` CPU time has 10 ms resolution; differences below ~0.1 % are within it");
}

#[test]
#[ignore = "BENCH: browser-side event queue memory bound"]
fn event_queue_bounds() {
    let issuer = SessionIssuer::new();
    let target = protocol::inspector::TargetId {
        tab: protocol::identity::TabId(1),
        frame: protocol::identity::FrameId(1),
        process: protocol::identity::ProcessSessionId([1; 16]),
    };
    let mut host = HostSessions::new(target);
    let (session, _) = host.open(&issuer).unwrap();
    host.complete(
        session,
        InspectorCommand::SubscribeStatus,
        Ok(InspectorResponse::Subscribed),
    );
    let before = wega_platform::resident_bytes();
    let mut peak = 0;
    let start = Instant::now();
    for seq in 1..=5_000_000_u64 {
        host.event(
            session,
            seq,
            InspectorEvent::LoadingChanged {
                loading: seq % 2 == 0,
            },
        );
        peak = peak.max(host.queued().0);
    }
    let elapsed = start.elapsed();
    let after = wega_platform::resident_bytes();
    let (events, _) = (host.take_events(session), ());
    println!("BENCH event ring peak length after 5,000,000 events: {peak} events");
    println!("BENCH event ring capacity: {EVENT_RING} events");
    println!(
        "BENCH event ring static bound: {} bytes",
        EVENT_RING * std::mem::size_of::<InspectorEvent>()
    );
    println!(
        "BENCH event ring resident growth over 5,000,000 events: {} KiB",
        after.unwrap_or(0).saturating_sub(before.unwrap_or(0)) / 1024
    );
    println!(
        "BENCH event ring ingest rate: {:.1} ns/event",
        elapsed.as_nanos() as f64 / 5_000_000.0
    );
    println!(
        "NOTE drained {} entries incl. the overflow marker",
        events.len()
    );
    assert!(peak <= EVENT_RING);
}

fn rss_kib(pid: u32) -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

#[test]
#[ignore = "BENCH: console and evaluation paths over a real worker pipe (debug engine)"]
fn console_ipc_paths() {
    use protocol::inspector::{GrantToken, InspectorArgs};
    let mut worker = Worker::start();
    wait_idle(&mut worker);
    let session = worker.attach();
    let token = GrantToken([0x33; 16]);
    worker
        .call_with(
            session,
            InspectorCommand::GrantEvaluation,
            InspectorArgs::Grant {
                token,
                opaque_override: true,
                origin: None,
            },
        )
        .unwrap();
    // Evaluation round trip.
    let mut eval = Vec::new();
    for _ in 0..300 {
        let start = Instant::now();
        worker
            .call_with(
                session,
                InspectorCommand::Evaluate,
                InspectorArgs::Evaluate {
                    expression: "1+1".into(),
                    grant: token,
                },
            )
            .unwrap();
        eval.push(start.elapsed().as_secs_f64() * 1e3);
    }
    report("inspector evaluate(1+1) round trip", &mut eval, "ms");
    // History retrieval over a full ring.
    worker
        .evaluate("for(let i=0;i<1000;i++)console.log('h',i,{i});true")
        .unwrap();
    let mut history = Vec::new();
    for _ in 0..200 {
        let start = Instant::now();
        worker
            .call_with(
                session,
                InspectorCommand::GetConsoleHistory,
                InspectorArgs::History {
                    after: 0,
                    limit: 100,
                },
            )
            .unwrap();
        history.push(start.elapsed().as_secs_f64() * 1e3);
    }
    report(
        "inspector console history page (100 entries)",
        &mut history,
        "ms",
    );
    // Flood: task time with the console disabled vs enabled, then delivery.
    let pid = worker.child.id();
    let flood = "for(let i=0;i<20000;i++)console.log('flood',i,{i});true";
    let rss_start = rss_kib(pid);
    let start = Instant::now();
    worker.evaluate(flood).unwrap();
    let disabled = start.elapsed().as_secs_f64() * 1e3;
    worker
        .call_with(session, InspectorCommand::ClearConsole, InspectorArgs::None)
        .unwrap();
    println!("BENCH flood task time, console not enabled (20,000 calls): {disabled:.0} ms");
    assert!(matches!(
        worker.call_with(
            session,
            InspectorCommand::EnableConsole,
            InspectorArgs::None
        ),
        Ok(InspectorResponse::ConsoleEnabled { .. })
    ));
    let _ = worker.take_seen();
    let start = Instant::now();
    worker.evaluate(flood).unwrap();
    let enabled = start.elapsed().as_secs_f64() * 1e3;
    println!("BENCH flood task time, console enabled (20,000 calls): {enabled:.0} ms");
    // Delivery: how long until the engine has delivered what survived, and how much arrived.
    let settle = Instant::now();
    worker.pump(Duration::from_millis(250));
    let seen = worker.take_seen();
    let delivered = seen
        .iter()
        .filter(|(_, _, e)| matches!(e, InspectorEvent::ConsoleMessageAdded(_)))
        .count();
    let dropped: u64 = seen
        .iter()
        .filter_map(|(_, _, e)| match e {
            InspectorEvent::ConsoleMessagesDropped { count } => Some(*count),
            _ => None,
        })
        .sum();
    println!(
        "BENCH flood delivered events after the task: {delivered} messages, {dropped} reported dropped, {} ms to quiesce",
        settle.elapsed().as_millis().saturating_sub(250)
    );
    println!(
        "BENCH engine resident growth over two 20,000-call floods: {} KiB",
        rss_kib(pid).saturating_sub(rss_start)
    );
    // A snapshot request while a backlog drains (fairness).
    worker.evaluate(flood).unwrap();
    let start = Instant::now();
    worker.snapshot(session);
    println!(
        "BENCH status snapshot latency while a console backlog drains: {:.1} ms",
        start.elapsed().as_secs_f64() * 1e3
    );
    worker.pump(Duration::from_millis(250));
    // Navigation with and without a subscribed, console-enabled session.
    let navigate = |worker: &mut Worker, serial: u64| {
        let start = Instant::now();
        worker.send(Command::Load {
            url: "about:blank".into(),
            serial,
            bypass_cache: false,
        });
        worker
            .until(|e| matches!(e, Event::State { serial: s, loading: false, .. } if *s == serial));
        start.elapsed().as_secs_f64() * 1e3
    };
    let mut serial = 0;
    let mut with_console = Vec::new();
    for _ in 0..30 {
        serial += 1;
        with_console.push(navigate(&mut worker, serial));
    }
    report("navigation, console enabled", &mut with_console, "ms");
    // Idle CPU, 10 s: console enabled with an empty ring.
    let before = cpu_seconds(pid);
    std::thread::sleep(Duration::from_secs(10));
    println!(
        "BENCH idle engine CPU, console enabled: {:.2} %",
        (cpu_seconds(pid) - before) * 10.0
    );
    worker
        .call_with(
            session,
            InspectorCommand::DisableConsole,
            InspectorArgs::None,
        )
        .unwrap();
    println!("NOTE debug engine; `ps` CPU resolution is 10 ms");
}
