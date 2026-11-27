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

/// `font-size`: a non-negative length-percentage, an absolute size keyword, `smaller` or
/// `larger` (Stylo `FontSize`).
pub fn font_size(value: &str) -> Specified {
    const KEYWORDS: &[&str] = &[
        "xx-small",
        "x-small",
        "small",
        "medium",
        "large",
        "x-large",
        "xx-large",
        "xxx-large",
        "smaller",
        "larger",
    ];
    match single_token(value) {
        Some(Token::Ident(i)) => Specified::Parsed(
            KEYWORDS
                .iter()
                .find(|k| i.eq_ignore_ascii_case(k))
                .map(|k| (*k).to_owned()),
        ),
        _ => length_percentage(value, &[], false, true),
    }
}

/// `font-style`: `normal`, `italic` or `oblique [<angle>]?` (-90deg to 90deg). An exact
/// `0deg` angle serializes as `normal`, the default 14deg as plain `oblique`.
pub fn font_style(value: &str) -> Specified {
    let mut parser = cssparser::Parser::new(value);
    let Ok(word) = parser.expect_ident().map(|i| i.to_ascii_lowercase()) else {
        return Specified::Parsed(None);
    };
    match word.as_str() {
        "normal" | "italic" if parser.is_exhausted() => return Specified::Parsed(Some(word)),
        "oblique" => {}
        _ => return Specified::Parsed(None),
    }
    if parser.is_exhausted() {
        return Specified::Parsed(Some("oblique".into()));
    }
    let (value, unit) = match parser.next().cloned() {
        Ok(Token::Dimension { value, unit, .. }) => (value, unit.to_ascii_lowercase()),
        Ok(Token::Function(_)) => return Specified::Unported,
        _ => return Specified::Parsed(None),
    };
    let degrees = match unit.as_str() {
        "deg" => value,
        "grad" => value * 0.9,
        "rad" => value.to_degrees(),
        "turn" => value * 360.0,
        _ => return Specified::Parsed(None),
    };
    if !parser.is_exhausted() || !(-90.0..=90.0).contains(&degrees) {
        return Specified::Parsed(None);
    }
    Specified::Parsed(Some(match (value, unit.as_str()) {
        (0.0, "deg") => "normal".into(),
        (14.0, "deg") => "oblique".into(),
        _ => format!("oblique {}{unit}", value.to_css_string()),
    }))
}

/// `font-width`: a non-negative percentage or a width keyword (Stylo `FontWidth`).
pub fn font_width(value: &str) -> Specified {
    const KEYWORDS: &[&str] = &[
        "normal",
        "condensed",
        "ultra-condensed",
        "extra-condensed",
        "semi-condensed",
        "semi-expanded",
        "expanded",
        "extra-expanded",
        "ultra-expanded",
    ];
    match single_token(value) {
        Some(Token::Ident(i)) => Specified::Parsed(
            KEYWORDS
                .iter()
                .find(|k| i.eq_ignore_ascii_case(k))
                .map(|k| (*k).to_owned()),
        ),
        Some(Token::Percentage { unit_value, .. }) if unit_value.is_finite() => Specified::Parsed(
            (unit_value >= 0.0).then(|| format!("{}%", (unit_value * 100.0).to_css_string())),
        ),
        Some(Token::Function(_)) => Specified::Unported,
        _ => Specified::Parsed(None),
    }
}

/// `font-family`: a comma-separated list of quoted names, generic families, or names made of
/// identifiers, serialized as Stylo does (`SingleFontFamily`, `FamilyName`).
pub fn font_family(value: &str) -> Specified {
    const GENERICS: &[(&str, &str)] = &[
        ("serif", "serif"),
        ("sans-serif", "sans-serif"),
        ("monospace", "monospace"),
        ("-moz-fixed", "monospace"),
        ("cursive", "cursive"),
        ("fantasy", "fantasy"),
        ("system-ui", "system-ui"),
    ];
    let quote = |name: &str| {
        let mut out = String::new();
        cssparser::serialize_string(name, &mut out).ok()?;
        Some(out)
    };
    let mut parser = cssparser::Parser::new(value);
    let families = parser.parse_entirely(|p| {
        p.parse_comma_separated(|p| {
            let fail = cssparser::ParseError::<()>::unexpected_token;
            if let Ok(s) = p.try_parse(|p| p.expect_string_cloned()) {
                return quote(&s).ok_or_else(fail);
            }
            if let Ok(generic) = p.try_parse(|p| {
                let ident = p.expect_ident()?.clone();
                GENERICS
                    .iter()
                    .find(|(k, _)| ident.eq_ignore_ascii_case(k))
                    .map(|(_, css)| (*css).to_owned())
                    .ok_or_else(fail)
            }) {
                return Ok(generic);
            }
            let first = p.expect_ident()?.to_string();
            let reserved = ["inherit", "initial", "unset", "revert", "default"]
                .iter()
                .any(|k| first.eq_ignore_ascii_case(k));
            let mut words = vec![first];
            if reserved {
                words.push(p.expect_ident()?.to_string());
            }
            while let Ok(ident) = p.try_parse(|p| p.expect_ident_cloned()) {
                words.push(ident.to_string());
            }
            if words.iter().any(|w| w.contains(' ')) {
                return quote(&words.join(" ")).ok_or_else(fail);
            }
            let mut out = Vec::new();
            for word in &words {
                let mut ident = String::new();
                cssparser::serialize_identifier(word, &mut ident).map_err(|_| fail())?;
                out.push(ident);
            }
            Ok(out.join(" "))
        })
    });
    Specified::Parsed(families.ok().map(|f| f.join(", ")))
}
