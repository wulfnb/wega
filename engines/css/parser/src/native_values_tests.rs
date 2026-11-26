//! Tests for `native_values`.
use super::*;

struct Never;
impl ValueEngine for Never {
    fn parse(&self, _: &str, _: &str) -> Option<Vec<(String, String)>> {
        Some(vec![("fallback".into(), String::new())])
    }
    fn serialize_shorthand(&self, _: &str, _: &[&str]) -> Option<String> {
        None
    }
}

#[test]
fn keyword_properties_are_native() {
    let engine = NativeFirst(Never);
    let one = |p: &str, v: &str| engine.parse(p, v).map(|v| v[0].1.clone());
    assert_eq!(
        one("background-attachment", "FIXED").as_deref(),
        Some("fixed")
    );
    assert_eq!(
        one("background-attachment", "scroll ,  fixed").as_deref(),
        Some("scroll, fixed")
    );
    assert_eq!(one("background-attachment", "sideways"), None);
    assert_eq!(one("background-attachment", "fixed fixed"), None);
    // Not ported yet: the fallback answers.
    assert_eq!(
        engine.parse("color", "rgb(1 2 3)").unwrap()[0].0,
        "fallback"
    );
}

#[test]
fn color_keywords_and_hex_serialize_like_stylo() {
    let engine = NativeFirst(Never);
    let native = |v: &str| engine.parse("color", v).map(|v| v[0].1.clone());
    assert_eq!(native("Red").as_deref(), Some("red"));
    assert_eq!(native("TRANSPARENT").as_deref(), Some("transparent"));
    assert_eq!(native("currentColor").as_deref(), Some("currentcolor"));
    assert_eq!(native("#0f0").as_deref(), Some("rgb(0, 255, 0)"));
    assert_eq!(native("#00ff0080").as_deref(), Some("rgba(0, 255, 0, 0.5)"));
    assert_eq!(
        native("#12345678").as_deref(),
        Some("rgba(18, 52, 86, 0.47)")
    );
    assert_eq!(native("#12"), None);
    assert_eq!(
        engine.parse("background-color", "Canvas").unwrap()[0].0,
        "fallback"
    );
}

#[test]
fn keyword_enum_types_are_native() {
    let engine = NativeFirst(Never);
    let native = |p: &str, v: &str| engine.parse(p, v).map(|v| v[0].1.clone());
    assert_eq!(
        native("float", "INLINE-START").as_deref(),
        Some("inline-start")
    );
    assert_eq!(native("user-select", "-moz-none").as_deref(), Some("none"));
    assert_eq!(
        native("border-top-style", "Dashed").as_deref(),
        Some("dashed")
    );
    assert_eq!(native("float", "middle"), None);
}

#[test]
fn numbers_serialize_like_stylo() {
    let engine = NativeFirst(Never);
    let native = |p: &str, v: &str| engine.parse(p, v).map(|v| v[0].1.clone());
    assert_eq!(native("flex-grow", "2.50").as_deref(), Some("2.5"));
    assert_eq!(native("flex-grow", "-1"), None);
    assert_eq!(native("order", "-3").as_deref(), Some("-3"));
    assert_eq!(native("order", "1.5"), None);
    assert_eq!(native("z-index", "AUTO").as_deref(), Some("auto"));
    assert_eq!(native("z-index", "10").as_deref(), Some("10"));
    assert_eq!(native("z-index", "1px"), None);
    assert_eq!(
        engine.parse("order", "calc(1 + 1)").unwrap()[0].0,
        "fallback"
    );
}

#[test]
fn times_opacity_line_height_weights_and_radii() {
    let engine = NativeFirst(Never);
    let native = |p: &str, v: &str| engine.parse(p, v).map(|v| v[0].1.clone());
    assert_eq!(
        native("transition-delay", "-1S, 200MS").as_deref(),
        Some("-1s, 200ms")
    );
    assert_eq!(native("transition-duration", "-1s"), None);
    assert_eq!(native("transition-duration", "1px"), None);
    assert_eq!(native("opacity", "50%").as_deref(), Some("0.5"));
    assert_eq!(native("opacity", "1.50").as_deref(), Some("1.5"));
    assert_eq!(native("line-height", "0").as_deref(), Some("0"));
    assert_eq!(native("line-height", "1.5em").as_deref(), Some("1.5em"));
    assert_eq!(native("line-height", "NORMAL").as_deref(), Some("normal"));
    assert_eq!(native("line-height", "-1"), None);
    assert_eq!(native("font-weight", "Bold").as_deref(), Some("bold"));
    assert_eq!(native("font-weight", "450").as_deref(), Some("450"));
    assert_eq!(native("font-weight", "1001"), None);
    assert_eq!(
        native("border-top-left-radius", "5px 5px").as_deref(),
        Some("5px")
    );
    assert_eq!(
        native("border-top-left-radius", "5px 10%").as_deref(),
        Some("5px 10%")
    );
    assert_eq!(native("border-top-left-radius", "-5px"), None);
}

#[test]
fn repeat_iteration_columns_caret_spacing() {
    let engine = NativeFirst(Never);
    let native = |p: &str, v: &str| engine.parse(p, v).map(|v| v[0].1.clone());
    assert_eq!(
        native("background-repeat", "repeat no-repeat, SPACE space").as_deref(),
        Some("repeat-x, space")
    );
    assert_eq!(
        native("background-repeat", "repeat-y").as_deref(),
        Some("repeat-y")
    );
    assert_eq!(native("background-repeat", "repeat-x repeat"), None);
    assert_eq!(
        native("animation-iteration-count", "INFINITE, 2.5").as_deref(),
        Some("infinite, 2.5")
    );
    assert_eq!(native("animation-iteration-count", "-1"), None);
    assert_eq!(native("animation-duration", "auto"), None);
    assert_eq!(native("animation-duration", "2s").as_deref(), Some("2s"));
    assert_eq!(native("column-count", "0"), None);
    assert_eq!(native("column-count", "3").as_deref(), Some("3"));
    assert_eq!(native("caret-color", "Auto").as_deref(), Some("auto"));
    assert_eq!(
        native("caret-color", "#fff").as_deref(),
        Some("rgb(255, 255, 255)")
    );
    assert_eq!(native("border-spacing", "2px 2px").as_deref(), Some("2px"));
    assert_eq!(native("border-spacing", "2px 10%"), None);
}

#[test]
fn position_components_serialize_as_written() {
    let engine = NativeFirst(Never);
    let native = |p: &str, v: &str| engine.parse(p, v).map(|v| v[0].1.clone());
    assert_eq!(
        native("background-position-x", "Center, RIGHT 10px, 0, left").as_deref(),
        Some("center, right 10px, 0px, left")
    );
    assert_eq!(
        native("background-position-y", "bottom 5%").as_deref(),
        Some("bottom 5%")
    );
    assert_eq!(native("background-position-x", "top"), None);
    assert_eq!(native("background-position-x", "10px left"), None);
}

#[test]
fn background_clip_and_size() {
    let engine = NativeFirst(Never);
    let native = |p: &str, v: &str| engine.parse(p, v).map(|v| v[0].1.clone());
    assert_eq!(
        native("background-clip", "TEXT, border-box").as_deref(),
        Some("text, border-box")
    );
    assert_eq!(native("mask-clip", "text"), None);
    assert_eq!(
        native("background-size", "10px").as_deref(),
        Some("10px auto")
    );
    assert_eq!(
        native("background-size", "auto AUTO").as_deref(),
        Some("auto")
    );
    assert_eq!(
        native("background-size", "Cover, 50% 10%").as_deref(),
        Some("cover, 50% 10%")
    );
    assert_eq!(native("background-size", "-1px"), None);
}

#[test]
fn lengths_serialize_like_stylo() {
    let engine = NativeFirst(Never);
    let one = |p: &str, v: &str| engine.parse(p, v).map(|v| v[0].clone());
    let native = |p: &str, v: &str| one(p, v).map(|(_, v)| v);
    assert_eq!(native("width", "10PX").as_deref(), Some("10px"));
    assert_eq!(native("width", "1.50em").as_deref(), Some("1.5em"));
    assert_eq!(native("margin-top", "0").as_deref(), Some("0px"));
    assert_eq!(native("margin-top", "-3Q").as_deref(), Some("-3q"));
    assert_eq!(native("margin-left", "AUTO").as_deref(), Some("auto"));
    assert_eq!(native("width", "33.333333%").as_deref(), Some("33.3333%"));
    assert_eq!(native("max-width", "none").as_deref(), Some("none"));
    assert_eq!(
        native("border-top-width", "Thick").as_deref(),
        Some("thick")
    );
    assert_eq!(engine.parse("width", "-1px"), None);
    assert_eq!(engine.parse("padding-top", "-1%"), None);
    assert_eq!(engine.parse("border-top-width", "10%"), None);
    // Left to the fallback: math functions, other keywords, unknown units.
    for (p, v) in [
        ("width", "calc(1px + 1%)"),
        ("width", "fit-content"),
        ("width", "1foo"),
    ] {
        assert_eq!(one(p, v).unwrap().0, "fallback", "{p}: {v}");
    }
}

#[test]
fn image_none_and_urls() {
    let engine = NativeFirst(Never);
    let native = |p: &str, v: &str| engine.parse(p, v).map(|v| v[0].1.clone());
    assert_eq!(
        native("background-image", "url(a.png), NONE, url( 'b \"c.png' )").as_deref(),
        Some("url(\"a.png\"), none, url(\"b \\\"c.png\")")
    );
    assert_eq!(
        native("list-style-image", "url(x.svg)").as_deref(),
        Some("url(\"x.svg\")")
    );
    assert_eq!(
        engine
            .parse("background-image", "linear-gradient(red, blue)")
            .unwrap()[0]
            .0,
        "fallback"
    );
}
