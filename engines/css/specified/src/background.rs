//! Background and mask layer values: repeat, clip, size, position components and images.
//!
//! Ported from Stylo `values/specified/{background,position,image,url}.rs` (MPL-2.0).
use super::{Specified, components, length_percentage, single_token};
use cssparser::{Parser, Token};

/// One `background-clip` (or `mask-clip`) layer. `text` is for backgrounds only
/// (`for_background`); `border-area` is behind a preference that is off.
pub fn background_clip(value: &str, for_background: bool) -> Specified {
    let mut boxes = vec!["border-box", "padding-box", "content-box"];
    if for_background {
        boxes.push("text");
    }
    match single_token(value) {
        Some(Token::Ident(i)) => Specified::Parsed(
            boxes
                .iter()
                .find(|b| i.eq_ignore_ascii_case(b))
                .map(|b| (*b).to_owned()),
        ),
        _ => Specified::Parsed(None),
    }
}

/// One position component (Stylo `PositionComponent`): `center`, a length-percentage, or a
/// side keyword with an optional length-percentage offset, serialized as written.
pub fn position_component(value: &str, sides: [&str; 2]) -> Specified {
    let Some(parts) = components(value) else {
        return Specified::Parsed(None);
    };
    let offset = |v: &str| length_percentage(v, &[], true, true);
    match parts.as_slice() {
        [one] if one.eq_ignore_ascii_case("center") => Specified::Parsed(Some("center".into())),
        [one] => match sides.iter().find(|s| one.eq_ignore_ascii_case(s)) {
            Some(side) => Specified::Parsed(Some((*side).to_owned())),
            // Any other identifier is no length either.
            None if matches!(single_token(one), Some(Token::Ident(_))) => Specified::Parsed(None),
            None => offset(one),
        },
        [side, length] => match sides.iter().find(|s| side.eq_ignore_ascii_case(s)) {
            Some(side) => match offset(length) {
                Specified::Parsed(Some(l)) => Specified::Parsed(Some(format!("{side} {l}"))),
                other => other,
            },
            None => match offset(side) {
                Specified::Unported => Specified::Unported,
                _ => Specified::Parsed(None),
            },
        },
        _ => Specified::Unported,
    }
}

/// One `background-size` layer: `cover`, `contain`, or a width and an optional height
/// (each `auto` or a non-negative length-percentage), serialized `W H` unless both are
/// `auto` (Stylo `GenericBackgroundSize`).
pub fn background_size(value: &str) -> Specified {
    let Some(parts) = components(value) else {
        return Specified::Parsed(None);
    };
    let size = |v: &str| length_percentage(v, &["auto"], false, true);
    let (width, height) = match parts.as_slice() {
        [one] if one.eq_ignore_ascii_case("cover") || one.eq_ignore_ascii_case("contain") => {
            return Specified::Parsed(Some(one.to_ascii_lowercase()));
        }
        [w] => (size(w), Specified::Parsed(Some("auto".into()))),
        [w, h] => (size(w), size(h)),
        _ => return Specified::Unported,
    };
    match (width, height) {
        (Specified::Parsed(Some(w)), Specified::Parsed(Some(h))) => {
            Specified::Parsed(Some(if w == "auto" && h == "auto" {
                "auto".into()
            } else {
                format!("{w} {h}")
            }))
        }
        (Specified::Parsed(None), _) | (_, Specified::Parsed(None)) => Specified::Parsed(None),
        _ => Specified::Unported,
    }
}

/// `none` or a `url()`, serialized with the authored URL as a CSS string (Stylo `CssUrl`).
/// Gradients and other image functions are [`Specified::Unported`].
pub fn image(value: &str) -> Specified {
    let quoted = |url: &str| {
        let mut out = String::from("url(");
        if cssparser::serialize_string(url, &mut out).is_err() {
            return None;
        }
        out.push(')');
        Some(out)
    };
    let mut parser = Parser::new(value);
    let result = match parser.next().cloned() {
        Ok(Token::Ident(i)) if i.eq_ignore_ascii_case("none") => Some("none".to_owned()),
        Ok(Token::UnquotedUrl(url)) => quoted(&url),
        Ok(Token::Function(f)) if f.eq_ignore_ascii_case("url") => parser
            .parse_nested_block(|p| {
                let url = p.expect_string()?.to_string();
                p.expect_exhausted()?;
                Ok::<_, cssparser::ParseError<()>>(url)
            })
            .ok()
            .and_then(|url| quoted(&url)),
        _ => return Specified::Unported,
    };
    match result {
        Some(v) if parser.is_exhausted() => Specified::Parsed(Some(v)),
        _ => Specified::Unported,
    }
}

/// One `background-repeat` layer: `repeat-x`, `repeat-y`, or one or two of `repeat`,
/// `space`, `round`, `no-repeat`, serialized as Stylo does.
pub fn background_repeat(value: &str) -> Specified {
    const KEYWORDS: [&str; 4] = ["repeat", "space", "round", "no-repeat"];
    let mut parser = Parser::new(value);
    let mut words = Vec::new();
    while !parser.is_exhausted() {
        match parser.expect_ident() {
            Ok(w) => words.push(w.to_ascii_lowercase()),
            Err(_) => return Specified::Parsed(None),
        }
    }
    let keyword = |w: &String| KEYWORDS.contains(&w.as_str());
    let (h, v) = match words.as_slice() {
        [w] if w == "repeat-x" => ("repeat", "no-repeat"),
        [w] if w == "repeat-y" => ("no-repeat", "repeat"),
        [w] if keyword(w) => (w.as_str(), w.as_str()),
        [a, b] if keyword(a) && keyword(b) => (a.as_str(), b.as_str()),
        _ => return Specified::Parsed(None),
    };
    Specified::Parsed(Some(match (h, v) {
        ("repeat", "no-repeat") => "repeat-x".into(),
        ("no-repeat", "repeat") => "repeat-y".into(),
        (h, v) if h == v => h.to_owned(),
        (h, v) => format!("{h} {v}"),
    }))
}
