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
mod easing;
mod flags;
mod font;
mod grid;
mod length;
mod names;
mod number;
mod position;
mod svg;
mod text;

pub use align::{Axis, Grammar, alignment};
pub use background::{
    background_clip, background_repeat, background_size, image, position_component,
};
pub use box_values::{aspect_ratio, display, flex_wrap};
pub use color::{caret_color, color, color_scheme};
pub use easing::timing_function;
pub use flags::{
    CONTAIN, CONTAINER_TYPE, Flags, TEXT_DECORATION_LINE, TEXT_TRANSFORM, TEXT_UNDERLINE_POSITION,
    TOUCH_ACTION, flags,
};
pub use font::{font_weight, line_height};
pub use grid::{grid_auto_flow, grid_line};
pub use length::{border_spacing, corner_radius, length_percentage};
pub use names::{animation_name, container_name, will_change};
pub use number::{
    animation_iteration_count, column_count, length_or_number, number, opacity, svg_opacity, time,
};
pub use position::position;
pub use svg::{dash_array, svg_length, svg_paint};
pub use text::{text_align, text_overflow};

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
        let block = matches!(
            parser.next().ok()?,
            cssparser::Token::Function(_)
                | cssparser::Token::ParenthesisBlock
                | cssparser::Token::SquareBracketBlock
                | cssparser::Token::CurlyBracketBlock
        );
        if block {
            // Consume the whole block so the part is the complete function.
            parser
                .parse_nested_block(|p| {
                    while p.next().is_ok() {}
                    Ok::<_, cssparser::ParseError<()>>(())
                })
                .ok()?;
        }
        parts.push(parser.slice_from(start).trim().to_owned());
    }
    Some(parts)
}

/// The idents of `value` as written, or `None` if it holds anything else.
pub(crate) fn idents_raw(value: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new(value);
    let mut out = Vec::new();
    while !parser.is_exhausted() {
        out.push(parser.expect_ident().ok()?.to_string());
    }
    Some(out)
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

/// A `<custom-ident>` serialized as an identifier, or `None` for a CSS-wide keyword, `default`,
/// or one of `excluding` (any ASCII case), as Stylo's `CustomIdent::from_ident`.
pub fn custom_ident(ident: &str, excluding: &[&str]) -> Option<String> {
    const RESERVED: &[&str] = &[
        "initial",
        "inherit",
        "unset",
        "revert",
        "revert-layer",
        "default",
    ];
    if RESERVED
        .iter()
        .chain(excluding)
        .any(|k| ident.eq_ignore_ascii_case(k))
    {
        return None;
    }
    let mut out = String::new();
    cssparser::serialize_identifier(ident, &mut out).ok()?;
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
