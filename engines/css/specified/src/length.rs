//! `<length-percentage>` and the two-value length types built from it.
//!
//! Ported from Stylo `values/specified/length.rs` (MPL-2.0): `NoCalcLength` units and
//! serialization; math functions are not ported yet.
use super::{Specified, pair};
use cssparser::{Parser, ToCss, Token};

/// Length units Stylo's `NoCalcLength` accepts, as it serializes them.
const LENGTH_UNITS: &[&str] = &[
    "px", "in", "cm", "mm", "q", "pt", "pc", "em", "ex", "ch", "cap", "ic", "rem", "rex", "rch",
    "rcap", "ric", "lh", "rlh", "vw", "vh", "vmin", "vmax", "vb", "vi", "svw", "svh", "svmin",
    "svmax", "svb", "svi", "lvw", "lvh", "lvmin", "lvmax", "lvb", "lvi", "dvw", "dvh", "dvmin",
    "dvmax", "dvb", "dvi", "cqw", "cqh", "cqi", "cqb", "cqmin", "cqmax",
];

/// One `<length-percentage>` or one of `words`, serialized as Stylo serializes specified
/// values: the number in its shortest form, then the unit in lowercase; a unitless zero is
/// `0px`. `negative` allows negative values and `percent` percentages. Anything else (math
/// functions, other keywords, unknown units) is [`Specified::Unported`].
pub fn length_percentage(value: &str, words: &[&str], negative: bool, percent: bool) -> Specified {
    let mut parser = Parser::new(value);
    let Ok(token) = parser.next().cloned() else {
        return Specified::Unported;
    };
    if !parser.is_exhausted() {
        return Specified::Unported;
    }
    let number = |v: f32| v.to_css_string();
    let (value, serialized) = match token {
        Token::Ident(ident) => {
            return match words.iter().find(|w| ident.eq_ignore_ascii_case(w)) {
                Some(word) => Specified::Parsed(Some((*word).to_owned())),
                None => Specified::Unported,
            };
        }
        Token::Number { value, .. } if value == 0.0 => (value, "0px".to_owned()),
        Token::Dimension { value, unit, .. } => {
            let unit = unit.to_ascii_lowercase();
            if !LENGTH_UNITS.contains(&unit.as_str()) {
                return Specified::Unported;
            }
            (value, format!("{}{unit}", number(value)))
        }
        Token::Percentage { unit_value, .. } if percent => {
            (unit_value, format!("{}%", number(unit_value * 100.0)))
        }
        // Length-only types reject percentages.
        Token::Percentage { .. } => return Specified::Parsed(None),
        _ => return Specified::Unported,
    };
    if !value.is_finite() {
        return Specified::Unported;
    }
    if value < 0.0 && !negative {
        return Specified::Parsed(None);
    }
    Specified::Parsed(Some(serialized))
}

/// A corner radius: one or two non-negative length-percentages; the second is written only
/// when it differs (Stylo `BorderCornerRadius`, a `Size2D`).
pub fn corner_radius(value: &str) -> Specified {
    pair(value, |v| length_percentage(v, &[], false, true))
}

/// `border-spacing`: one or two non-negative lengths (Stylo `BorderSpacing`).
pub fn border_spacing(value: &str) -> Specified {
    pair(value, |v| length_percentage(v, &[], false, false))
}
