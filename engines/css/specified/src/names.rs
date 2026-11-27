//! Values made of names: keyframes, containers and `will-change` features.
//!
//! Ported from Stylo `values/mod.rs` (`KeyframesName`, `CustomIdent`) and
//! `values/specified/{box,animation}.rs` (MPL-2.0).
use super::{Specified, custom_ident, idents_raw};
use cssparser::{Parser, Token};

/// One `animation-name`: `none`, a `<custom-ident>` or a non-empty string. A name that is a
/// valid identifier serializes as one, any other as a string (Stylo `KeyframesName`).
pub fn animation_name(value: &str) -> Specified {
    let mut parser = Parser::new(value);
    let name = match parser.next().cloned() {
        Ok(Token::Ident(i)) if i.eq_ignore_ascii_case("none") => {
            return Specified::Parsed(parser.is_exhausted().then(|| "none".into()));
        }
        Ok(Token::Ident(i)) => match custom_ident(&i, &["none"]) {
            Some(_) => i.to_string(),
            None => return Specified::Parsed(None),
        },
        Ok(Token::QuotedString(s)) if !s.is_empty() => s.to_string(),
        _ => return Specified::Parsed(None),
    };
    if !parser.is_exhausted() {
        return Specified::Parsed(None);
    }
    Specified::Parsed(custom_ident(&name, &["none"]).or_else(|| {
        let mut out = String::new();
        cssparser::serialize_string(&name, &mut out).ok()?;
        Some(out)
    }))
}

/// `container-name`: `none` or one or more names other than `none`, `not`, `and`, `or`.
pub fn container_name(value: &str) -> Specified {
    let Some(words) = idents_raw(value) else {
        return Specified::Parsed(None);
    };
    if let [one] = words.as_slice()
        && one.eq_ignore_ascii_case("none")
    {
        return Specified::Parsed(Some("none".into()));
    }
    let names: Option<Vec<String>> = words
        .iter()
        .map(|w| custom_ident(w, &["none", "not", "or", "and"]))
        .collect();
    Specified::Parsed(names.filter(|n| !n.is_empty()).map(|n| n.join(" ")))
}

/// `will-change`: `auto` or a comma-separated list of names other than `will-change`,
/// `none`, `all` and `auto`.
pub fn will_change(value: &str) -> Specified {
    let mut parser = Parser::new(value);
    if parser
        .try_parse(|p| p.expect_ident_matching("auto"))
        .is_ok()
        && parser.is_exhausted()
    {
        return Specified::Parsed(Some("auto".into()));
    }
    let mut parser = Parser::new(value);
    let names = parser.parse_entirely(|p| {
        p.parse_comma_separated(|p| {
            let ident = p.expect_ident()?.clone();
            custom_ident(&ident, &["will-change", "none", "all", "auto"])
                .ok_or_else(cssparser::ParseError::<()>::unexpected_token)
        })
    });
    Specified::Parsed(names.ok().map(|n| n.join(", ")))
}
