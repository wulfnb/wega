//! Math functions: `calc()`, `min()`, `max()`, `clamp()`.
use crate::*;
use cssparser::{ParseError, Parser, Token};
type Error = ParseError<()>;

// Length expressions retain dimensional terms until the containing block is known.
/// A unitless `calc()`/`min()`/`max()`/`clamp()` result, e.g. `calc(10 * -1)`.
pub fn calc_number(css: &str) -> Option<f64> {
    let mut p = Parser::new(css);
    p.parse_entirely(|p| {
        let name = p.expect_function()?.to_ascii_lowercase();
        p.parse_nested_block(|p| {
            let number = |q: Quantity| match q {
                Quantity::Number(n) => Ok(n),
                _ => Err(ParseError::unexpected_token()),
            };
            if name == "calc" {
                return number(expression(p, 0)?);
            }
            let args = p.parse_comma_separated(|p| expression(p, 0).and_then(number))?;
            Ok(match (name.as_str(), args.as_slice()) {
                ("min", [_, ..]) => args.iter().copied().fold(f64::INFINITY, f64::min),
                ("max", [_, ..]) => args.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                ("clamp", [lo, v, hi]) => v.min(*hi).max(*lo),
                _ => return Err(ParseError::unexpected_token()),
            })
        })
    })
    .ok()
    .filter(|n: &f64| n.is_finite())
}

pub(crate) fn narrow(v: [f64; 4]) -> [f32; 4] {
    v.map(|x| x as f32)
}

pub fn calc_length(css: &str) -> Option<Length> {
    let mut p = Parser::new(css);
    p.parse_entirely(|p| {
        let name = p.expect_function()?.to_ascii_lowercase();
        p.parse_nested_block(|p| {
            let linear = |q: Quantity| match q {
                Quantity::Length(v) => Ok(v),
                Quantity::Number(0.0) => Ok([0.0; 4]),
                _ => Err(ParseError::unexpected_token()),
            };
            let pure = |v: &[f64; 4]| v[1] == 0.0 && v[2] == 0.0 && v[3] == 0.0;
            if name == "calc" {
                return Ok(Length::Calc(linear(expression(p, 0)?)?));
            }
            let args = p.parse_comma_separated(|p| expression(p, 0).and_then(linear))?;
            let (inf, neg) = (
                [f64::INFINITY, 0.0, 0.0, 0.0],
                [f64::NEG_INFINITY, 0.0, 0.0, 0.0],
            );
            Ok(match (name.as_str(), args.as_slice()) {
                ("clamp", [lo, value, hi]) => Length::Clamp([*lo, *value, *hi].map(narrow)),
                ("min" | "max", [only]) => Length::Calc(*only),
                ("min" | "max", many) if many.iter().all(pure) => {
                    let values = many.iter().map(|v| v[0]);
                    Length::Px(if name == "min" {
                        values.fold(f64::INFINITY, f64::min)
                    } else {
                        values.fold(f64::NEG_INFINITY, f64::max)
                    })
                }
                // Mixed-unit lists beyond two arguments keep the first pair.
                ("min", [a, b, ..]) => Length::Clamp([neg, *a, *b].map(narrow)),
                ("max", [a, b, ..]) => Length::Clamp([*a, *b, inf].map(narrow)),
                _ => return Err(ParseError::unexpected_token()),
            })
        })
    })
    .ok()
}

#[derive(Clone, Copy)]
pub(crate) enum Quantity {
    Number(f64),
    Length([f64; 4]),
}

pub(crate) fn expression(p: &mut Parser<'_>, depth: usize) -> Result<Quantity, Error> {
    let mut a = product(p, depth)?;
    while !p.is_exhausted() {
        let op = match p.next()? {
            Token::Delim('+') => 1.0,
            Token::Delim('-') => -1.0,
            _ => return Err(ParseError::unexpected_token()),
        };
        let b = product(p, depth)?;
        a = match (a, b) {
            (Quantity::Number(a), Quantity::Number(b)) => Quantity::Number(a + op * b),
            (Quantity::Length(mut a), Quantity::Length(b)) => {
                for i in 0..4 {
                    a[i] += op * b[i];
                }
                Quantity::Length(a)
            }
            _ => return Err(ParseError::unexpected_token()),
        };
    }
    Ok(a)
}

pub(crate) fn product(p: &mut Parser<'_>, depth: usize) -> Result<Quantity, Error> {
    let mut a = atom(p, depth)?;
    loop {
        let state = p.state();
        let op = match p.next() {
            Ok(Token::Delim('*')) => '*',
            Ok(Token::Delim('/')) => '/',
            _ => {
                p.reset(&state);
                break;
            }
        };
        let b = atom(p, depth)?;
        let (quantity, scale) = match (a, b, op) {
            (a, Quantity::Number(b), '*') => (a, b),
            (Quantity::Number(a), b, '*') => (b, a),
            (a, Quantity::Number(b), '/') if b != 0.0 => (a, 1.0 / b),
            _ => return Err(ParseError::unexpected_token()),
        };
        a = match quantity {
            Quantity::Number(v) => Quantity::Number(v * scale),
            Quantity::Length(mut v) => {
                for x in &mut v {
                    *x *= scale;
                }
                Quantity::Length(v)
            }
        };
    }
    Ok(a)
}

pub(crate) fn atom(p: &mut Parser<'_>, depth: usize) -> Result<Quantity, Error> {
    if depth > 32 {
        return Err(ParseError::unexpected_token());
    }
    let mut terms = [0.0; 4];
    match p.next()?.clone() {
        Token::Number { value, .. } if value.is_finite() => {
            return Ok(Quantity::Number(value as f64));
        }
        Token::Percentage { unit_value, .. } if unit_value.is_finite() => {
            terms[1] = unit_value as f64 * 100.0
        }
        Token::Dimension { value, unit, .. } if value.is_finite() => {
            match Length::parse(&format!("{value}{unit}")).map(viewport_px) {
                Some(Length::Px(v)) => terms[0] = v,
                Some(Length::Em(v)) => terms[2] = v,
                Some(Length::Rem(v)) => terms[3] = v,
                _ => return Err(ParseError::unexpected_token()),
            }
        }
        Token::ParenthesisBlock => return p.parse_nested_block(|p| expression(p, depth + 1)),
        Token::Function(f) if f.eq_ignore_ascii_case("calc") => {
            return p.parse_nested_block(|p| expression(p, depth + 1));
        }
        _ => return Err(ParseError::unexpected_token()),
    }
    Ok(Quantity::Length(terms))
}

/// Skip the rest of `input`, rejecting parse-error tokens.
pub fn consume(input: &mut Parser<'_>) -> Result<(), Error> {
    consume_at(input, 0)
}

pub(crate) fn consume_at(input: &mut Parser<'_>, depth: usize) -> Result<(), Error> {
    if depth > 64 {
        return Err(ParseError::unexpected_token());
    }
    while !input.is_exhausted() {
        let token = input.next()?.clone();
        if token.is_parse_error() {
            return Err(ParseError::unexpected_token());
        }
        if matches!(
            token,
            Token::Function(_)
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::CurlyBracketBlock
        ) {
            input.parse_nested_block(|p| consume_at(p, depth + 1))?;
        }
    }
    Ok(())
}
