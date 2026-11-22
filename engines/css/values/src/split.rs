//! Generic value splitting and number helpers shared by property parsers.
use crate::*;

/// A plain or `calc()`-computed number.
pub fn css_number(val: &str) -> Option<f64> {
    let v = val.trim();
    v.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .or_else(|| calc_number(v))
}

/// Top-level comma-separated parts, keeping function arguments together.
pub fn top_level_commas(val: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, 0);
    for (i, ch) in val.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(&val[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&val[start..]);
    out
}

/// Top-level space-separated components, keeping function arguments together.
pub fn components(val: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, None);
    for (i, ch) in val.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c.is_whitespace() && depth == 0 => {
                if let Some(s) = start.take() {
                    out.push(&val[s..i]);
                }
                continue;
            }
            _ => {}
        }
        start.get_or_insert(i);
    }
    if let Some(s) = start {
        out.push(&val[s..]);
    }
    out
}

pub fn background_repeat(parts: &[&str]) -> Option<(bool, bool)> {
    let axis = |v: &str| match v {
        "repeat" | "space" | "round" => Some(true),
        "no-repeat" => Some(false),
        _ => None,
    };
    match parts {
        ["repeat-x"] => Some((true, false)),
        ["repeat-y"] => Some((false, true)),
        [one] => axis(one).map(|r| (r, r)),
        [x, y] => Some((axis(x)?, axis(y)?)),
        _ => None,
    }
}
