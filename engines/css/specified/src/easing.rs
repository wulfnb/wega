//! Easing functions (`animation-timing-function`, `transition-timing-function`).
//!
//! Ported from Stylo `values/{specified,generics}/easing.rs` (MPL-2.0). `linear()` and math
//! functions in arguments are not ported yet.
use super::Specified;
use cssparser::{ParseError, Parser, ToCss, Token};

/// Not ported: leave the whole value to the fallback.
struct Unported;

/// One easing function, serialized as Stylo does.
pub fn timing_function(value: &str) -> Specified {
    let mut parser = Parser::new(value);
    let result = parser.parse_entirely(|p| -> Result<Result<String, Unported>, ParseError<()>> {
        let token = p.next()?.clone();
        match token {
            Token::Ident(ident) => {
                let lower = ident.to_ascii_lowercase();
                Ok(Ok(match lower.as_str() {
                    "linear" | "ease" | "ease-in" | "ease-out" | "ease-in-out" => lower,
                    "step-start" => "steps(1, start)".into(),
                    "step-end" => "steps(1)".into(),
                    _ => return Err(ParseError::unexpected_token()),
                }))
            }
            Token::Function(name) => {
                let name = name.to_ascii_lowercase();
                p.parse_nested_block(|p| match name.as_str() {
                    "cubic-bezier" => cubic_bezier(p),
                    "steps" => steps(p),
                    "linear" => {
                        while p.next().is_ok() {}
                        Ok(Err(Unported))
                    }
                    _ => Err(ParseError::unexpected_token()),
                })
            }
            _ => Err(ParseError::unexpected_token()),
        }
    });
    match result {
        Ok(Ok(css)) => Specified::Parsed(Some(css)),
        Ok(Err(Unported)) => Specified::Unported,
        Err(_) => Specified::Parsed(None),
    }
}

/// A literal number, or `Unported` for a math function.
fn number(p: &mut Parser<'_>) -> Result<Result<f32, Unported>, ParseError<()>> {
    match p.next()?.clone() {
        Token::Number { value, .. } => Ok(Ok(value)),
        Token::Function(_) => {
            p.parse_nested_block(|p| {
                while p.next().is_ok() {}
                Ok::<_, ParseError<()>>(())
            })?;
            Ok(Err(Unported))
        }
        _ => Err(ParseError::unexpected_token()),
    }
}

fn cubic_bezier(p: &mut Parser<'_>) -> Result<Result<String, Unported>, ParseError<()>> {
    let mut values = [0.0; 4];
    for (i, slot) in values.iter_mut().enumerate() {
        if i > 0 {
            p.expect_comma()?;
        }
        match number(p)? {
            Ok(v) => *slot = v,
            Err(Unported) => {
                while p.next().is_ok() {}
                return Ok(Err(Unported));
            }
        }
    }
    let [x1, y1, x2, y2] = values;
    if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
        return Err(ParseError::unexpected_token());
    }
    let n = |v: f32| v.to_css_string();
    Ok(Ok(format!(
        "cubic-bezier({}, {}, {}, {})",
        n(x1),
        n(y1),
        n(x2),
        n(y2)
    )))
}

fn steps(p: &mut Parser<'_>) -> Result<Result<String, Unported>, ParseError<()>> {
    let count = match p.next()?.clone() {
        Token::Number {
            int_value: Some(n), ..
        } if n >= 1 => n,
        Token::Function(_) => {
            while p.next().is_ok() {}
            return Ok(Err(Unported));
        }
        _ => return Err(ParseError::unexpected_token()),
    };
    let position = if p.try_parse(|p| p.expect_comma()).is_ok() {
        let ident = p.expect_ident()?.to_ascii_lowercase();
        match ident.as_str() {
            "jump-start" | "jump-end" | "jump-none" | "jump-both" | "start" | "end" => ident,
            _ => return Err(ParseError::unexpected_token()),
        }
    } else {
        "end".into()
    };
    if position == "jump-none" && count <= 1 {
        return Err(ParseError::unexpected_token());
    }
    Ok(Ok(if position == "end" || position == "jump-end" {
        format!("steps({count})")
    } else {
        format!("steps({count}, {position})")
    }))
}
