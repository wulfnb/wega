//! Native property values for CSSOM declaration blocks, with a fallback [`ValueEngine`] for
//! value types not ported yet. Each ported type validates and serializes specified values
//! as Stylo does (`stylo/values/specified/*`), so moving a type here changes no output.
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
        _ => return Native::Unported,
    };
    length_percentage(value, words, negative, percent)
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
