//! Tests for `color`.
use super::Color;

fn rgb(r: u8, g: u8, b: u8) -> Option<Color> {
    Some(Color { r, g, b, a: 255 })
}

#[test]
fn legacy_rgb_rejects_mixed_numbers_and_percentages() {
    for invalid in [
        "rgb(255, 0, 0%)",
        "rgb(100%, 0%, 0)",
        "rgba(255, 1%, 0%, 1)",
        "rgb(255, none, 0)",
        "rgb(1deg, 0, 0)",
    ] {
        assert_eq!(Color::parse(invalid), None, "{invalid}");
    }
    assert_eq!(Color::parse("rgb(255, 0, 0)"), rgb(255, 0, 0));
    assert_eq!(Color::parse("rgb(100%, 0%, 0%)"), rgb(255, 0, 0));
    // The modern syntax may mix them, and accepts `none`.
    assert_eq!(Color::parse("rgb(255 0% none)"), rgb(255, 0, 0));
}

#[test]
fn legacy_hsl_needs_a_hue_then_percentages() {
    for invalid in [
        "hsl(0, 255, 128)",
        "hsl(0%, 100%, 50%)",
        "hsl(0px, 100%, 50%)",
        "hsla(0, 100%, 50, 1)",
    ] {
        assert_eq!(Color::parse(invalid), None, "{invalid}");
    }
    assert_eq!(Color::parse("hsl(120, 100%, 25%)"), rgb(0, 128, 0));
    assert_eq!(Color::parse("hsl(120deg 100 25)"), rgb(0, 128, 0));
    assert_eq!(Color::parse("hwb(0, 0%, 0%)"), None);
}

#[test]
fn channels_accept_math_functions() {
    assert_eq!(
        Color::parse("rgb(calc(5% + 15%), calc(40% / 2), calc(5% * 4))"),
        rgb(51, 51, 51)
    );
    assert_eq!(Color::parse("rgb(calc(255), min(0, 9), 0)"), rgb(255, 0, 0));
    assert_eq!(Color::parse("rgb(calc(10px), 0, 0)"), None);
}

#[test]
fn predefined_color_spaces_convert_to_srgb() {
    assert_eq!(Color::parse("color(srgb 1 0 0)"), rgb(255, 0, 0));
    assert_eq!(Color::parse("color(display-p3 0 1 0)"), rgb(0, 255, 0));
    assert_eq!(
        Color::parse("color(xyz 0.9505 1 1.089)"),
        rgb(255, 255, 255)
    );
    // Round trips through the relative syntax keep the origin color.
    for space in [
        "srgb",
        "display-p3",
        "a98-rgb",
        "prophoto-rgb",
        "rec2020",
        "xyz-d50",
    ] {
        let keywords = if space.starts_with("xyz") {
            "x y z"
        } else {
            "r g b"
        };
        assert_eq!(
            Color::parse(&format!("color(from rgb(10 120 200) {space} {keywords})")),
            rgb(10, 120, 200),
            "{space}"
        );
    }
    assert_eq!(
        Color::parse("color(rec2020 0.3281959 0.543352 0.1865422)"),
        Color::parse("color(prophoto-rgb 0.23 0.43 0.13)")
    );
    assert_eq!(Color::parse("color(nonsense 1 0 0)"), None);
    assert_eq!(Color::parse("color(srgb 1 0)"), None);
}

#[test]
fn color_mix_in_srgb_mixes_encoded_values() {
    assert_eq!(
        Color::parse(
            "color-mix(in srgb, color-mix(in srgb, rgb(0 0 255) 50%, color(srgb 0 1 0)), white)"
        ),
        Color::parse("color(srgb 0.5 0.75 0.75)")
    );
}
