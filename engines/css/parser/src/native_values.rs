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
        "BackgroundRepeat" => return list(info, value, background_repeat),
        "BackgroundClip" => {
            // `text` is for backgrounds only; `border-area` is behind a preference that is off.
            let mut boxes = vec!["border-box", "padding-box", "content-box"];
            if info.parse_method == "parse_for_background" {
                boxes.push("text");
            }
            return list(info, value, |v| match single_token(v) {
                Some(cssparser::Token::Ident(i)) => Native::Parsed(
                    boxes
                        .iter()
                        .find(|b| i.eq_ignore_ascii_case(b))
                        .map(|b| (*b).to_owned()),
                ),
                _ => Native::Parsed(None),
            });
        }
        "BackgroundSize" => return list(info, value, background_size),
        "position::HorizontalPosition" => {
            return list(info, value, |v| position_component(v, ["left", "right"]));
        }
        "position::VerticalPosition" => {
            return list(info, value, |v| position_component(v, ["top", "bottom"]));
        }
        "AnimationIterationCount" => {
            return list(info, value, |v| match single_token(v) {
                Some(cssparser::Token::Ident(i)) if i.eq_ignore_ascii_case("infinite") => {
                    Native::Parsed(Some("infinite".into()))
                }
                _ => number("NonNegativeNumber", v),
            });
        }
        // `auto` needs the scroll-driven animations preference, which is off.
        "AnimationDuration" => return list(info, value, |v| time(v, true)),
        "ColumnCount" => {
            return match single_token(value) {
                Some(cssparser::Token::Ident(i)) if i.eq_ignore_ascii_case("auto") => {
                    Native::Parsed(Some("auto".into()))
                }
                Some(cssparser::Token::Number {
                    int_value: Some(n), ..
                }) => Native::Parsed((n >= 1).then(|| n.to_string())),
                Some(cssparser::Token::Function(_)) => Native::Unported,
                _ => Native::Parsed(None),
            };
        }
        "color::CaretColor" => {
            return match single_token(value) {
                Some(cssparser::Token::Ident(i)) if i.eq_ignore_ascii_case("auto") => {
                    Native::Parsed(Some("auto".into()))
                }
                _ => color(value),
            };
        }
        "BorderSpacing" => return pair(value, |v| length_percentage(v, &[], false, false)),
        "length::NonNegativeLengthOrAuto" => {
            return length_percentage(value, &["auto"], false, false);
        }
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
        // Only a dimension or a math function is a time.
        Some(Token::Function(_)) => Native::Unported,
        _ => Native::Parsed(None),
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
    pair(value, |v| length_percentage(v, &[], false, true))
}

/// One or two values (Stylo `Size2D`), the second written only when it differs.
fn pair(value: &str, item: impl Fn(&str) -> Native) -> Native {
    let mut parser = Parser::new(value);
    let mut parts = Vec::new();
    while !parser.is_exhausted() {
        let start = parser.position();
        if parser.next().is_err() {
            return Native::Unported;
        }
        parts.push(parser.slice_from(start).trim().to_owned());
    }
    let parsed: Vec<Native> = parts.iter().map(|p| item(p)).collect();
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

/// One position component (Stylo `PositionComponent`): `center`, a length-percentage, or a
/// side keyword with an optional length-percentage offset, serialized as written.
fn position_component(value: &str, sides: [&str; 2]) -> Native {
    let mut parser = Parser::new(value);
    let mut parts = Vec::new();
    while !parser.is_exhausted() {
        let start = parser.position();
        if parser.next().is_err() {
            return Native::Parsed(None);
        }
        parts.push(parser.slice_from(start).trim().to_owned());
    }
    let offset = |v: &str| length_percentage(v, &[], true, true);
    match parts.as_slice() {
        [one] if one.eq_ignore_ascii_case("center") => Native::Parsed(Some("center".into())),
        [one] => match sides.iter().find(|s| one.eq_ignore_ascii_case(s)) {
            Some(side) => Native::Parsed(Some((*side).to_owned())),
            // Any other identifier is no length either.
            None if matches!(single_token(one), Some(cssparser::Token::Ident(_))) => {
                Native::Parsed(None)
            }
            None => offset(one),
        },
        [side, length] => match sides.iter().find(|s| side.eq_ignore_ascii_case(s)) {
            Some(side) => match offset(length) {
                Native::Parsed(Some(l)) => Native::Parsed(Some(format!("{side} {l}"))),
                other => other,
            },
            None => match offset(side) {
                Native::Unported => Native::Unported,
                _ => Native::Parsed(None),
            },
        },
        _ => Native::Unported,
    }
}

/// One `background-size` layer: `cover`, `contain`, or a width and an optional height
/// (each `auto` or a non-negative length-percentage), serialized `W H` unless both are
/// `auto` (Stylo `GenericBackgroundSize`).
fn background_size(value: &str) -> Native {
    let mut parser = Parser::new(value);
    let mut parts = Vec::new();
    while !parser.is_exhausted() {
        let start = parser.position();
        if parser.next().is_err() {
            return Native::Parsed(None);
        }
        parts.push(parser.slice_from(start).trim().to_owned());
    }
    let size = |v: &str| length_percentage(v, &["auto"], false, true);
    let (width, height) = match parts.as_slice() {
        [one] if one.eq_ignore_ascii_case("cover") || one.eq_ignore_ascii_case("contain") => {
            return Native::Parsed(Some(one.to_ascii_lowercase()));
        }
        [w] => (size(w), Native::Parsed(Some("auto".into()))),
        [w, h] => (size(w), size(h)),
        _ => return Native::Unported,
    };
    match (width, height) {
        (Native::Parsed(Some(w)), Native::Parsed(Some(h))) => {
            Native::Parsed(Some(if w == "auto" && h == "auto" {
                "auto".into()
            } else {
                format!("{w} {h}")
            }))
        }
        (Native::Parsed(None), _) | (_, Native::Parsed(None)) => Native::Parsed(None),
        _ => Native::Unported,
    }
}

/// One `background-repeat` layer: `repeat-x`, `repeat-y`, or one or two of `repeat`,
/// `space`, `round`, `no-repeat`, serialized as Stylo does.
fn background_repeat(value: &str) -> Native {
    const KEYWORDS: [&str; 4] = ["repeat", "space", "round", "no-repeat"];
    let mut parser = Parser::new(value);
    let mut words = Vec::new();
    while !parser.is_exhausted() {
        match parser.expect_ident() {
            Ok(w) => words.push(w.to_ascii_lowercase()),
            Err(_) => return Native::Parsed(None),
        }
    }
    let keyword = |w: &String| KEYWORDS.contains(&w.as_str());
    let (h, v) = match words.as_slice() {
        [w] if w == "repeat-x" => ("repeat", "no-repeat"),
        [w] if w == "repeat-y" => ("no-repeat", "repeat"),
        [w] if keyword(w) => (w.as_str(), w.as_str()),
        [a, b] if keyword(a) && keyword(b) => (a.as_str(), b.as_str()),
        _ => return Native::Parsed(None),
    };
    Native::Parsed(Some(match (h, v) {
        ("repeat", "no-repeat") => "repeat-x".into(),
        ("no-repeat", "repeat") => "repeat-y".into(),
        (h, v) if h == v => h.to_owned(),
        (h, v) => format!("{h} {v}"),
    }))
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
        // Length-only types reject percentages.
        Token::Percentage { .. } => return Native::Parsed(None),
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
#[path = "native_values_tests.rs"]
mod tests;
