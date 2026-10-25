//! Offline acceptance list shared by the QuickJS and V8 hosts.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};
fn fixture(name: &str) -> View {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.html"));
    Engine::new(
        Limits {
            script_time: Duration::from_secs(2),
            js_heap_bytes: 64 * 1024 * 1024,
            ..Limits::default()
        },
        NetworkPolicy::default(),
        Arc::new(|| {}),
    )
    .unwrap()
    .create_view(Url::from_file_path(file).unwrap())
    .unwrap()
}
fn counter(name: &str) {
    let mut view = fixture(name);
    let deadline = Instant::now() + Duration::from_secs(10);
    while view.text("#action").as_deref() != Some("0") && Instant::now() < deadline {
        view.poll();
        if view.needs_paint() {
            view.paint(300, 200, 1.0, 1.0, false).unwrap();
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        view.text("#action").as_deref(),
        Some("0"),
        "{name}: {:?}",
        view.notices
    );
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    view.click(30.0, 30.0);
    while view.text("#action").as_deref() != Some("1") && Instant::now() < deadline {
        view.poll();
        if view.needs_paint() {
            view.paint(300, 200, 1.0, 1.0, false).unwrap();
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        view.text("#action").as_deref(),
        Some("1"),
        "{name}: {:?}",
        view.notices
    );
}
#[test]
fn jquery() {
    counter("jquery");
}
#[test]
fn react() {
    counter("react");
}
#[test]
fn vue() {
    counter("vue");
}
#[test]
fn preact() {
    counter("preact");
}
#[test]
fn inline_svg() {
    let mut view = fixture("svg");
    let deadline = Instant::now() + Duration::from_secs(5);
    while view.loading && Instant::now() < deadline {
        view.poll();
        if view.needs_paint() {
            view.paint(300, 200, 1.0, 1.0, false).unwrap();
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let frame = view.paint(120, 120, 1.0, 1.0, false).unwrap();
    assert!(frame.rgba[(30 * 120 + 30) * 4 + 1] > 240);
    view.evaluate(
        "document.getElementById('shape').dispatchEvent(new Event('click',{bubbles:true}))",
    )
    .unwrap();
    view.poll();
    let frame = view.paint(120, 120, 1.0, 1.0, false).unwrap();
    assert!(frame.rgba[(30 * 120 + 30) * 4] > 240);
}
#[test]
fn angular() {
    counter("angular");
}
