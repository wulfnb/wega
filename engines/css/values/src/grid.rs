//! Grid track lists.
use crate::*;
use cssparser::{ParseError, Parser, Token};
type Error = ParseError<()>;

#[derive(Debug, Clone, PartialEq)]
pub enum GridTrack {
    Length(Length),
    Fraction(f64),
    MinMax(Length, Box<GridTrack>),
    /// `repeat(auto-fill | auto-fit, …)`; expanded once the container width is known.
    AutoRepeat(Vec<GridTrack>),
}

pub fn tracks(css: &str) -> Option<Vec<GridTrack>> {
    if css.trim() == "none" {
        return Some(Vec::new());
    }
    track_list(&mut Parser::new(css), 0).ok()
}

pub(crate) fn track_list(p: &mut Parser<'_>, depth: usize) -> Result<Vec<GridTrack>, Error> {
    if depth > 16 {
        return Err(ParseError::unexpected_token());
    }
    let mut out = Vec::new();
    while !p.is_exhausted() {
        let start = p.position();
        match p.next()?.clone() {
            Token::Function(f) if f.eq_ignore_ascii_case("minmax") => {
                out.push(p.parse_nested_block(|p| {
                    let start = p.position();
                    p.parse_until_before(cssparser::Delimiter::Comma, consume)?;
                    let min = Length::parse(p.slice_from(start).trim())
                        .ok_or_else(ParseError::unexpected_token)?;
                    p.expect_comma()?;
                    let mut max = track_list(p, depth + 1)?;
                    if max.len() != 1 {
                        return Err(ParseError::unexpected_token());
                    }
                    Ok(GridTrack::MinMax(min, Box::new(max.remove(0))))
                })?)
            }
            Token::SquareBracketBlock => continue,
            Token::Ident(ref i)
                if matches!(
                    i.to_ascii_lowercase().as_str(),
                    "min-content" | "max-content" | "auto"
                ) =>
            {
                out.push(GridTrack::Length(Length::Auto))
            }
            Token::Function(f) if f.eq_ignore_ascii_case("fit-content") => {
                p.parse_nested_block(consume)?;
                out.push(GridTrack::Length(Length::Auto));
            }
            Token::Function(f)
                if f.eq_ignore_ascii_case("repeat")
                    && p.try_parse(|p| {
                        p.parse_nested_block(|p| {
                            let i = p.expect_ident()?.to_ascii_lowercase();
                            if i == "auto-fill" || i == "auto-fit" {
                                while p.next().is_ok() {}
                                Ok::<(), Error>(())
                            } else {
                                Err(ParseError::unexpected_token())
                            }
                        })
                    })
                    .is_ok() =>
            {
                // `try_parse` above consumed the block on success; re-read it.
                let mut inner = Parser::new(p.slice_from(start));
                let items = inner.parse_entirely(|p| {
                    p.expect_function()?;
                    p.parse_nested_block(|p| {
                        p.expect_ident()?;
                        p.expect_comma()?;
                        track_list(p, depth + 1)
                    })
                })?;
                out.push(GridTrack::AutoRepeat(items));
            }
            Token::Function(f) if f.eq_ignore_ascii_case("repeat") => {
                let repeated = p.parse_nested_block(|p| {
                    let count = p.expect_integer()?;
                    if !(1..=256).contains(&count) {
                        return Err(ParseError::unexpected_token());
                    }
                    p.expect_comma()?;
                    let items = track_list(p, depth + 1)?;
                    if items.len() * count as usize > 256 {
                        return Err(ParseError::unexpected_token());
                    }
                    Ok((0..count).flat_map(|_| items.clone()).collect::<Vec<_>>())
                })?;
                out.extend(repeated);
            }
            Token::Dimension { value, unit, .. }
                if unit.eq_ignore_ascii_case("fr") && value >= 0.0 && value.is_finite() =>
            {
                out.push(GridTrack::Fraction(value as f64))
            }
            token => {
                if matches!(token, Token::Function(_)) {
                    p.parse_nested_block(consume)?;
                }
                out.push(GridTrack::Length(
                    Length::parse(p.slice_from(start).trim())
                        .ok_or_else(ParseError::unexpected_token)?,
                ))
            }
        }
        if out.len() > 256 {
            return Err(ParseError::unexpected_token());
        }
    }
    if out.is_empty() {
        return Err(ParseError::unexpected_token());
    }
    Ok(out)
}

pub fn grid_areas(val: &str) -> Option<Vec<Vec<String>>> {
    if val.trim() == "none" {
        return Some(Vec::new());
    }
    let mut rows = Vec::new();
    let mut rest = val;
    while let Some(open) = rest.find(['"', '\'']) {
        let quote = rest.as_bytes()[open] as char;
        let after = &rest[open + 1..];
        let close = after.find(quote)?;
        rows.push(
            after[..close]
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        );
        rest = &after[close + 1..];
    }
    let width = rows.first().map_or(0, Vec::len);
    (width > 0 && rows.iter().all(|r| r.len() == width) && rows.len() <= 256 && width <= 256)
        .then_some(rows)
}
