//! Execute unmodified upstream WPT programs using the real View and WPT harness.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use wega_engine::{Engine, Limits, NetworkPolicy, Url};
#[test]
fn pinned_css_wpt_subset_has_no_regressions() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/css-wpt/cases.json")).unwrap();
    use sha2::{Digest, Sha256};
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/css-wpt");
    let upstream: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/css-wpt/UPSTREAM.json")).unwrap();
    for (file, expected) in upstream["files"].as_object().unwrap() {
        let bytes = std::fs::read(directory.join(file)).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            expected.as_str().unwrap(),
            "Pinned WPT source changed: {file}"
        );
    }
    for case in cases.as_array().unwrap() {
        let source =
            std::fs::read_to_string(directory.join(case["file"].as_str().unwrap())).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(source.as_bytes())),
            case["sha256"].as_str().unwrap()
        );
        let mut remaining = source.as_str();
        let mut programs = Vec::new();
        let mut html = String::new();
        while let Some(start) = remaining.find("<script") {
            html.push_str(&remaining[..start]);
            let tag_end = start + remaining[start..].find('>').unwrap();
            let end = tag_end + 1 + remaining[tag_end + 1..].find("</script>").unwrap();
            if !remaining[start..tag_end].contains("src=") {
                programs.push(&remaining[tag_end + 1..end]);
            }
            remaining = &remaining[end + 9..];
        }
        html.push_str(remaining);
        assert_eq!(
            html,
            case["html"].as_str().unwrap(),
            "WPT fixture extraction changed"
        );
        assert_eq!(
            programs,
            case["scripts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap())
                .collect::<Vec<_>>(),
            "WPT assertions must stay unmodified"
        );
    }
    let mut total = 0;
    for case in cases.as_array().unwrap() {
        let engine = Engine::new(
            Limits {
                script_time: Duration::from_secs(3),
                js_heap_bytes: 64 * 1024 * 1024,
                ..Limits::default()
            },
            NetworkPolicy {
                offline: true,
                ..Default::default()
            },
            Arc::new(|| {}),
        )
        .unwrap();
        let mut view = engine
            .create_view(Url::parse("about:blank").unwrap())
            .unwrap();
        view.set_html(case["html"].as_str().unwrap()).unwrap();
        view.evaluate(include_str!("fixtures/wpt/testharness.js"))
            .unwrap();
        view.evaluate("setup({explicit_done:true,output:false});globalThis.__wptResults=null;add_completion_callback((tests,status)=>globalThis.__wptResults={status:status.status,message:status.message,tests:tests.map(t=>({name:t.name,status:t.status,message:t.message}))})").unwrap();
        view.evaluate(include_str!("fixtures/css-wpt/cq-testcommon.js"))
            .unwrap();
        view.paint(800, 600, 1.0, 1.0, false).unwrap();
        for script in case["scripts"].as_array().unwrap() {
            view.evaluate(script.as_str().unwrap()).unwrap();
        }
        view.evaluate("done()").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let report = loop {
            view.poll();
            let report = view.evaluate_json("__wptResults").unwrap();
            if report != "null" {
                break serde_json::from_str::<serde_json::Value>(&report).unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "{} timed out: {:?}",
                case["file"],
                view.notices
            );
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(report["status"], 0, "{}: {report}", case["file"]);
        let tests = report["tests"].as_array().unwrap();
        assert!(!tests.is_empty());
        for test in tests {
            assert_eq!(test["status"], 0, "{}: {test}", case["file"]);
        }
        println!(
            "WPT {}: {}/{} passed",
            case["file"],
            tests.len(),
            tests.len()
        );
        total += tests.len();
    }
    assert!(total >= 7, "WPT subset must never shrink: {total} cases");
    println!("WPT CSS subset: {total}/{total} passed");
}

#[test]
fn pinned_css_overflow_clip_reftest_matches_hidden_reference() {
    fn pixels(html: &str) -> Vec<u8> {
        let mut view = Engine::new(
            Limits::default(),
            NetworkPolicy {
                offline: true,
                ..Default::default()
            },
            Arc::new(|| {}),
        )
        .unwrap()
        .create_view(Url::parse("about:blank").unwrap())
        .unwrap();
        view.set_html(html).unwrap();
        view.poll();
        view.paint(400, 400, 1.0, 1.0, false).unwrap().rgba
    }
    let actual = pixels(include_str!("fixtures/css-wpt/clip-001.html"));
    let reference = pixels(include_str!("fixtures/css-wpt/clip-001-ref.html"));
    assert_eq!(
        actual
            .iter()
            .zip(reference)
            .filter(|(a, b)| **a != *b)
            .count(),
        0
    );
}
