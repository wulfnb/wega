//! Iframe cost measurements. Run in release for meaningful numbers:
//! `cargo test --release -p wega-engine --test iframe_bench -- --ignored --nocapture --test-threads=1`
//! Prints one `BENCH {json}` line per measurement. Nothing here asserts timing (machines
//! differ); the assertions only check that the measured behaviour happened.
mod support;
use std::{
    thread,
    time::{Duration, Instant},
};
use support::{Reply, Server, engine, settle};
use wega_engine::{Pointer, PointerKind, Url, View};

fn pump(view: &mut View, mut done: impl FnMut(&mut View) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        view.poll();
        if done(view) {
            return true;
        }
        thread::sleep(Duration::from_micros(500));
    }
    false
}
fn rss_kb() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}
fn emit(name: &str, value: serde_json::Value) {
    println!(
        "BENCH {}",
        serde_json::json!({"name": name, "value": value})
    );
}
fn all_loaded(v: &mut View, n: usize) -> bool {
    v.child_frames().len() == n
        && v.frame_tree()
            .iter()
            .all(|f| f.url.contains("/leaf") || f.url.contains("/nest"))
        && v.child_frames()
            .iter()
            .all(|(k, _)| v.child_by_key(*k).is_some_and(|c| !c.loading))
}
fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}
fn site() -> Server {
    Server::new(|seen| match seen.path.as_str() {
        "/none" => Reply::html("<body style='margin:0'><p>no frames</p></body>"),
        "/one" => Reply::html(
            "<body style='margin:0'><iframe src=/leaf style='width:300px;height:200px;border:0'></iframe></body>",
        ),
        "/eight" => Reply::html(&format!(
            "<body style='margin:0'>{}</body>",
            "<iframe src=/leaf style='width:150px;height:100px;border:0'></iframe>".repeat(8)
        )),
        "/nest" => Reply::html(
            "<body style='margin:0'><iframe src=/nest2 style='width:300px;height:200px;border:0'></iframe></body>",
        ),
        "/nest2" => Reply::html(
            "<body style='margin:0'><iframe src=/nest3 style='width:280px;height:180px;border:0'></iframe></body>",
        ),
        "/nest3" => Reply::html(
            "<body style='margin:0'><iframe src=/leaf style='width:260px;height:160px;border:0'></iframe></body>",
        ),
        "/leaf" => Reply::html(
            "<body style='margin:0;background:#9cf'><p>leaf</p><script>addEventListener('message',e=>parent.postMessage(e.data,'*'));addEventListener('click',()=>{globalThis.c=(globalThis.c||0)+1})</script></body>",
        ),
        _ => Reply::html("").status("404 Not Found"),
    })
}
fn load(engine: &wega_engine::Engine, server: &Server, path: &str, frames: usize) -> (View, f64) {
    let started = Instant::now();
    let mut view = engine
        .create_view(Url::parse(&server.url("localhost", path)).unwrap())
        .unwrap();
    settle(&mut view);
    assert!(pump(&mut view, |v| v.live_frames() >= frames
        && v.frame_tree()
            .iter()
            .all(|f| f.url.contains("/leaf") || f.url.contains("/nest"))
        && v.child_frames().iter().all(|(k, _)| v
            .child_by_key(*k)
            .is_some_and(|c| !c.loading))));
    (view, started.elapsed().as_secs_f64() * 1000.0)
}

#[test]
#[ignore]
fn iframe_costs() {
    let server = site();
    let engine = engine();
    let _ = all_loaded;
    // Load times (median of 5), page with and without frames.
    for (label, path, frames) in [
        ("none", "/none", 0usize),
        ("one_frame", "/one", 1),
        ("eight_siblings", "/eight", 8),
        ("nested_depth_4", "/nest", 4),
    ] {
        let mut samples = Vec::new();
        for _ in 0..5 {
            let (view, ms) = load(&engine, &server, path, frames);
            samples.push(ms);
            drop(view);
        }
        emit(
            &format!("load_ms.{label}"),
            serde_json::json!(median(samples)),
        );
    }
    // Paint cost (median of 20) for the same documents, 1280x800.
    for (label, path, frames) in [
        ("none", "/none", 0usize),
        ("one_frame", "/one", 1),
        ("eight_siblings", "/eight", 8),
        ("nested_depth_4", "/nest", 4),
    ] {
        let (mut view, _) = load(&engine, &server, path, frames);
        view.paint(1280, 800, 1.0, 1.0, false).unwrap();
        let mut samples = Vec::new();
        for _ in 0..20 {
            view.request_redraw();
            let t = Instant::now();
            view.paint(1280, 800, 1.0, 1.0, false).unwrap();
            samples.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        emit(
            &format!("paint_ms.{label}"),
            serde_json::json!(median(samples)),
        );
    }
    // Frame creation + destruction (script-created srcdoc frames), 16 frames.
    let (mut view, _) = load(&engine, &server, "/none", 0);
    let t = Instant::now();
    view.evaluate_json("for (let i = 0; i < 16; i++) { const f = document.createElement('iframe'); f.srcdoc = '<p>x</p>'; document.body.appendChild(f); } 0").unwrap();
    assert!(pump(&mut view, |v| v.live_frames() == 16
        && v.child_frames().iter().all(|(k, _)| v
            .child_by_key(*k)
            .is_some_and(|c| !c.loading))));
    emit(
        "create_16_srcdoc_frames_ms",
        serde_json::json!(t.elapsed().as_secs_f64() * 1000.0),
    );
    let t = Instant::now();
    view.evaluate_json("document.querySelectorAll('iframe').forEach(f => f.remove()); 0")
        .unwrap();
    assert!(pump(&mut view, |v| v.live_frames() == 0));
    emit(
        "destroy_16_frames_ms",
        serde_json::json!(t.elapsed().as_secs_f64() * 1000.0),
    );
    // Memory per frame: RSS growth for 16 live frames (includes one V8 isolate each).
    drop(view);
    thread::sleep(Duration::from_millis(200));
    let before = rss_kb();
    let (mut view, _) = load(&engine, &server, "/none", 0);
    let base = rss_kb();
    view.evaluate_json("for (let i = 0; i < 16; i++) { const f = document.createElement('iframe'); f.srcdoc = '<p>x</p>'; document.body.appendChild(f); } 0").unwrap();
    assert!(pump(&mut view, |v| v.live_frames() == 16
        && v.child_frames().iter().all(|(k, _)| v
            .child_by_key(*k)
            .is_some_and(|c| !c.loading))));
    let with = rss_kb();
    emit(
        "rss_kb_per_frame",
        serde_json::json!((with.saturating_sub(base)) as f64 / 16.0),
    );
    emit(
        "rss_kb_baseline_view",
        serde_json::json!(base.saturating_sub(before)),
    );
    // Heap of one child realm (JS heap, not RSS).
    let key = view.child_frames()[0].0;
    emit(
        "child_js_heap_bytes",
        serde_json::json!(view.child_by_key(key).unwrap().heap_bytes()),
    );
    drop(view);
    // postMessage round trip parent -> child -> parent (median of 50) and click routing.
    let (mut view, _) = load(&engine, &server, "/one", 1);
    view.evaluate_json(
        "globalThis.n = 0; addEventListener('message', () => { globalThis.n++; }); 0",
    )
    .unwrap();
    let mut samples = Vec::new();
    for i in 0..50 {
        let t = Instant::now();
        view.evaluate_json(
            "document.querySelector('iframe').contentWindow.postMessage('x', '*'); 0",
        )
        .unwrap();
        assert!(pump(&mut view, |v| v
            .evaluate_json("n")
            .is_ok_and(|r| r == (i + 1).to_string())));
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    emit(
        "postmessage_roundtrip_ms",
        serde_json::json!(median(samples)),
    );
    view.paint(1280, 800, 1.0, 1.0, false).unwrap();
    let mut samples = Vec::new();
    for _ in 0..100 {
        let t = Instant::now();
        view.pointer(Pointer::new(PointerKind::Move, 20.0, 20.0));
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    emit(
        "pointer_move_into_frame_ms",
        serde_json::json!(median(samples)),
    );
    // Navigation latency of one frame (src change to load).
    let mut samples = Vec::new();
    for i in 0..10 {
        let url = server.url("localhost", &format!("/leaf?{i}"));
        let t = Instant::now();
        view.evaluate_json(&format!(
            "document.querySelector('iframe').src = '{url}'; 0"
        ))
        .unwrap();
        assert!(pump(&mut view, |v| v
            .frame_tree()
            .first()
            .is_some_and(|f| f.url.ends_with(&format!("?{i}")))
            && v.child_frames().iter().all(|(k, _)| v
                .child_by_key(*k)
                .is_some_and(|c| !c.loading))));
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    emit("frame_navigation_ms", serde_json::json!(median(samples)));
}
