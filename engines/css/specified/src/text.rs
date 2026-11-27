//! Text values: alignment and overflow.
//!
//! Ported from Stylo `values/specified/text.rs` (MPL-2.0), Servo build.
use super::{Specified, components, single_token};
use cssparser::Token;

/// `text-align`: a `TextAlignKeyword` or `match-parent`; the `-webkit-` aliases serialize
/// as their `-moz-` names, as in Stylo.
pub fn text_align(value: &str) -> Specified {
    const KEYWORDS: &[(&str, &str)] = &[
        ("start", "start"),
        ("left", "left"),
        ("right", "right"),
        ("center", "center"),
        ("justify", "justify"),
        ("end", "end"),
        ("-moz-center", "-moz-center"),
        ("-webkit-center", "-moz-center"),
        ("-moz-left", "-moz-left"),
        ("-webkit-left", "-moz-left"),
        ("-moz-right", "-moz-right"),
        ("-webkit-right", "-moz-right"),
        ("match-parent", "match-parent"),
    ];
    match single_token(value) {
        Some(Token::Ident(i)) => Specified::Parsed(
            KEYWORDS
                .iter()
                .find(|(k, _)| i.eq_ignore_ascii_case(k))
                .map(|(_, out)| (*out).to_owned()),
        ),
        _ => Specified::Parsed(None),
    }
}

/// `text-overflow`: one or two of `clip`, `ellipsis` or a string. One side is written alone;
/// two are written as given (Stylo `TextOverflow`).
pub fn text_overflow(value: &str) -> Specified {
    let side = |part: &str| -> Option<String> {
        match single_token(part)? {
            Token::Ident(i) if i.eq_ignore_ascii_case("clip") => Some("clip".into()),
            Token::Ident(i) if i.eq_ignore_ascii_case("ellipsis") => Some("ellipsis".into()),
            Token::QuotedString(s) => {
                let mut out = String::new();
                cssparser::serialize_string(&s, &mut out).ok()?;
                Some(out)
            }
            _ => None,
        }
    };
    let Some(parts) = components(value) else {
        return Specified::Parsed(None);
    };
    Specified::Parsed(match parts.as_slice() {
        [one] => side(one),
        [first, second] => side(first)
            .zip(side(second))
            .map(|(a, b)| format!("{a} {b}")),
        _ => None,
    })
}
