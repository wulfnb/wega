//! Numbers, integers, times and the properties built only from them.
//!
//! Ported from Stylo `values/specified/{number,time,effects}.rs` and the animation and
//! column types (MPL-2.0); math functions are not ported yet.
use super::{Specified, single_token};
use cssparser::{Parser, ToCss, Token};

/// `NonNegativeNumber`, `Integer` or `ZIndex` (`auto | <integer>`), by Stylo value type
/// name, serialized as Stylo does.
pub fn number(value_type: &str, value: &str) -> Specified {
    let mut parser = Parser::new(value);
    let Ok(token) = parser.next().cloned() else {
        return Specified::Unported;
    };
    if !parser.is_exhausted() {
        return Specified::Unported;
    }
    match (value_type, token) {
        ("ZIndex", Token::Ident(ident)) if ident.eq_ignore_ascii_case("auto") => {
            Specified::Parsed(Some("auto".into()))
        }
        ("NonNegativeNumber", Token::Number { value, .. }) if value.is_finite() => {
            Specified::Parsed((value >= 0.0).then(|| value.to_css_string()))
        }
        ("Integer" | "ZIndex", Token::Number { int_value, .. }) => {
            Specified::Parsed(int_value.map(|i| i.to_string()))
        }
        (_, Token::Function(_)) => Specified::Unported,
        _ => Specified::Parsed(None),
    }
}

/// `<time>` in `s` or `ms`, serialized in its unit (Stylo `NoCalcTime`).
pub fn time(value: &str, non_negative: bool) -> Specified {
    match single_token(value) {
        Some(Token::Dimension { value, unit, .. }) if value.is_finite() => {
            let unit = unit.to_ascii_lowercase();
            if unit != "s" && unit != "ms" {
                return Specified::Parsed(None);
            }
            if non_negative && value < 0.0 {
                return Specified::Parsed(None);
            }
            Specified::Parsed(Some(format!("{}{unit}", value.to_css_string())))
        }
        // Only a dimension or a math function is a time.
        Some(Token::Function(_)) => Specified::Unported,
        _ => Specified::Parsed(None),
    }
}

/// `opacity`: a number, or a percentage serialized as the equivalent number.
pub fn opacity(value: &str) -> Specified {
    match single_token(value) {
        Some(Token::Number { value, .. }) if value.is_finite() => {
            Specified::Parsed(Some(value.to_css_string()))
        }
        Some(Token::Percentage { unit_value, .. }) if unit_value.is_finite() => {
            Specified::Parsed(Some(unit_value.to_css_string()))
        }
        _ => Specified::Unported,
    }
}

/// One `animation-iteration-count`: `infinite` or a non-negative number.
pub fn animation_iteration_count(value: &str) -> Specified {
    match single_token(value) {
        Some(Token::Ident(i)) if i.eq_ignore_ascii_case("infinite") => {
            Specified::Parsed(Some("infinite".into()))
        }
        _ => number("NonNegativeNumber", value),
    }
}

/// `column-count`: `auto` or a positive integer.
pub fn column_count(value: &str) -> Specified {
    match single_token(value) {
        Some(Token::Ident(i)) if i.eq_ignore_ascii_case("auto") => {
            Specified::Parsed(Some("auto".into()))
        }
        Some(Token::Number {
            int_value: Some(n), ..
        }) => Specified::Parsed((n >= 1).then(|| n.to_string())),
        Some(Token::Function(_)) => Specified::Unported,
        _ => Specified::Parsed(None),
    }
}

/// `tab-size`: a non-negative number (tried first, so `0` stays a number) or a non-negative
/// length (Stylo `NonNegativeLengthOrNumber`).
pub fn length_or_number(value: &str) -> Specified {
    match single_token(value) {
        Some(Token::Number { .. }) => number("NonNegativeNumber", value),
        _ => super::length_percentage(value, &[], false, false),
    }
}

/// `fill-opacity`, `stroke-opacity`: an opacity, `context-fill-opacity` or
/// `context-stroke-opacity` (Stylo `SVGOpacity`).
pub fn svg_opacity(value: &str) -> Specified {
    match single_token(value) {
        Some(Token::Ident(i)) => Specified::Parsed(
            ["context-fill-opacity", "context-stroke-opacity"]
                .iter()
                .find(|k| i.eq_ignore_ascii_case(k))
                .map(|k| (*k).to_owned()),
        ),
        _ => opacity(value),
    }
}
