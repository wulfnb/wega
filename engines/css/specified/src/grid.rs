//! Grid placement values (`grid-row-start` and the other line longhands).
//!
//! Ported from Stylo `values/generics/grid.rs` (`GridLine`, MPL-2.0).
use super::{Specified, custom_ident};
use cssparser::Token;

/// `auto | <custom-ident> | [ <integer> && <custom-ident>? ] | [ span && [ <integer> ||
/// <custom-ident> ] ]`, serialized as Stylo does (`span 1 a` is `span a`).
pub fn grid_line(value: &str) -> Specified {
    let mut parser = cssparser::Parser::new(value);
    let mut tokens = Vec::new();
    while let Ok(token) = parser.next() {
        tokens.push(token.clone());
    }
    if let [Token::Ident(i)] = tokens.as_slice()
        && i.eq_ignore_ascii_case("auto")
    {
        return Specified::Parsed(Some("auto".into()));
    }
    let (mut span, mut line, mut ident) = (false, None::<i32>, None::<String>);
    let mut value_before_span = false;
    for token in &tokens {
        match token {
            Token::Ident(i) if i.eq_ignore_ascii_case("span") => {
                if span {
                    return Specified::Parsed(None);
                }
                value_before_span = line.is_some() || ident.is_some();
                span = true;
            }
            Token::Number {
                int_value: Some(n), ..
            } => {
                if value_before_span || line.is_some() || *n == 0 {
                    return Specified::Parsed(None);
                }
                line = Some(*n);
            }
            Token::Ident(i) => {
                if value_before_span || ident.is_some() {
                    return Specified::Parsed(None);
                }
                match custom_ident(i, &["auto"]) {
                    Some(name) => ident = Some(name),
                    None => return Specified::Parsed(None),
                }
            }
            Token::Function(_) => return Specified::Unported,
            _ => return Specified::Parsed(None),
        }
    }
    if tokens.len() > 3 || (line.is_none() && ident.is_none()) {
        return Specified::Parsed(None);
    }
    if span && line.is_some_and(|n| n < 1) {
        return Specified::Parsed(None);
    }
    let mut out = Vec::new();
    if span {
        out.push("span".to_owned());
        if let Some(n) = line
            && !(n == 1 && ident.is_some())
        {
            out.push(n.to_string());
        }
    } else if let Some(n) = line {
        out.push(n.to_string());
    }
    out.extend(ident);
    Specified::Parsed(Some(out.join(" ")))
}

/// `grid-auto-flow`: `[ row | column ] || dense`, serialized as Stylo does (`row dense` and
/// `dense` are `dense`).
pub fn grid_auto_flow(value: &str) -> Specified {
    let Some(words) = super::idents(value) else {
        return Specified::Parsed(None);
    };
    let (mut row, mut column, mut dense) = (false, false, false);
    for w in &words {
        let flag = match w.as_str() {
            "row" => &mut row,
            "column" => &mut column,
            "dense" => &mut dense,
            _ => return Specified::Parsed(None),
        };
        if *flag {
            return Specified::Parsed(None);
        }
        *flag = true;
    }
    Specified::Parsed(match (row, column, dense) {
        (false, false, false) | (true, true, _) => None,
        (_, false, true) => Some("dense".into()),
        (true, false, false) => Some("row".into()),
        (false, true, d) => Some(if d { "column dense" } else { "column" }.into()),
    })
}
