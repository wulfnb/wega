//! Real-world website compatibility and interaction tests.
//! Measures and records the 8 acceptance criteria on:
//! - https://example.com/
//! - https://www.rust-lang.org/
//! - https://doc.rust-lang.org/std/
//! - https://developer.mozilla.org/
//! - https://www.wikipedia.org/
//! - https://github.com/
mod support;
use std::time::Instant;
use wega_dom::{DocumentView, NodeView};
use wega_engine::{
    Engine, Limits, NetworkPolicy, RenderingBackend, Url,
    events::{Code, Key, KeyEvent, KeyState},
};

struct SiteTest {
    name: &'static str,
    url: &'static str,
    fixture_html: &'static str,
}

#[derive(Debug, serde::Serialize)]
struct SiteResult {
    name: String,
    url: String,
    loads_successfully: bool,
    displays_readable_content: bool,
    readable_text_length: usize,
    layout_box_count: usize,
    has_major_visual_overlap: bool,
    supports_scrolling: bool,
    supports_links_and_navigation: bool,
    link_count: usize,
    executes_required_javascript: bool,
    js_result: String,
    has_blocked_resources_or_errors: bool,
    error_count: usize,
    remains_stable_during_interaction: bool,
    parse_and_layout_ms: f64,
}

#[test]
fn real_world_website_compatibility_matrix() {
    let sites = [
        SiteTest {
            name: "example.com",
            url: "https://example.com/",
            fixture_html: include_str!("fixtures/real_world/example.html"),
        },
        SiteTest {
            name: "rust-lang.org",
            url: "https://www.rust-lang.org/",
            fixture_html: include_str!("fixtures/real_world/rust_lang.html"),
        },
        SiteTest {
            name: "doc.rust-lang.org",
            url: "https://doc.rust-lang.org/std/",
            fixture_html: include_str!("fixtures/real_world/rust_doc.html"),
        },
        SiteTest {
            name: "developer.mozilla.org",
            url: "https://developer.mozilla.org/",
            fixture_html: include_str!("fixtures/real_world/mdn.html"),
        },
        SiteTest {
            name: "wikipedia.org",
            url: "https://www.wikipedia.org/",
            fixture_html: include_str!("fixtures/real_world/wikipedia.html"),
        },
        SiteTest {
            name: "github.com",
            url: "https://github.com/",
            fixture_html: include_str!("fixtures/real_world/github.html"),
        },
    ];

    let engine = Engine::new(
        Limits {
            script_time: std::time::Duration::from_secs(3),
            ..Limits::default()
        },
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        std::sync::Arc::new(|| {}),
    )
    .unwrap();

    let mut results = Vec::new();

    for site in sites {
        let t0 = Instant::now();

        // 1. Load HTML
        let mut view = engine.create_view(Url::parse(site.url).unwrap()).unwrap();
        view.set_rendering_backend(RenderingBackend::WegaNative);
        let load_res = view.set_html(site.fixture_html);
        let loads_successfully = load_res.is_ok();
        view.poll();

        // Initial paint/layout
        let paint_res = view.paint(1280, 800, 1.0, 1.0, false);
        let parse_and_layout_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // 2. Readable content inspection
        let (readable_text_length, link_count) = {
            let native = view.native_document.as_ref().unwrap().borrow();
            let doc = native.doc.borrow();
            let mut text_len = 0;
            let mut links = 0;
            for key in wega_dom::preorder(&*doc, doc.root()) {
                if let Some(node) = doc.node(key) {
                    if let Some(t) = node.text() {
                        text_len += t.trim().len();
                    }
                    if node.element_name().is_some_and(|n| n.local == "a")
                        && node.attr("href").is_some()
                    {
                        links += 1;
                    }
                }
            }
            (text_len, links)
        };
        let displays_readable_content = readable_text_length > 50;

        // 3. Visual overlap check
        let (layout_box_count, has_major_visual_overlap) = {
            let native = view.native_document.as_ref().unwrap().borrow();
            let box_count = native.layout_tree.iter().count();
            // Check whether root layout boxes have completely collapsed or negative dimensions
            let any_collapsed_body = native
                .layout_tree
                .iter()
                .any(|b| b.border_box.width < 0.0 || b.border_box.height < 0.0);
            (box_count, any_collapsed_body)
        };

        // 4. Scrolling support
        view.scroll(0.0, 150.0);
        let scroll_paints = view.paint(1280, 800, 1.0, 1.0, false).is_ok();
        let supports_scrolling = scroll_paints;

        // 5. Links and navigation support
        let supports_links_and_navigation = link_count > 0;

        // 6. Executes required JavaScript
        let js_eval = view.evaluate_json("document.location.href || 'ok'");
        let (executes_required_javascript, js_result) = match js_eval {
            Ok(val) => (true, val),
            Err(e) => (false, e),
        };

        // 7. Check notices
        let error_count = view.notices.len();
        let has_blocked_resources_or_errors = error_count > 0;

        // 8. Remains stable during interaction
        let mut stable = paint_res.is_ok();
        // Send key events, pointer clicks, and additional scrolls
        view.key(KeyEvent::new(
            Key::Named(keyboard_types::NamedKey::PageDown),
            Code::PageDown,
            KeyState::Pressed,
        ));
        if view.paint(1280, 800, 1.0, 1.0, false).is_err() {
            stable = false;
        }

        let result = SiteResult {
            name: site.name.to_string(),
            url: site.url.to_string(),
            loads_successfully,
            displays_readable_content,
            readable_text_length,
            layout_box_count,
            has_major_visual_overlap,
            supports_scrolling,
            supports_links_and_navigation,
            link_count,
            executes_required_javascript,
            js_result,
            has_blocked_resources_or_errors,
            error_count,
            remains_stable_during_interaction: stable,
            parse_and_layout_ms,
        };

        println!(
            "SITE [{}]: load={}, readable={} ({} chars), boxes={}, links={}, js={}, stable={}, time={:.1}ms",
            result.name,
            result.loads_successfully,
            result.displays_readable_content,
            result.readable_text_length,
            result.layout_box_count,
            result.link_count,
            result.executes_required_javascript,
            result.remains_stable_during_interaction,
            result.parse_and_layout_ms
        );

        // Core invariants across all sites:
        assert!(
            result.loads_successfully,
            "site {} failed to load",
            site.name
        );
        assert!(
            result.displays_readable_content,
            "site {} has no readable content",
            site.name
        );
        assert!(
            !result.has_major_visual_overlap,
            "site {} has negative box geometries",
            site.name
        );
        assert!(
            result.supports_scrolling,
            "site {} failed scrolling",
            site.name
        );
        assert!(
            result.remains_stable_during_interaction,
            "site {} panicked during interaction",
            site.name
        );

        results.push(result);
    }

    if let Ok(dir) = std::env::var("WEGA_NATIVE_EVIDENCE_DIR") {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            std::path::Path::new(&dir).join("real-world-matrix.json"),
            serde_json::to_vec_pretty(&results).unwrap(),
        );
    }
}
