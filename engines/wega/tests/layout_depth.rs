//! Nesting-depth regression tests for the "depth-122 hang" (roadmap B1, D3-C.0).
//!
//! Root cause: style and layout recurse once per nesting level, and a native stack overflow in
//! this process does not abort, it spins forever. Default test threads have a 2 MiB stack and
//! overflow at 122 nested blocks; the engine's `wega-dom` thread has 8 MiB and overflows between
//! 520 and 600. The fix is `wega_dom::layout::MAX_LAYOUT_DEPTH`: `flush_layout` refuses deeper
//! documents. Every scenario runs the real engine pipeline in a *child process* (this test
//! binary re-executed with `WEGA_LAYOUT_PROBE`, on an explicitly sized stack) that the parent
//! kills at an external deadline, so a hang can never block the test runner.
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use wega_dom::layout::MAX_LAYOUT_DEPTH;
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

fn view() -> View {
    Engine::new(
        Limits::default(),
        NetworkPolicy {
            offline: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .create_view(Url::parse("https://probe.test/").unwrap())
    .unwrap()
}
fn nested(depth: usize) -> String {
    format!(
        "<!doctype html><body>{}x{}",
        "<div>".repeat(depth),
        "</div>".repeat(depth)
    )
}

/// Child entry point: `WEGA_LAYOUT_PROBE=<depth>:<mode>`. Prints `stage <name> <ms>` lines.
#[test]
fn child_probe() {
    let Ok(spec) = std::env::var("WEGA_LAYOUT_PROBE") else {
        return;
    };
    if std::env::var("WEGA_PROBE_UNGUARDED").is_ok() {
        wega_adapter_blitz::set_layout_depth_limit_for_probe(Some(usize::MAX - 1));
    }
    let stack = std::env::var("WEGA_PROBE_STACK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(wega_engine::ENGINE_THREAD_STACK);
    // The engine's own thread configuration, not the test harness thread.
    std::thread::Builder::new()
        .name("wega-dom".into())
        .stack_size(stack)
        .spawn(move || probe(&spec))
        .unwrap()
        .join()
        .unwrap();
}
fn probe(spec: &str) {
    let (depth, mode) = spec.split_once(':').unwrap();
    let depth: usize = depth.parse().unwrap();
    let t = Instant::now();
    let stage = |name: &str| {
        println!("stage {name} {}", t.elapsed().as_millis());
        use std::io::Write;
        std::io::stdout().flush().unwrap();
    };
    let mut v = view();
    stage("view");
    v.set_html(&nested(depth)).unwrap();
    stage("set_html");
    v.poll();
    stage("poll");
    match mode {
        "script_flush" => {
            let height = v
                .evaluate_json("document.body.getBoundingClientRect().height")
                .unwrap();
            println!("height {height}");
        }
        "paint_then_flush" => {
            v.paint(400, 300, 1.0, 1.0, false).unwrap();
            stage("paint");
            let height = v
                .evaluate_json("document.body.getBoundingClientRect().height")
                .unwrap();
            println!("height {height}");
        }
        "script_walk" => {
            for js in [
                "document.documentElement.outerHTML.length",
                "document.body.textContent.length",
                "document.body.cloneNode(true).querySelectorAll('div').length",
                "document.querySelectorAll('div div div div').length",
                "document.body.innerHTML.length",
            ] {
                v.evaluate_json(js).unwrap();
                stage(js);
            }
        }
        "paint" => {
            v.paint(400, 300, 1.0, 1.0, false).unwrap();
        }
        // Every other way a page or the browser can reach layout, style or geometry.
        "misc" => {
            for js in [
                "getComputedStyle(document.body).width.length",
                "document.body.scrollIntoView();1",
                "document.body.offsetWidth+document.body.scrollHeight+document.body.clientWidth",
                "window.innerWidth+window.scrollY",
                "document.body.focus();document.activeElement===document.body",
                "document.body.getClientRects().length",
                "(()=>{window.scrollTo(0,10);return window.scrollY})()",
            ] {
                // A missing API is a TypeError, not a layout problem; only hangs and crashes matter.
                let _ = v.evaluate_json(js);
                stage(js);
            }
            v.paint(800, 600, 2.0, 1.5, true).unwrap();
            stage("repaint-dark-hidpi");
            v.paint(200, 100, 1.0, 1.0, false).unwrap();
            stage("repaint-resized");
            let hit = v.pointer(wega_engine::Pointer {
                kind: wega_engine::PointerKind::Move,
                x: 10.0,
                y: 10.0,
                button: 0,
                buttons: 0,
                modifiers: 0,
                delta_x: 0.0,
                delta_y: 0.0,
            });
            let _ = hit;
            stage("pointer");
        }
        // After a refused layout the same view must show an ordinary page correctly.
        "recover" => {
            v.paint(400, 300, 1.0, 1.0, false).unwrap();
            stage("deep-painted");
            v.set_html(
                "<!doctype html><body style='margin:0'><div id=a style='height:77px'>ok</div>",
            )
            .unwrap();
            v.poll();
            v.paint(400, 300, 1.0, 1.0, false).unwrap();
            let h = v
                .evaluate_json("document.getElementById('a').getBoundingClientRect().height")
                .unwrap();
            println!("height {h}");
        }
        _ => panic!("mode"),
    }
    stage("done");
}

struct Outcome {
    finished: bool,
    elapsed: Duration,
    stages: String,
    height: Option<f64>,
}
/// The stack of the engine's `wega-dom` thread (`engines/wega/src/main.rs`).
const ENGINE_STACK: usize = 8 * 1024 * 1024;
/// The default stack of a Rust test thread, where the historical hang was first seen.
const TEST_THREAD_STACK: usize = 2 * 1024 * 1024;
fn run_child(depth: usize, mode: &str, limit: Duration) -> Outcome {
    run_child_on(depth, mode, limit, ENGINE_STACK)
}
fn run_child_on(depth: usize, mode: &str, limit: Duration, stack: usize) -> Outcome {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["child_probe", "--exact", "--nocapture", "--test-threads=1"])
        .env("WEGA_LAYOUT_PROBE", format!("{depth}:{mode}"))
        .env("WEGA_PROBE_STACK", stack.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut out = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let started = Instant::now();
    let mut finished = false;
    while started.elapsed() < limit {
        if let Some(status) = child.try_wait().unwrap() {
            finished = status.success();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let elapsed = started.elapsed();
    if child.try_wait().unwrap().is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let text = reader.join().unwrap();
    // libtest prefixes the first line of `--nocapture` output with `test child_probe ... `.
    let lines = || {
        text.lines()
            .map(|l| l.rsplit("... ").next().unwrap_or(l).trim())
    };
    let stages = lines()
        .filter(|l| l.starts_with("stage"))
        .collect::<Vec<_>>()
        .join("; ");
    let height = lines()
        .find_map(|l| l.strip_prefix("height "))
        .and_then(|h| h.trim_matches('"').parse().ok());
    Outcome {
        finished,
        elapsed,
        stages,
        height,
    }
}

/// Generous for debug builds under load (a typical scenario takes 0.3 to 1 s); a spinning child
/// is killed here, so a regression costs this long instead of hanging the run.
const DEADLINE: Duration = Duration::from_secs(60);

#[test]
fn every_depth_completes_on_the_engine_stack_through_script_flush_and_paint() {
    for mode in ["script_flush", "paint_then_flush", "paint"] {
        for depth in [120, 121, 122, 128, 256, 1000] {
            let o = run_child(depth, mode, DEADLINE);
            assert!(
                o.finished,
                "{mode} at depth {depth} did not complete in {:?}; stages: {}",
                o.elapsed, o.stages
            );
        }
    }
}

#[test]
fn documents_within_the_limit_are_laid_out_and_deeper_ones_are_refused_not_hung() {
    // `<html><body>` plus the nested divs: the limit counts tree depth from the document root.
    let within = MAX_LAYOUT_DEPTH - 3;
    for mode in ["script_flush", "paint_then_flush"] {
        let laid_out = run_child(within, mode, DEADLINE);
        assert!(laid_out.finished, "{mode}: {}", laid_out.stages);
        assert!(
            laid_out.height.is_some_and(|h| h > 0.0),
            "{mode}: depth {within} must lay out, got {:?}",
            laid_out.height
        );
        let refused = run_child(within + 1, mode, DEADLINE);
        assert!(refused.finished, "{mode}: {}", refused.stages);
        assert_eq!(
            refused.height,
            Some(0.0),
            "{mode}: depth {} is past the limit and has no layout",
            within + 1
        );
    }
}

#[test]
fn the_cliff_is_the_native_stack_not_the_depth_so_a_small_stack_is_unsupported() {
    // Characterisation of the root cause, kept so the contract stays honest: on a 2 MiB stack the
    // supported depth is lower than `MAX_LAYOUT_DEPTH`, and the overflow is a spin (or an abort on
    // other platforms), never a completed layout. `MAX_LAYOUT_DEPTH` is therefore only valid on
    // the engine thread's 8 MiB stack. This scenario costs up to its 4 s deadline.
    let o = run_child_on(
        200,
        "script_flush",
        Duration::from_secs(4),
        TEST_THREAD_STACK,
    );
    assert!(
        !o.finished,
        "depth 200 completed on a 2 MiB stack; the cliff moved, re-measure and update MAX_LAYOUT_DEPTH docs"
    );
    // The same page on the engine stack completes.
    assert!(run_child(200, "script_flush", DEADLINE).finished);
}

/// Extra scenarios on the engine stack: every other route into layout or geometry, and recovery.
#[test]
fn other_routes_into_layout_cannot_bypass_the_guard_and_the_view_recovers() {
    for depth in [122, 257, 520, 600, 1000] {
        let o = run_child(depth, "misc", DEADLINE);
        assert!(o.finished, "misc at depth {depth}: {}", o.stages);
        let o = run_child(depth, "recover", DEADLINE);
        assert!(o.finished, "recover at depth {depth}: {}", o.stages);
        assert_eq!(o.height, Some(77.0), "an ordinary page after depth {depth}");
    }
}

#[test]
fn the_remaining_depths_of_the_validation_list_complete() {
    for depth in [200, 520, 600] {
        for mode in ["script_flush", "paint"] {
            let o = run_child(depth, mode, DEADLINE);
            assert!(o.finished, "{mode} at depth {depth}: {}", o.stages);
        }
    }
}

#[test]
#[ignore = "measurement: finds the UNGUARDED cliff of this build (set WEGA_PROBE_DEPTHS)"]
fn print_unguarded_cliff() {
    // SAFETY-free: the child lifts the limit through a doc(hidden) probe hook.
    let depths: Vec<usize> = std::env::var("WEGA_PROBE_DEPTHS")
        .map(|v| v.split(',').map(|d| d.parse().unwrap()).collect())
        .unwrap_or_else(|_| vec![256, 400, 600, 800, 1200, 1600, 2400, 3200]);
    for depth in depths {
        for mode in ["script_flush", "paint"] {
            let o = run_child_unguarded(depth, mode, Duration::from_secs(12));
            println!(
                "UNGUARDED {mode:13} depth {depth:5} finished={}",
                o.finished
            );
        }
    }
}
fn run_child_unguarded(depth: usize, mode: &str, limit: Duration) -> Outcome {
    // The child reads the variable; the parent only needs it set for the spawn.
    // (Environment mutation is process-wide but this test is ignored and single-purpose.)
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["child_probe", "--exact", "--nocapture", "--test-threads=1"])
        .env("WEGA_LAYOUT_PROBE", format!("{depth}:{mode}"))
        .env("WEGA_PROBE_UNGUARDED", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let started = Instant::now();
    let mut finished = false;
    while started.elapsed() < limit {
        if let Some(status) = child.try_wait().unwrap() {
            finished = status.success();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if child.try_wait().unwrap().is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Outcome {
        finished,
        elapsed: started.elapsed(),
        stages: String::new(),
        height: None,
    }
}

#[test]
#[ignore = "exploration: prints a depth/mode table"]
fn print_matrix() {
    let limit = Duration::from_secs(
        std::env::var("WEGA_PROBE_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20),
    );
    let depths: Vec<usize> = std::env::var("WEGA_PROBE_DEPTHS")
        .map(|v| v.split(',').map(|d| d.parse().unwrap()).collect())
        .unwrap_or_else(|_| vec![100, 119, 120, 121, 122, 123, 128, 256]);
    let modes: Vec<String> = std::env::var("WEGA_PROBE_MODES")
        .map(|v| v.split(',').map(str::to_string).collect())
        .unwrap_or_else(|_| {
            vec![
                "script_flush".into(),
                "paint_then_flush".into(),
                "paint".into(),
            ]
        });
    for mode in modes.iter().map(String::as_str) {
        for &depth in &depths {
            let o = run_child(depth, mode, limit);
            println!(
                "{mode:18} depth {depth:5} finished={} {:6} ms | {}",
                o.finished,
                o.elapsed.as_millis(),
                o.stages
            );
        }
    }
}
