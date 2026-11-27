//! SVG geometry values: `stroke-width`, `stroke-dashoffset`, `stroke-dasharray`.
//!
//! Ported from Stylo `values/specified/svg.rs` (MPL-2.0). These accept unitless numbers as
//! pixels in every mode (`AllowQuirks::Always`); `context-value` is behind a preference
//! that is off.
use super::{Specified, length_percentage, single_token};
use cssparser::{Parser, ToCss, Token};

/// One SVG length: a length-percentage, or a unitless number serialized as pixels.
pub fn svg_length(value: &str, negative: bool) -> Specified {
    match single_token(value) {
        Some(Token::Number { value, .. }) if value.is_finite() && value != 0.0 => {
            Specified::Parsed(
                (negative || value >= 0.0).then(|| format!("{}px", value.to_css_string())),
            )
        }
        _ => length_percentage(value, &[], negative, true),
    }
}

/// `stroke-dasharray`: `none`, or non-negative SVG lengths separated by commas or spaces,
/// written comma-separated.
pub fn dash_array(value: &str) -> Specified {
    if matches!(single_token(value), Some(Token::Ident(i)) if i.eq_ignore_ascii_case("none")) {
        return Specified::Parsed(Some("none".into()));
    }
    let mut parser = Parser::new(value);
    let mut items = Vec::new();
    let mut expect_item = true;
    while !parser.is_exhausted() {
        let start = parser.position();
        let Ok(token) = parser.next().cloned() else {
            return Specified::Parsed(None);
        };
        if token == Token::Comma {
            if expect_item {
                return Specified::Parsed(None);
            }
            expect_item = true;
            continue;
        }
        match svg_length(parser.slice_from(start).trim(), false) {
            Specified::Parsed(Some(item)) => items.push(item),
            other => return other,
        }
        expect_item = false;
    }
    if items.is_empty() || expect_item {
        return Specified::Parsed(None);
    }
    Specified::Parsed(Some(items.join(", ")))
}

/// `fill`, `stroke`: `none`, a color, or a `url()`, `context-fill` or `context-stroke` with an
/// optional `none` or color fallback (Stylo `SVGPaint`).
pub fn svg_paint(value: &str) -> Specified {
    let Some(parts) = super::components(value) else {
        return Specified::Parsed(None);
    };
    let ident = |part: &str, name: &str| matches!(single_token(part), Some(Token::Ident(i)) if i.eq_ignore_ascii_case(name));
    let is_url = |part: &str| match single_token(part) {
        Some(Token::UnquotedUrl(_)) => true,
        Some(Token::Function(f)) => f.eq_ignore_ascii_case("url"),
        _ => false,
    };
    let fallback = |part: &str| -> Specified {
        if ident(part, "none") {
            Specified::Parsed(Some("none".into()))
        } else {
            super::color(part)
        }
    };
    let (kind, rest) = match parts.as_slice() {
        [] => return Specified::Parsed(None),
        [first, rest @ ..] if ident(first, "none") => ("none".to_owned(), (rest, false)),
        [first, rest @ ..] if ident(first, "context-fill") => ("context-fill".into(), (rest, true)),
        [first, rest @ ..] if ident(first, "context-stroke") => {
            ("context-stroke".into(), (rest, true))
        }
        [first, rest @ ..] if is_url(first) => match super::image(first) {
            Specified::Parsed(Some(url)) => (url, (rest, true)),
            other => return other,
        },
        [first, rest @ ..] => match super::color(first) {
            Specified::Parsed(Some(color)) => (color, (rest, false)),
            other => return other,
        },
    };
    match rest {
        ([], _) => Specified::Parsed(Some(kind)),
        ([one], true) => match fallback(one) {
            Specified::Parsed(Some(f)) => Specified::Parsed(Some(format!("{kind} {f}"))),
            other => other,
        },
        _ => Specified::Parsed(None),
    }
}
