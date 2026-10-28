//! CSS pixel regressions are reftests: actual and reference use the same fonts,
//! dimensions and Skia path. No platform-specific screenshot tolerances hide failures.
use std::sync::Arc;
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};
fn page(html: &str) -> View {
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
    view.set_html(&format!(
        "<!doctype html><style>body{{margin:0}}div{{box-sizing:border-box}}</style>{html}"
    ))
    .unwrap();
    view.poll();
    view
}
fn pixels(html: &str) -> Vec<u8> {
    page(html).paint(64, 64, 1.0, 1.0, false).unwrap().rgba
}
#[test]
fn floated_navigation_items_share_a_row_and_clear_starts_below() {
    let mut view = page(
        "<style>nav{width:240px;display:flow-root}a{float:left;width:60px;height:30px}footer{clear:both;height:10px}</style><nav><a id=a>A</a><a id=b>B</a><a id=c>C</a><footer id=f></footer></nav>",
    );
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    assert_eq!(view.evaluate_json("['a','b','c','f'].map(id=>{const r=document.getElementById(id).getBoundingClientRect();return [r.x,r.y]})").unwrap(), "[[0,0],[60,0],[120,0],[0,30]]");
}

#[test]
fn css_transition_keeps_painting_until_finished_then_idles() {
    let mut view = page(
        "<style>#cursor{width:20px;height:20px;background:lime;transition:transform .08s linear}</style><div id=cursor></div>",
    );
    view.paint(100, 100, 1.0, 1.0, false).unwrap();
    assert!(!view.needs_paint());
    view.evaluate("document.getElementById('cursor').style.transform='translate3d(60px,0,0)'")
        .unwrap();
    let first = view.paint(100, 100, 1.0, 1.0, false).unwrap().rgba;
    assert!(
        view.needs_paint(),
        "CSS-only transitions must request another frame"
    );
    std::thread::sleep(std::time::Duration::from_millis(120));
    let last = view.paint(100, 100, 1.0, 1.0, false).unwrap().rgba;
    assert_ne!(first, last);
    assert!(
        !view.needs_paint(),
        "completed transitions must stop rendering"
    );
    assert_eq!(
        &last[(10 * 100 + 70) * 4..(10 * 100 + 70) * 4 + 4],
        &[0, 255, 0, 255]
    );
}
fn same(actual: Vec<u8>, reference: Vec<u8>) {
    let diff = actual
        .iter()
        .zip(&reference)
        .filter(|(a, b)| a != b)
        .count();
    if diff > 0 {
        std::fs::write("/tmp/wega-css-actual.rgba", &actual).unwrap();
        std::fs::write("/tmp/wega-css-reference.rgba", &reference).unwrap();
    }
    assert_eq!(
        diff, 0,
        "differing channels; actual/reference raw RGBA saved under /tmp/wega-css-*.rgba"
    );
}
fn green() -> Vec<u8> {
    pixels("<div style='width:64px;height:64px;background:lime'></div>")
}
#[test]
fn skia_color_filter_and_blend_match_reference_pixels() {
    same(
        pixels("<div style='width:64px;height:64px;background:magenta;filter:invert(1)'></div>"),
        green(),
    );
    same(
        pixels(
            "<div style='width:64px;height:64px;background:yellow'><div style='width:64px;height:64px;background:cyan;mix-blend-mode:multiply'></div></div>",
        ),
        green(),
    );
}
#[test]
fn text_shadow_matches_positioned_glyph_reference() {
    let actual = pixels(
        "<div style='font:20px sans-serif;color:transparent;text-shadow:2px 3px red'>ABC</div>",
    );
    let reference = pixels(
        "<div style='font:20px sans-serif;color:red;position:relative;left:2px;top:3px'>ABC</div>",
    );
    same(actual, reference);
}
#[test]
fn overflow_auto_scrolls_the_pointed_container_without_scrolling_the_viewport() {
    let mut view = page(
        "<div style='width:64px;height:32px;overflow:auto'><div style='height:32px;background:red'></div><div style='height:32px;background:lime'></div></div>",
    );
    view.paint(64, 64, 1.0, 1.0, false).unwrap();
    view.scroll_at(0.0, 32.0, 8.0, 8.0);
    let frame = view.paint(64, 64, 1.0, 1.0, false).unwrap();
    assert_eq!(
        &frame.rgba[(8 * 64 + 8) * 4..(8 * 64 + 8) * 4 + 4],
        &[0, 255, 0, 255]
    );
}
#[test]
fn inline_size_container_query_uses_content_size_and_updates_after_resize() {
    let mut view = page(
        "<style>#c{container-type:inline-size;width:64px;height:64px}#t{width:64px;height:64px;background:red}@container(min-width:50px){#t{background:lime}}</style><div id=c><div id=t></div></div>",
    );
    let frame = view.paint(64, 64, 1.0, 1.0, false).unwrap();
    same(frame.rgba, green());
    view.evaluate("document.getElementById('c').style.width='32px'")
        .unwrap();
    let frame = view.paint(64, 64, 1.0, 1.0, false).unwrap();
    assert_eq!(&frame.rgba[0..4], &[255, 0, 0, 255]);
}
#[test]
fn sticky_header_stays_at_the_viewport_edge_and_does_not_accumulate_offsets() {
    let mut view = page(
        "<div style='position:sticky;top:0;height:16px;background:lime'></div><div style='height:256px;background:red'></div>",
    );
    view.paint(64, 64, 1.0, 1.0, false).unwrap();
    view.scroll(0.0, 32.0);
    let first = view.paint(64, 64, 1.0, 1.0, false).unwrap();
    assert_eq!(&first.rgba[0..4], &[0, 255, 0, 255]);
    same(
        first.rgba,
        view.paint(64, 64, 1.0, 1.0, false).unwrap().rgba,
    );
}
#[test]
fn ellipsis_keeps_dom_text_and_matches_a_positioned_reference() {
    let mut view = page(
        "<div id=t style='width:64px;font:20px/24px monospace;white-space:nowrap;overflow:hidden;text-overflow:ellipsis'>ABCDEFGHIJK</div>",
    );
    let actual = view.paint(64, 64, 1.0, 1.0, false).unwrap().rgba;
    assert_eq!(view.text("#t").as_deref(), Some("ABCDEFGHIJK"));
    let reference = pixels(
        "<div style='position:relative;width:64px;font:20px/24px monospace;white-space:nowrap'>ABCD<span style='position:absolute;right:0;top:0'>…</span></div>",
    );
    same(actual, reference);
}
#[test]
fn line_clamp_sets_layout_height_and_draws_ellipsis_without_changing_dom() {
    let mut view = page(
        "<div id=t style='line-clamp:2;width:64px;font:20px/24px monospace;overflow:hidden'>ABCD EFGH IJKL</div>",
    );
    let actual = view.paint(64, 64, 1.0, 1.0, false).unwrap().rgba;
    assert_eq!(view.text("#t").as_deref(), Some("ABCD EFGH IJKL"));
    assert_eq!(
        view.evaluate_json("document.getElementById('t').getBoundingClientRect().height")
            .unwrap(),
        "48"
    );
    let reference = pixels(
        "<div style='width:64px;font:20px/24px monospace'><div>ABCD</div><div style='position:relative'>EFGH<span style='position:absolute;right:0;top:0'>…</span></div></div>",
    );
    same(actual, reference);
}
#[test]
fn backdrop_filter_and_transparent_glyphs_preserve_alpha() {
    let actual = pixels(
        "<div style='width:64px;height:64px;background:magenta'><div style='width:64px;height:64px;backdrop-filter:invert(1)'></div></div>",
    );
    same(actual, green());
    assert!(
        pixels("<div style='color:transparent;font:20px sans-serif'>Invisible</div>")
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 0)
    );
}

#[test]
fn default_text_color_follows_the_root_color_scheme_and_author_colors_win() {
    let mut actual = page(
        "<style>html{color-scheme:light dark;background:light-dark(white,black)}</style><div style='font:20px monospace'>ABC</div>",
    );
    let actual = actual.paint(64, 64, 1.0, 1.0, true).unwrap().rgba;
    let reference = pixels(
        "<style>html{background:black;color:rgb(232,232,232)}</style><div style='font:20px monospace'>ABC</div>",
    );
    same(actual, reference);
}
#[test]
fn viewport_units_are_css_pixels_on_hidpi_displays() {
    let mut view = page(
        "<style>body{padding-top:25vh}</style><div style='width:100%;height:10px;background:lime'></div>",
    );
    // Sites can force layout before the embedder receives its first resize.
    view.evaluate_json("document.body.getBoundingClientRect()")
        .unwrap();
    let frame = view.paint(200, 200, 2.0, 1.0, false).unwrap();
    assert_eq!(
        &frame.rgba[(55 * 200 + 8) * 4..(55 * 200 + 8) * 4 + 4],
        &[0, 255, 0, 255]
    );
    assert_eq!(
        view.evaluate_json("getComputedStyle(document.body).getPropertyValue('padding-top')")
            .unwrap(),
        "\"25px\""
    );
}

#[test]
fn inserted_svg_does_not_duplicate_the_body_or_its_padding() {
    let mut view = page(
        "<style>body{padding-top:50px}p{margin:0}</style><p>reference text</p><script>document.body.insertAdjacentHTML('afterbegin',`<style>svg{display:block;margin:-44px auto 0}</style><svg width=44 height=44><rect width=44 height=44 fill=lime /></svg>`)</script>",
    );
    let frame = view.paint(200, 200, 1.0, 1.0, false).unwrap();
    assert_eq!(
        view.evaluate_json("document.querySelector('svg').getBoundingClientRect().y")
            .unwrap(),
        "6"
    );
    assert_eq!(
        &frame.rgba[(8 * 200 + 100) * 4..(8 * 200 + 100) * 4 + 4],
        &[0, 255, 0, 255]
    );
    assert_eq!(
        view.evaluate_json("document.querySelectorAll('body').length")
            .unwrap(),
        "1"
    );
}

#[test]
fn collapsed_select_paints_only_the_selected_label_and_preserves_dom() {
    let mut view = page(
        "<style>select{font:16px/20px monospace;border:0;padding:0;background:transparent}</style><select id=s><option>Other</option><optgroup label=group><option selected>English (en-US)</option><option>French</option></optgroup></select>",
    );
    let first = view.paint(180, 40, 1.0, 1.0, false).unwrap().rgba;
    let mut reference = page(
        "<span style='display:inline-block;white-space:nowrap;font:16px/20px monospace'>English (en-US)</span>",
    );
    same(
        first,
        reference.paint(180, 40, 1.0, 1.0, false).unwrap().rgba,
    );
    drop(reference);
    assert_eq!(
        view.evaluate_json("document.querySelectorAll('option').length")
            .unwrap(),
        "3"
    );
    view.evaluate("document.querySelectorAll('option')[1].selected=false;document.querySelectorAll('option')[2].selected=true").unwrap();
    let last = view.paint(180, 40, 1.0, 1.0, false).unwrap().rgba;
    let mut reference = page(
        "<span style='display:inline-block;white-space:nowrap;font:16px/20px monospace'>French</span>",
    );
    same(
        last,
        reference.paint(180, 40, 1.0, 1.0, false).unwrap().rgba,
    );
}
