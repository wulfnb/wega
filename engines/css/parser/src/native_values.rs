//! Native property values for CSSOM declaration blocks, with a fallback [`ValueEngine`] for
//! value types not ported yet. Each ported type validates and serializes specified values
//! as Stylo does (`stylo/values/specified/*`), so moving a type here changes no output.
use crate::generated::properties::KEYWORD_TYPES;
use crate::properties_table::longhand;
use crate::{ValueEngine, properties_table::Longhand};
use cssparser::Parser;

/// Native values first, `fallback` for the rest.
pub struct NativeFirst<F: ValueEngine>(pub F);

/// The result of parsing one longhand natively.
enum Native {
    /// Parsed: the serialized specified value, or `None` if invalid.
    Parsed(Option<String>),
    /// Not ported: ask the fallback engine.
    Unported,
}

impl<F: ValueEngine> ValueEngine for NativeFirst<F> {
    fn parse(&self, property: &str, value: &str) -> Option<Vec<(String, String)>> {
        if let Some(info) = longhand(property) {
            match parse_longhand(info, value) {
                Native::Parsed(parsed) => {
                    return parsed.map(|value| vec![(property.to_owned(), value)]);
                }
                Native::Unported => {}
            }
        }
        self.0.parse(property, value)
    }

    fn serialize_shorthand(&self, shorthand: &str, longhands: &[&str]) -> Option<String> {
        self.0.serialize_shorthand(shorthand, longhands)
    }
}

fn parse_longhand(info: &Longhand, value: &str) -> Native {
    if !info.keywords.is_empty() {
        return Native::Parsed(keywords(info, value));
    }
    let base = info.value_type.rsplit("::").next().unwrap_or_default();
    if let Ok(i) = KEYWORD_TYPES.binary_search_by(|(name, _)| (*name).cmp(base)) {
        return Native::Parsed(keyword_enum(info, KEYWORD_TYPES[i].1, value));
    }
    // `(keywords handled here, negative values allowed, percentages allowed)`.
    let (words, negative, percent): (&[&str], bool, bool) = match info.value_type {
        "Margin" | "Inset" => (&["auto"], true, true),
        "Size" => (&["auto"], false, true),
        "MaxSize" => (&["none"], false, true),
        "NonNegativeLengthPercentage" => (&[], false, true),
        "NonNegativeLengthPercentageOrAuto" => (&["auto"], false, true),
        "length::NonNegativeLengthPercentageOrNormal" => (&["normal"], false, true),
        "LengthPercentage" => (&[], true, true),
        "BorderSideWidth" => (&["thin", "medium", "thick"], false, false),
        "Color" | "ColorPropertyValue" => return color(value),
        "NonNegativeNumber" | "Integer" | "ZIndex" => return number(info.value_type, value),
        "Time" => return list(info, value, |v| time(v, info.non_negative)),
        "Opacity" => return opacity(value),
        "LineHeight" => return line_height(value),
        "FontWeight" => return font_weight(value),
        "BorderCornerRadius" => return corner_radius(value),
        "Display" => return Native::Parsed(crate::native_align::display(value)),
        "ContentDistribution" | "SelfAlignment" | "ItemPlacement" | "JustifyItems" => {
            use crate::native_align::{Axis, Grammar, alignment};
            let grammar = match info.value_type {
                "ContentDistribution" => Grammar::ContentDistribution,
                "SelfAlignment" => Grammar::SelfAlignment,
                _ => Grammar::ItemPlacement,
            };
            let axis = if info.parse_method == "parse_block" {
                Axis::Block
            } else {
                Axis::Inline
            };
            return Native::Parsed(alignment(value, grammar, axis));
        }
        _ => return Native::Unported,
    };
    length_percentage(value, words, negative, percent)
}

/// A keyword enum value type (generated from Stylo's `#[derive(Parse)]` enums): one of its
/// keywords or their aliases, serialized as the keyword; a comma-separated list for list
/// properties.
fn keyword_enum(
    info: &Longhand,
    variants: &[(&'static str, &'static [&'static str])],
    value: &str,
) -> Option<String> {
    let one = |p: &mut Parser<'_>| -> Result<String, cssparser::ParseError<()>> {
        let ident = p.expect_ident()?.clone();
        variants
            .iter()
            .find(|(keyword, aliases)| {
                ident.eq_ignore_ascii_case(keyword)
                    || aliases.iter().any(|a| ident.eq_ignore_ascii_case(a))
            })
            .map(|(keyword, _)| (*keyword).to_owned())
            .ok_or_else(cssparser::ParseError::unexpected_token)
    };
    let mut parser = Parser::new(value);
    let values = parser
        .parse_entirely(|p| {
            if info.list {
                p.parse_comma_separated(one)
            } else {
                one(p).map(|k| vec![k])
            }
        })
        .ok()?;
    Some(values.join(", "))
}

/// A color keyword or hex color, serialized as Stylo serializes specified colors: keywords
/// as written (lowercase), hex colors in the legacy `rgb()`/`rgba()` form with cssparser's
/// channel rounding and alpha serialization. Color functions, system colors and
/// `color-mix()` are left to the fallback.
fn color(value: &str) -> Native {
    use cssparser::{
        Token,
        color::{parse_hash_color, parse_named_color, serialize_color_alpha},
    };
    let mut parser = Parser::new(value);
    let Ok(token) = parser.next().cloned() else {
        return Native::Unported;
    };
    if !parser.is_exhausted() {
        return Native::Unported;
    }
    match token {
        Token::Ident(ident) => {
            let lower = ident.to_ascii_lowercase();
            if lower == "transparent"
                || lower == "currentcolor"
                || parse_named_color(&lower).is_ok()
            {
                Native::Parsed(Some(lower))
            } else {
                Native::Unported
            }
        }
        Token::Hash(hex) | Token::IDHash(hex) => {
            let Ok((r, g, b, alpha)) = parse_hash_color(hex.as_bytes()) else {
                return Native::Parsed(None);
            };
            let mut out = String::from(if alpha == 1.0 { "rgb(" } else { "rgba(" });
            out.push_str(&format!("{r}, {g}, {b}"));
            if serialize_color_alpha(&mut out, Some(alpha), true).is_err() {
                return Native::Unported;
            }
            out.push(')');
            Native::Parsed(Some(out))
        }
        _ => Native::Unported,
    }
}

/// `<number [0,∞]>`, `<integer>` or `auto | <integer>` (`z-index`), serialized as Stylo
/// does. Math functions are left to the fallback.
fn number(value_type: &str, value: &str) -> Native {
    use cssparser::{ToCss, Token};
    let mut parser = Parser::new(value);
    let Ok(token) = parser.next().cloned() else {
        return Native::Unported;
    };
    if !parser.is_exhausted() {
        return Native::Unported;
    }
    match (value_type, token) {
        ("ZIndex", Token::Ident(ident)) if ident.eq_ignore_ascii_case("auto") => {
            Native::Parsed(Some("auto".into()))
        }
        ("NonNegativeNumber", Token::Number { value, .. }) if value.is_finite() => {
            Native::Parsed((value >= 0.0).then(|| value.to_css_string()))
        }
        ("Integer" | "ZIndex", Token::Number { int_value, .. }) => {
            Native::Parsed(int_value.map(|i| i.to_string()))
        }
        (_, Token::Function(_)) => Native::Unported,
        _ => Native::Parsed(None),
    }
}

/// A single token of `value`, or `None` if there are more (or none).
fn single_token(value: &str) -> Option<cssparser::Token<'_>> {
    let mut parser = Parser::new(value);
    let token = parser.next().ok()?.clone();
    parser.is_exhausted().then_some(token)
}

/// A comma-separated list for list properties (each item parsed by `item`), else one item.
fn list(info: &Longhand, value: &str, item: impl Fn(&str) -> Native) -> Native {
    if !info.list {
        return item(value);
    }
    let mut items = Vec::new();
    let mut parser = Parser::new(value);
    let parsed: Result<Vec<String>, cssparser::ParseError<()>> =
        parser.parse_comma_separated(|p| {
            let start = p.position();
            while p.next().is_ok() {}
            Ok(p.slice_from(start).trim().to_owned())
        });
    let Ok(parts) = parsed else {
        return Native::Parsed(None);
    };
    for part in parts {
        match item(&part) {
            Native::Parsed(Some(v)) => items.push(v),
            other => return other,
        }
    }
    Native::Parsed(Some(items.join(", ")))
}

/// `<time>` in `s` or `ms`, serialized in its unit (Stylo `NoCalcTime`).
fn time(value: &str, non_negative: bool) -> Native {
    use cssparser::{ToCss, Token};
    match single_token(value) {
        Some(Token::Dimension { value, unit, .. }) if value.is_finite() => {
            let unit = unit.to_ascii_lowercase();
            if unit != "s" && unit != "ms" {
                return Native::Parsed(None);
            }
            if non_negative && value < 0.0 {
                return Native::Parsed(None);
            }
            Native::Parsed(Some(format!("{}{unit}", value.to_css_string())))
        }
        _ => Native::Unported,
    }
}

/// `opacity`: a number, or a percentage serialized as the equivalent number.
fn opacity(value: &str) -> Native {
    use cssparser::{ToCss, Token};
    match single_token(value) {
        Some(Token::Number { value, .. }) if value.is_finite() => {
            Native::Parsed(Some(value.to_css_string()))
        }
        Some(Token::Percentage { unit_value, .. }) if unit_value.is_finite() => {
            Native::Parsed(Some(unit_value.to_css_string()))
        }
        _ => Native::Unported,
    }
}

/// `line-height`: `normal`, a non-negative number (kept unitless, so `0` stays `0`), or a
/// non-negative length-percentage.
fn line_height(value: &str) -> Native {
    use cssparser::{ToCss, Token};
    match single_token(value) {
        Some(Token::Number { value, .. }) if value.is_finite() => {
            Native::Parsed((value >= 0.0).then(|| value.to_css_string()))
        }
        _ => length_percentage(value, &["normal"], false, true),
    }
}

/// `font-weight`: a number from 1 to 1000, or `normal`, `bold`, `bolder`, `lighter`.
fn font_weight(value: &str) -> Native {
    use cssparser::{ToCss, Token};
    match single_token(value) {
        Some(Token::Number { value, .. }) if value.is_finite() => Native::Parsed(
            (1.0..=1000.0)
                .contains(&value)
                .then(|| value.to_css_string()),
        ),
        Some(Token::Ident(ident)) => match ["normal", "bold", "bolder", "lighter"]
            .iter()
            .find(|k| ident.eq_ignore_ascii_case(k))
        {
            Some(k) => Native::Parsed(Some((*k).to_owned())),
            None => Native::Unported,
        },
        _ => Native::Unported,
    }
}

/// A corner radius: one or two non-negative length-percentages; the second is written only
/// when it differs (Stylo `Size2D`).
fn corner_radius(value: &str) -> Native {
    let mut parser = Parser::new(value);
    let mut parts = Vec::new();
    while !parser.is_exhausted() {
        let start = parser.position();
        if parser.next().is_err() {
            return Native::Unported;
        }
        parts.push(parser.slice_from(start).trim().to_owned());
    }
    let parsed: Vec<Native> = parts
        .iter()
        .map(|p| length_percentage(p, &[], false, true))
        .collect();
    match parsed.as_slice() {
        [Native::Parsed(Some(a))] => Native::Parsed(Some(a.clone())),
        [Native::Parsed(Some(a)), Native::Parsed(Some(b))] => Native::Parsed(Some(if a == b {
            a.clone()
        } else {
            format!("{a} {b}")
        })),
        [_, _] | [_] if parsed.iter().any(|p| matches!(p, Native::Parsed(None))) => {
            Native::Parsed(None)
        }
        _ => Native::Unported,
    }
}

/// Length units Stylo's `NoCalcLength` accepts, as it serializes them.
const LENGTH_UNITS: &[&str] = &[
    "px", "in", "cm", "mm", "q", "pt", "pc", "em", "ex", "ch", "cap", "ic", "rem", "rex", "rch",
    "rcap", "ric", "lh", "rlh", "vw", "vh", "vmin", "vmax", "vb", "vi", "svw", "svh", "svmin",
    "svmax", "svb", "svi", "lvw", "lvh", "lvmin", "lvmax", "lvb", "lvi", "dvw", "dvh", "dvmin",
    "dvmax", "dvb", "dvi", "cqw", "cqh", "cqi", "cqb", "cqmin", "cqmax",
];

/// One `<length-percentage>` or keyword, serialized as Stylo serializes specified values:
/// the number in its shortest form, then the unit in lowercase; a unitless zero is `0px`.
/// Anything else (math functions, other keywords, unknown units) is left to the fallback.
fn length_percentage(value: &str, words: &[&str], negative: bool, percent: bool) -> Native {
    use cssparser::{ToCss, Token};
    let mut parser = Parser::new(value);
    let Ok(token) = parser.next().cloned() else {
        return Native::Unported;
    };
    if !parser.is_exhausted() {
        return Native::Unported;
    }
    let number = |v: f32| v.to_css_string();
    let (value, serialized) = match token {
        Token::Ident(ident) => {
            return match words.iter().find(|w| ident.eq_ignore_ascii_case(w)) {
                Some(word) => Native::Parsed(Some((*word).to_owned())),
                None => Native::Unported,
            };
        }
        Token::Number { value, .. } if value == 0.0 => (value, "0px".to_owned()),
        Token::Dimension { value, unit, .. } => {
            let unit = unit.to_ascii_lowercase();
            if !LENGTH_UNITS.contains(&unit.as_str()) {
                return Native::Unported;
            }
            (value, format!("{}{unit}", number(value)))
        }
        Token::Percentage { unit_value, .. } if percent => {
            (unit_value, format!("{}%", number(unit_value * 100.0)))
        }
        _ => return Native::Unported,
    };
    if !value.is_finite() {
        return Native::Unported;
    }
    if value < 0.0 && !negative {
        return Native::Parsed(None);
    }
    Native::Parsed(Some(serialized))
}

/// A keyword property (Stylo `single_keyword`): one of its keywords, ASCII
/// case-insensitively, serialized in lowercase; a comma-separated list for list properties.
fn keywords(info: &Longhand, value: &str) -> Option<String> {
    let one = |p: &mut Parser<'_>| -> Result<String, cssparser::ParseError<()>> {
        let ident = p.expect_ident()?.clone();
        info.keywords
            .iter()
            .find(|k| ident.eq_ignore_ascii_case(k))
            .map(|k| (*k).to_owned())
            .ok_or_else(cssparser::ParseError::unexpected_token)
    };
    let mut parser = Parser::new(value);
    let values = parser
        .parse_entirely(|p| {
            if info.list {
                p.parse_comma_separated(one)
            } else {
                one(p).map(|k| vec![k])
            }
        })
        .ok()?;
    Some(values.join(", "))
}

#[cfg(test)]
mod tests {
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
        // Left to the fallback: math functions, other keywords, unknown units.
        for (p, v) in [
            ("width", "calc(1px + 1%)"),
            ("width", "fit-content"),
            ("width", "1foo"),
            ("border-top-width", "10%"),
        ] {
            assert_eq!(one(p, v).unwrap().0, "fallback", "{p}: {v}");
        }
    }
}
