//! Offline acceptance list for the V8 host.
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
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
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
fn trak_in_jquery_363_browser_bundle_initializes_and_manipulates_dom() {
    let engine = Engine::new(
        Limits {
            script_time: Duration::from_secs(2),
            ..Limits::default()
        },
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap();
    let mut view = engine
        .create_view(Url::parse("about:blank").unwrap())
        .unwrap();
    view.set_html("<body><main id='mount'></main>").unwrap();
    view.poll();
    view.evaluate(include_str!("fixtures/jquery-3.6.3.min.js"))
        .unwrap();
    assert_eq!(view.evaluate_json("(()=>{const node=jQuery('<div><span>trak</span></div>');jQuery(document.body).append(node);return [jQuery.fn.jquery,node.find('span').text(),node[0].ownerDocument===document]})()").unwrap(), "[\"3.6.3\",\"trak\",true]");
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
