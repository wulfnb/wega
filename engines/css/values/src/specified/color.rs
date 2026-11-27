//! Color property values.
//!
//! Ported from Stylo `values/specified/color.rs` (MPL-2.0): keywords and hex colors, with
//! cssparser's channel rounding and alpha serialization.
use super::{Specified, single_token};
use cssparser::{
    Parser, Token,
    color::{parse_hash_color, parse_named_color, serialize_color_alpha},
};

/// A color keyword or hex color, serialized as Stylo serializes specified colors: keywords
/// as written (lowercase), hex colors in the legacy `rgb()`/`rgba()` form. Color functions,
/// system colors and `color-mix()` are [`Specified::Unported`].
pub fn color(value: &str) -> Specified {
    let mut parser = Parser::new(value);
    let Ok(token) = parser.next().cloned() else {
        return Specified::Unported;
    };
    if !parser.is_exhausted() {
        return Specified::Unported;
    }
    match token {
        Token::Ident(ident) => {
            let lower = ident.to_ascii_lowercase();
            if lower == "transparent"
                || lower == "currentcolor"
                || parse_named_color(&lower).is_ok()
            {
                Specified::Parsed(Some(lower))
            } else {
                Specified::Unported
            }
        }
        Token::Hash(hex) | Token::IDHash(hex) => {
            let Ok((r, g, b, alpha)) = parse_hash_color(hex.as_bytes()) else {
                return Specified::Parsed(None);
            };
            let mut out = String::from(if alpha == 1.0 { "rgb(" } else { "rgba(" });
            out.push_str(&format!("{r}, {g}, {b}"));
            if serialize_color_alpha(&mut out, Some(alpha), true).is_err() {
                return Specified::Unported;
            }
            out.push(')');
            Specified::Parsed(Some(out))
        }
        _ => Specified::Unported,
    }
}

/// `caret-color`: `auto` or a color (Stylo `CaretColor`).
pub fn caret_color(value: &str) -> Specified {
    match single_token(value) {
        Some(Token::Ident(i)) if i.eq_ignore_ascii_case("auto") => {
            Specified::Parsed(Some("auto".into()))
        }
        _ => color(value),
    }
}

/// `color-scheme`: `normal`, or one or more names with `only` at the start or the end,
/// serialized as the names then `only` (Stylo `ColorScheme`).
pub fn color_scheme(value: &str) -> Specified {
    let Some(words) = super::idents_raw(value) else {
        return Specified::Parsed(None);
    };
    if let [one] = words.as_slice()
        && one.eq_ignore_ascii_case("normal")
    {
        return Specified::Parsed(Some("normal".into()));
    }
    let (mut names, mut only) = (Vec::new(), false);
    for (i, word) in words.iter().enumerate() {
        if word.eq_ignore_ascii_case("normal") {
            return Specified::Parsed(None);
        }
        if word.eq_ignore_ascii_case("only") {
            // Only at the start, or as the last word.
            if only || (!names.is_empty() && i + 1 != words.len()) {
                return Specified::Parsed(None);
            }
            only = true;
            continue;
        }
        match super::custom_ident(word, &[]) {
            Some(name) => names.push(name),
            None => return Specified::Parsed(None),
        }
    }
    if names.is_empty() {
        return Specified::Parsed(None);
    }
    let mut out = names.join(" ");
    if only {
        out.push_str(" only");
    }
    Specified::Parsed(Some(out))
}
