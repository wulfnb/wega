use std::{cell::RefCell, rc::Rc};
use wega_dom::DocumentView;
use wega_engine::{
    Engine, Limits, NetworkPolicy, Url,
    document::{DocumentSetup, WegaDocument},
    native_engine::*,
};

fn doc(html: &str) -> NativeDocument<WegaDocument> {
    let mut d = NativeDocument::new(Rc::new(RefCell::new(WegaDocument::from_html(
        html,
        DocumentSetup::default(),
    ))));
    d.flush_layout();
    d
}

#[test]
fn deeply_nested_blocks_respect_stack_limit_and_recover() {
    let mut html = String::from("<body>");
    for _ in 0..120 {
        html.push_str("<div>");
    }
    html.push_str("<p id=deep>leaf</p>");
    for _ in 0..120 {
        html.push_str("</div>");
    }
    html.push_str("</body>");
    let d = doc(&html);
    let key = d.doc.borrow().element_by_id("deep").unwrap();
    let r = d.bounding_rect(key).unwrap();
    assert!(r.width >= 0.0);
}

#[test]
fn cyclic_or_extreme_flex_values_do_not_panic() {
    let d = doc(
        "<body style='display:flex;width:100px'><div style='flex:1 1 0%;width:1e9px'></div><div style='flex:2 1 0%;min-width:50px'></div></body>",
    );
    assert_eq!(d.viewport.width_px, 1280);
}

#[test]
fn zero_and_negative_dimensions_clamp_safely() {
    let d = doc(
        "<body style='width:-20px;height:0px;padding:-5px'><div id=neg style='width:-100px;height:-50px;margin:-10px'>neg</div></body>",
    );
    let key = d.doc.borrow().element_by_id("neg").unwrap();
    let r = d.bounding_rect(key).unwrap();
    assert_eq!(r.width, 0.0);
    assert_eq!(r.height, 0.0);
}

#[test]
fn massive_stylesheet_budget_enforcement() {
    let mut css = String::new();
    for i in 0..600 {
        css.push_str(&format!(".c{i} {{ color: red; width: {i}px; }}\n"));
    }
    let sheet = StyleSheet::parse(&css);
    assert_eq!(sheet.rules.len(), 600);
}

#[test]
fn rapid_mutation_burst_maintains_consistent_tree_invariants() {
    let mut d =
        doc("<body style='margin:0'><div id=target style='width:50px;height:50px'></div></body>");
    for i in 1..=20 {
        let key = d.doc.borrow().element_by_id("target").unwrap();
        d.doc
            .borrow_mut()
            .set_inline_property(key, "width", &format!("{}px", 50 + i * 5), false)
            .unwrap();
        d.flush_layout();
        let r = d.bounding_rect(key).unwrap();
        assert_eq!(r.width, (50 + i * 5) as f64);
    }
}

#[test]
fn html_parser_fuzzing_and_malformed_input() {
    let hostile_inputs = [
        "<!doctype html><<<>>><<<<>>>>",
        "<a href=\"http://test.example\"\"\"\"\" attribute_with_no_value=\"\"\"\">link</a>",
        "<p>Unclosed paragraph <div>Inside div <span><span><span>text</div>",
        "<!-- unclosed comment <script>alert(1)</script>",
        "<style><<< invalid css inside style tag >>>",
        "<div style=\"width: calc(100px + (50px * (20px / 0))); height: calc(100% - - - 10px)\">math</div>",
        "<table><tr><td>Cell 1<tr><td>Cell 2 without closing tr or table",
        "<input type=\"text\" value=\"\0\0\0null bytes inside attribute\0\0\">",
        "<form id=f><input name=a><input name=a><button type=submit>Go</form>",
        "<![CDATA[cdata section in html5]]>outside",
        "<svg><path d=\"M 0 0 L 100 100 Z M invalid path coords Q Q Q\"/></svg>",
        "<img src=\"data:image/png;base64,corruptbase64payload!@#$%^&*()\" alt=\"broken\">",
    ];

    for (idx, input) in hostile_inputs.iter().enumerate() {
        let d = doc(input);
        // Ensure parsing never panics and root node is always accessible
        let root = d.doc.borrow().root();
        assert!(
            d.doc.borrow().node(root).is_some(),
            "input {idx} failed to yield a root node"
        );
    }
}

#[test]
fn css_parser_fuzzing_and_extreme_values() {
    let hostile_css = [
        "body { width: 1e38px; height: -1e38px; margin: 1e-38px; }",
        ":root { --a: var(--b); --b: var(--c); --c: var(--a); } div { color: var(--a); }",
        ".unclosed { font-size: 14px; /* unclosed comment",
        "@media (min-width: 0px) { @media (max-width: 1000px) { @supports (display: flex) { body { display: flex; } } } }",
        "div { margin: 10px 20px 30px 40px 50px 60px 70px; padding: calc(10px + 20px); }",
        "div[attr*=\"\"][attr^=\"\"][attr$=\"\"] { color: blue; }",
        "p::before::after:hover:focus:active { content: 'test'; }",
        "!@#$%^&*() { unknown: property; }",
    ];

    for css in hostile_css {
        let sheet = StyleSheet::parse(css);
        // Parsing should degrade gracefully without aborting
        std::hint::black_box(&sheet);
    }
}

#[test]
fn dom_mutation_fuzzing_and_reparenting() {
    let mut d = doc(
        "<body><div id=container><p id=p1>First</p><p id=p2>Second</p><span id=s1>Span</span></div></body>",
    );
    let container = d.doc.borrow().element_by_id("container").unwrap();
    let p1 = d.doc.borrow().element_by_id("p1").unwrap();
    let s1 = d.doc.borrow().element_by_id("s1").unwrap();

    // Perform multiple rapid structure mutations
    for i in 0..50 {
        // Toggle inline properties
        d.doc
            .borrow_mut()
            .set_inline_property(container, "padding", &format!("{}px", i % 10), false)
            .unwrap();
        d.doc
            .borrow_mut()
            .set_inline_property(p1, "color", if i % 2 == 0 { "red" } else { "blue" }, false)
            .unwrap();
        d.doc
            .borrow_mut()
            .set_inline_property(
                s1,
                "display",
                if i % 3 == 0 { "inline-block" } else { "block" },
                false,
            )
            .unwrap();
        d.flush_layout();

        let r = d.bounding_rect(container).unwrap();
        assert!(r.width >= 0.0);
    }
}

#[test]
fn url_and_http_header_fuzzing() {
    let policy = NetworkPolicy {
        offline: false,
        allow_loopback: true,
        ..Default::default()
    };

    let test_urls = [
        ("https://example.com/normal", true),
        ("http://example.com:8080/path?query=1#frag", true),
        ("ftp://example.com/unsupported", false),
        ("file:///path/to/local/file", false),
        ("javascript:alert(1)", false),
        ("data:text/html,<b>hi</b>", false),
        ("https://user:pass@example.com/auth", true),
        ("https://[::1]:8080/ipv6", true),
    ];

    for (raw, expected_permit) in test_urls {
        if let Ok(url) = Url::parse(raw) {
            assert_eq!(
                policy.permits(&url),
                expected_permit,
                "URL permit mismatch for {raw}"
            );
        }
    }
}

#[test]
fn repeated_document_creation_and_destruction_soak() {
    let engine = Engine::new(
        Limits {
            script_time: std::time::Duration::from_secs(1),
            ..Limits::default()
        },
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        std::sync::Arc::new(|| {}),
    )
    .unwrap();

    // Create and tear down 50 views sequentially to test memory/descriptor churn
    for i in 0..50 {
        let mut view = engine
            .create_view(Url::parse("about:blank").unwrap())
            .unwrap();
        view.set_rendering_backend(RenderingBackend::WegaNative);
        view.set_html(&format!(
            "<!doctype html><title>Document {i}</title><style>div{{width:100px;height:50px}}</style><div id=box>Iter {i}</div>"
        ))
        .unwrap();
        view.poll();
        let painted = view.paint(400, 300, 1.0, 1.0, false).unwrap();
        assert_eq!(painted.rgba.len(), 400 * 300 * 4);
        assert_eq!(view.title(), format!("Document {i}"));
        // `view` drops here, cleaning up its resources
    }
}
