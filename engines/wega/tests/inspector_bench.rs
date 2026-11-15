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
