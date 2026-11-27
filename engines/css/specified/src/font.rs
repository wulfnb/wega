//! Font and text metrics values.
//!
//! Ported from Stylo `values/specified/{font,text}.rs` (MPL-2.0).
use super::{Specified, length_percentage, single_token};
use cssparser::{ToCss, Token};

/// `line-height`: `normal`, a non-negative number (kept unitless, so `0` stays `0`), or a
/// non-negative length-percentage.
pub fn line_height(value: &str) -> Specified {
    match single_token(value) {
        Some(Token::Number { value, .. }) if value.is_finite() => {
            Specified::Parsed((value >= 0.0).then(|| value.to_css_string()))
        }
        _ => length_percentage(value, &["normal"], false, true),
    }
}

/// `font-weight`: a number from 1 to 1000, or `normal`, `bold`, `bolder`, `lighter`.
pub fn font_weight(value: &str) -> Specified {
    match single_token(value) {
        Some(Token::Number { value, .. }) if value.is_finite() => Specified::Parsed(
            (1.0..=1000.0)
                .contains(&value)
                .then(|| value.to_css_string()),
        ),
        Some(Token::Ident(ident)) => match ["normal", "bold", "bolder", "lighter"]
            .iter()
            .find(|k| ident.eq_ignore_ascii_case(k))
        {
            Some(k) => Specified::Parsed(Some((*k).to_owned())),
            None => Specified::Unported,
        },
        _ => Specified::Unported,
    }
}
