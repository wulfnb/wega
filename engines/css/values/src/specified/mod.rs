//! Specified values as the CSSOM serializes them: validation plus Stylo's specified-value
//! serialization (`stylo/values/specified/*`, MPL-2.0), one module per property family.
//!
//! Each parser returns [`Specified`]: a serialized value, invalid, or not ported yet (so a
//! caller can fall back to another engine for the grammar parts not covered here).
use cssparser::Parser;

mod align;
mod background;
mod box_values;
mod color;
mod font;
mod length;
mod number;

pub use align::{Axis, Grammar, alignment};
pub use background::{
    background_clip, background_repeat, background_size, image, position_component,
};
pub use box_values::display;
pub use color::{caret_color, color};
pub use font::{font_weight, line_height};
pub use length::{border_spacing, corner_radius, length_percentage};
pub use number::{animation_iteration_count, column_count, number, opacity, time};

/// The result of parsing one specified value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Specified {
    /// Parsed: the serialized specified value, or `None` if invalid.
    Parsed(Option<String>),
    /// Valid syntax this module does not handle yet (math functions and the like).
    Unported,
}

impl From<Option<String>> for Specified {
    fn from(value: Option<String>) -> Self {
        Specified::Parsed(value)
    }
}

/// A single token of `value`, or `None` if there are more (or none).
pub(crate) fn single_token(value: &str) -> Option<cssparser::Token<'_>> {
    let mut parser = Parser::new(value);
    let token = parser.next().ok()?.clone();
    parser.is_exhausted().then_some(token)
}

/// The top-level components of `value` (a function or block is one component), trimmed,
/// or `None` on a tokenizer error.
pub(crate) fn components(value: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new(value);
    let mut parts = Vec::new();
    while !parser.is_exhausted() {
        let start = parser.position();
        parser.next().ok()?;
        parts.push(parser.slice_from(start).trim().to_owned());
    }
    Some(parts)
}

/// The idents of `value`, lowercased, or `None` if it holds anything else.
pub(crate) fn idents(value: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new(value);
    let mut out = Vec::new();
    while !parser.is_exhausted() {
        out.push(parser.expect_ident().ok()?.to_ascii_lowercase());
    }
    Some(out)
}

/// A comma-separated list when `is_list` (each item parsed by `item`), else one item.
pub fn list(is_list: bool, value: &str, item: impl Fn(&str) -> Specified) -> Specified {
    if !is_list {
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
        return Specified::Parsed(None);
    };
    for part in parts {
        match item(&part) {
            Specified::Parsed(Some(v)) => items.push(v),
            other => return other,
        }
    }
    Specified::Parsed(Some(items.join(", ")))
}

/// One or two values (Stylo `Size2D`), the second written only when it differs.
pub fn pair(value: &str, item: impl Fn(&str) -> Specified) -> Specified {
    let Some(parts) = components(value) else {
        return Specified::Unported;
    };
    let parsed: Vec<Specified> = parts.iter().map(|p| item(p)).collect();
    match parsed.as_slice() {
        [Specified::Parsed(Some(a))] => Specified::Parsed(Some(a.clone())),
        [Specified::Parsed(Some(a)), Specified::Parsed(Some(b))] => {
            Specified::Parsed(Some(if a == b {
                a.clone()
            } else {
                format!("{a} {b}")
            }))
        }
        [_, _] | [_] if parsed.iter().any(|p| matches!(p, Specified::Parsed(None))) => {
            Specified::Parsed(None)
        }
        _ => Specified::Unported,
    }
}
