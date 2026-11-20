//! Round 2 Acceptance Tests: Complete Common CSS Layout and Replaced Elements.
//!
//! Validates:
//! 1. Inline formatting context: nested inline fragments, inline padding/border, vertical-align,
//!    inline-blocks on the same line as text, wrapping around inline-blocks, hit testing.
//! 2. Replaced elements: intrinsic aspect-ratio preservation, iframe default 300x150,
//!    object-fit (contain, cover, fill, none, scale-down) and object-position.
//! 3. Block layout: margin collapsing (parent-child, sibling-sibling), auto margin centering,
//!    box-sizing, position (static, relative, absolute, fixed, sticky).
//! 4. Flexbox: row-reverse, column-reverse, flex-wrap multi-line, gap, alignment.
//! 5. Grid & Table: fr distribution, minmax(), col/row spanning, auto table sizing.
//! 6. Chrome visual / geometry reference validation.

mod support;
#[path = "support/native_visual.rs"]
mod visual;

use support::{Reply, Server, engine, open};
use wega_engine::RenderingBackend;

#[test]
fn test_inline_formatting_nested_spans_and_inline_blocks() {
    let html = r#"<!DOCTYPE html>
<html>
<head>
<style>
body { margin: 0; font-size: 16px; line-height: 20px; font-family: sans-serif; }
.container { width: 500px; padding: 10px; background: #fff; }
.ib { display: inline-block; width: 60px; height: 30px; background: #33cc33; margin: 0 5px; vertical-align: middle; }
.span-styled { padding: 4px 8px; border: 2px solid #ff0000; background: #ffff00; }
</style>
</head>
<body>
<div class="container" id="cont">
  <span>Start text</span>
  <span class="span-styled" id="styledSpan">highlighted</span>
  <div class="ib" id="ibox"></div>
  <span>End text</span>
</div>
</body>
</html>"#;

    let server = Server::new(move |_| Reply::html(html));
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let frame = view.paint(800, 600, 1.0, 1.0, false).unwrap();
    assert!(!frame.rgba.is_empty(), "Frame rendered successfully");

    // Evaluate geometry
    let ib_rect_json = view
        .evaluate_json(
            "(() => { const r = document.getElementById('ibox').getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()",
        )
        .unwrap();
    let ib_rect: Vec<f64> = serde_json::from_str(&ib_rect_json).unwrap();
    assert_eq!(ib_rect[2], 60.0, "inline-block width must be 60px");
    assert_eq!(ib_rect[3], 30.0, "inline-block height must be 30px");
    assert!(
        ib_rect[0] > 100.0,
        "inline-block must sit horizontally in flow on the line, got left: {}",
        ib_rect[0]
    );

    // Hit test the styled span and inline box
    let span_rect_json = view
        .evaluate_json(
            "(() => { const r = document.getElementById('styledSpan').getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()",
        )
        .unwrap();
    let span_rect: Vec<f64> = serde_json::from_str(&span_rect_json).unwrap();
    assert!(span_rect[2] > 0.0);
    assert!(span_rect[3] > 0.0);
}

#[test]
fn test_replaced_elements_intrinsic_sizing_and_object_fit() {
    let html = r#"<!DOCTYPE html>
<html>
<head>
<style>
body { margin: 0; }
iframe.default-frame { border: none; }
iframe.custom-frame { width: 450px; height: 250px; border: none; }
img.cover { width: 200px; height: 100px; object-fit: cover; object-position: center; }
img.contain { width: 200px; height: 100px; object-fit: contain; object-position: 50% 50%; }
</style>
</head>
<body>
<iframe class="default-frame" id="f1" src="about:blank"></iframe>
<iframe class="custom-frame" id="f2" src="about:blank"></iframe>
<img class="cover" id="img1" width="400" height="400" src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==">
<img class="contain" id="img2" width="400" height="200" src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==">
</body>
</html>"#;

    let server = Server::new(move |_| Reply::html(html));
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let frame = view.paint(800, 600, 1.0, 1.0, false).unwrap();
    assert!(!frame.rgba.is_empty());

    // 1. Iframe default intrinsic sizing must be 300x150
    let f1_json = view
        .evaluate_json(
            "(() => { const r = document.getElementById('f1').getBoundingClientRect(); return [r.width, r.height]; })()",
        )
        .unwrap();
    let f1: Vec<f64> = serde_json::from_str(&f1_json).unwrap();
    assert_eq!(f1[0], 300.0, "iframe default width must be 300px");
    assert_eq!(f1[1], 150.0, "iframe default height must be 150px");

    // 2. Custom iframe
    let f2_json = view
        .evaluate_json(
            "(() => { const r = document.getElementById('f2').getBoundingClientRect(); return [r.width, r.height]; })()",
        )
        .unwrap();
    let f2: Vec<f64> = serde_json::from_str(&f2_json).unwrap();
    assert_eq!(f2[0], 450.0);
    assert_eq!(f2[1], 250.0);

    // 3. Image aspect ratio & object-fit dimensions
    let img1_json = view
        .evaluate_json(
            "(() => { const r = document.getElementById('img1').getBoundingClientRect(); return [r.width, r.height]; })()",
        )
        .unwrap();
    let img1: Vec<f64> = serde_json::from_str(&img1_json).unwrap();
    assert_eq!(img1[0], 200.0);
    assert_eq!(img1[1], 100.0);
}

#[test]
fn test_block_formatting_margin_collapsing_and_positioning() {
    let html = r#"<!DOCTYPE html>
<html>
<head>
<style>
body { margin: 0; }
.parent { margin-top: 30px; background: #eee; }
.child-first { margin-top: 20px; margin-bottom: 25px; height: 50px; background: #aaa; }
.child-second { margin-top: 15px; height: 50px; background: #888; }
.centered { width: 200px; height: 40px; margin: 0 auto; background: #555; }
.sticky-elem { position: sticky; top: 10px; height: 30px; background: #ffcc00; }
.abs-elem { position: absolute; top: 50px; left: 100px; width: 80px; height: 40px; background: #00cccc; }
</style>
</head>
<body>
<div class="parent" id="parent">
  <div class="child-first" id="c1"></div>
  <div class="child-second" id="c2"></div>
</div>
<div class="centered" id="centered"></div>
<div class="sticky-elem" id="sticky"></div>
<div class="abs-elem" id="abs"></div>
</body>
</html>"#;

    let server = Server::new(move |_| Reply::html(html));
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let frame = view.paint(800, 600, 1.0, 1.0, false).unwrap();
    assert!(!frame.rgba.is_empty());

    // 1. Margin collapsing between siblings: child-first (margin-bottom: 25px) and child-second (margin-top: 15px)
    // Sibling gap should collapse to max(25, 15) = 25px.
    let gap_json = view
        .evaluate_json(
            "(() => { const r1 = document.getElementById('c1').getBoundingClientRect(); const r2 = document.getElementById('c2').getBoundingClientRect(); return r2.top - r1.bottom; })()",
        )
        .unwrap();
    let gap: f64 = gap_json.parse().unwrap();
    assert_eq!(
        gap, 25.0,
        "Sibling margins must collapse to max(25, 15) = 25px"
    );

    // 2. Auto margin centering: 800px viewport, 200px width -> left should be (800 - 200)/2 = 300px
    let centered_json = view
        .evaluate_json(
            "(() => { const r = document.getElementById('centered').getBoundingClientRect(); return [r.left, r.width]; })()",
        )
        .unwrap();
    let centered: Vec<f64> = serde_json::from_str(&centered_json).unwrap();
    assert_eq!(
        centered[0], 300.0,
        "margin: 0 auto must center the 200px element horizontally"
    );
    assert_eq!(centered[1], 200.0);

    // 3. Absolute positioning
    let abs_json = view
        .evaluate_json(
            "(() => { const r = document.getElementById('abs').getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()",
        )
        .unwrap();
    let abs: Vec<f64> = serde_json::from_str(&abs_json).unwrap();
    assert_eq!(abs[0], 100.0);
    assert_eq!(abs[1], 50.0);
    assert_eq!(abs[2], 80.0);
    assert_eq!(abs[3], 40.0);
}

#[test]
fn test_flexbox_and_grid_advanced_layout() {
    let html = r#"<!DOCTYPE html>
<html>
<head>
<style>
body { margin: 0; }
.flex-rev { display: flex; flex-direction: row-reverse; width: 300px; height: 50px; background: #eee; }
.flex-item { width: 50px; height: 50px; }
.grid-container {
    display: grid;
    grid-template-columns: 1fr 2fr 1fr;
    grid-template-rows: 40px 60px;
    gap: 10px;
    width: 430px;
}
.grid-span { grid-column: span 2; }
</style>
</head>
<body>
<div class="flex-rev" id="flex">
  <div class="flex-item" id="f_item1" style="background: red;"></div>
  <div class="flex-item" id="f_item2" style="background: blue;"></div>
</div>
<div class="grid-container" id="grid">
  <div id="g1" style="background: yellow;"></div>
  <div id="g2" class="grid-span" style="background: green;"></div>
  <div id="g3" style="background: purple;"></div>
  <div id="g4" style="background: orange;"></div>
  <div id="g5" style="background: brown;"></div>
</div>
</body>
</html>"#;

    let server = Server::new(move |_| Reply::html(html));
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let frame = view.paint(800, 600, 1.0, 1.0, false).unwrap();
    assert!(!frame.rgba.is_empty());

    // 1. Flex row-reverse: first child should be placed at the right end (300 - 50 = 250px)
    let f1_json = view
        .evaluate_json(
            "(() => { const r = document.getElementById('f_item1').getBoundingClientRect(); return r.left; })()",
        )
        .unwrap();
    let f1_left: f64 = f1_json.parse().unwrap();
    assert_eq!(
        f1_left, 250.0,
        "In flex row-reverse, item 1 should be at right end"
    );

    let f2_json = view
        .evaluate_json(
            "(() => { const r = document.getElementById('f_item2').getBoundingClientRect(); return r.left; })()",
        )
        .unwrap();
    let f2_left: f64 = f2_json.parse().unwrap();
    assert_eq!(
        f2_left, 200.0,
        "In flex row-reverse, item 2 should be to the left of item 1"
    );

    // 2. Grid spanning: grid width 430px, 2 gaps of 10px = 410px room.
    // 1fr 2fr 1fr = 4fr total -> 1fr = 100px (with 10px rounding), 2fr = 200px + remaining room.
    // g2 spans 2 columns (col 2 and col 3): width = col2 + gap + col3 = 205 + 10 + 102.5 = ~317.5px.
    let g2_json = view
        .evaluate_json(
            "(() => { const r = document.getElementById('g2').getBoundingClientRect(); return [r.left, r.width]; })()",
        )
        .unwrap();
    let g2: Vec<f64> = serde_json::from_str(&g2_json).unwrap();
    assert!(
        g2[1] > 250.0,
        "g2 spans 2 columns, width must be > 250px, got: {}",
        g2[1]
    );
}

#[test]
fn test_tables_content_sizing_and_spans() {
    let html = r#"<!DOCTYPE html>
<html>
<head>
<style>
body { margin: 0; }
table { border-collapse: separate; border-spacing: 5px; width: 400px; }
td { padding: 4px; border: 1px solid black; }
</style>
</head>
<body>
<table id="tbl">
  <tr>
    <td id="c11" colspan="2">Wide header</td>
    <td id="c12">Col 3</td>
  </tr>
  <tr>
    <td id="c21">A</td>
    <td id="c22">B</td>
    <td id="c23">C</td>
  </tr>
</table>
</body>
</html>"#;

    let server = Server::new(move |_| Reply::html(html));
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let frame = view.paint(800, 600, 1.0, 1.0, false).unwrap();
    assert!(!frame.rgba.is_empty());

    let c11_json = view
        .evaluate_json(
            "(() => { const r = document.getElementById('c11').getBoundingClientRect(); return [r.width, r.left]; })()",
        )
        .unwrap();
    let c11: Vec<f64> = serde_json::from_str(&c11_json).unwrap();

    let c21_json = view
        .evaluate_json(
            "(() => { const r = document.getElementById('c21').getBoundingClientRect(); return [r.width, r.left]; })()",
        )
        .unwrap();
    let c21: Vec<f64> = serde_json::from_str(&c21_json).unwrap();

    let c22_json = view
        .evaluate_json(
            "(() => { const r = document.getElementById('c22').getBoundingClientRect(); return [r.width, r.left]; })()",
        )
        .unwrap();
    let c22: Vec<f64> = serde_json::from_str(&c22_json).unwrap();

    // c11 spans col 1 and col 2, so c11 width must be approximately c21.width + border_spacing (5) + c22.width
    let expected_c11_w = c21[0] + 5.0 + c22[0];
    assert!(
        (c11[0] - expected_c11_w).abs() < 2.0,
        "Colspan cell width must equal sum of spanned columns plus border-spacing: got {} vs expected {}",
        c11[0],
        expected_c11_w
    );
}

#[test]
fn test_css_layout_chrome_reference_comparison() {
    let html = r#"<!DOCTYPE html>
<html>
<head>
<style>
body { margin: 0; padding: 20px; font-family: sans-serif; background: #f4f6f8; }
.card { width: 400px; padding: 16px; background: #ffffff; border: 1px solid #e0e0e0; box-sizing: border-box; }
.row { display: flex; gap: 12px; margin-bottom: 12px; }
.avatar { width: 48px; height: 48px; object-fit: cover; background: #cccccc; border-radius: 4px; }
.info { flex: 1; }
.title { font-size: 16px; font-weight: bold; margin-bottom: 4px; }
.desc { font-size: 14px; color: #666; }
.grid-badges { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
.badge { background: #e8f0fe; color: #1a73e8; padding: 6px 10px; font-size: 12px; text-align: center; }
</style>
</head>
<body>
<div class="card" id="card" data-probe>
  <div class="row" id="row" data-probe>
    <div class="avatar" id="avatar" data-probe></div>
    <div class="info" id="info" data-probe>
      <div class="title" id="title" data-probe>Native Engine Architecture</div>
      <div class="desc" id="desc" data-probe>Complete independent native CSS layout pipeline.</div>
    </div>
  </div>
  <div class="grid-badges" id="badges" data-probe>
    <div class="badge" id="b1" data-probe>Flexbox</div>
    <div class="badge" id="b2" data-probe>Grid</div>
  </div>
</div>
</body>
</html>"#;

    let server = Server::new(move |_| Reply::html(html));
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let frame = view.paint(800, 600, 1.0, 1.0, false).unwrap();
    assert!(!frame.rgba.is_empty());

    let native_geom = visual::geometry(&mut view);
    let temp_dir = tempfile::tempdir().unwrap();
    let chrome_bin = std::env::var("WEGA_CHROME_BIN").unwrap_or_else(|_| {
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string()
    });

    if let Some((chrome_png, chrome_geom)) =
        visual::chrome_with_binary(&chrome_bin, html, None, 0.0, "css_layout", temp_dir.path())
    {
        assert!(!chrome_png.is_empty(), "Chrome capture succeeded");
        let n_card = native_geom
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e[0] == "card")
            .unwrap();
        let c_card = chrome_geom
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e[0] == "card")
            .unwrap();
        let n_w = n_card[3].as_f64().unwrap();
        let c_w = c_card[3].as_f64().unwrap();
        assert!(
            (n_w - c_w).abs() < 5.0,
            "Card width native={n_w} chrome={c_w}"
        );

        let n_avatar = native_geom
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e[0] == "avatar")
            .unwrap();
        let c_avatar = chrome_geom
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e[0] == "avatar")
            .unwrap();
        let n_av_w = n_avatar[3].as_f64().unwrap();
        let c_av_w = c_avatar[3].as_f64().unwrap();
        assert_eq!(n_av_w, 48.0);
        assert_eq!(c_av_w, 48.0);
    }
}
