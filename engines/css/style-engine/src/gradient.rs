//! Parse `linear-gradient()`.
use crate::*;
use wega_css_values::*;

/// Parse `linear-gradient(...)` (or `repeating-linear-gradient`, painted once).
pub(crate) fn linear_gradient(style: &ComputedStyle, val: &str) -> Option<LinearGradient> {
    let lower = val.trim().to_ascii_lowercase();
    let inner = lower
        .strip_prefix("linear-gradient(")
        .or_else(|| lower.strip_prefix("repeating-linear-gradient("))?
        .strip_suffix(')')?;
    let mut args = Vec::new();
    let (mut depth, mut start) = (0usize, 0);
    for (i, ch) in inner.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                args.push(inner[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    args.push(inner[start..].trim());
    let mut angle = 180.0f32;
    let mut first_stop = 0;
    // A leading direction and/or `in <colorspace>` interpolation clause.
    let head = args[0];
    let head_words: Vec<&str> = components(head);
    if head_words.first().is_some_and(|w| *w == "to" || *w == "in")
        || Length::parse(head_words.first().copied().unwrap_or("")).is_none()
            && head_words
                .first()
                .is_some_and(|w| w.ends_with("deg") || w.ends_with("turn") || w.ends_with("rad"))
    {
        first_stop = 1;
        let mut words = head_words.iter().copied().peekable();
        while let Some(w) = words.next() {
            match w {
                "to" => {
                    let mut sides = Vec::new();
                    while let Some(side) =
                        words.next_if(|s| matches!(*s, "top" | "bottom" | "left" | "right"))
                    {
                        sides.push(side);
                    }
                    let has = |s: &str| sides.contains(&s);
                    let x = if has("right") {
                        1.0
                    } else if has("left") {
                        -1.0
                    } else {
                        0.0
                    };
                    let y = if has("top") {
                        1.0
                    } else if has("bottom") {
                        -1.0
                    } else {
                        0.0
                    };
                    angle = (f32::atan2(x, y)).to_degrees().rem_euclid(360.0);
                }
                "in" => {
                    words.next();
                    let _ = words.next_if(|w| {
                        matches!(*w, "shorter" | "longer" | "increasing" | "decreasing")
                    });
                    let _ = words.next_if(|w| *w == "hue");
                }
                w => {
                    let number =
                        |suffix: &str| w.strip_suffix(suffix).and_then(|n| n.parse::<f32>().ok());
                    if let Some(d) = number("deg") {
                        angle = d;
                    } else if let Some(t) = number("turn") {
                        angle = t * 360.0;
                    } else if let Some(r) = number("rad") {
                        angle = r.to_degrees();
                    } else if let Some(g) = number("grad") {
                        angle = g * 0.9;
                    }
                }
            }
        }
    }
    let mut stops = Vec::new();
    for arg in &args[first_stop..] {
        let parts = components(arg);
        let Some(color) = parts.first().and_then(|c| color_value(style, c)) else {
            // A bare transition hint (`50%`) or an unknown stop.
            continue;
        };
        let positions: Vec<Option<f32>> = parts[1..]
            .iter()
            .map(|p| match Length::parse(p) {
                Some(Length::Percent(v)) => Some(v as f32 / 100.0),
                _ => None,
            })
            .collect();
        match positions.as_slice() {
            [] => stops.push((color, None)),
            [a] => stops.push((color, *a)),
            [a, b, ..] => {
                stops.push((color, *a));
                stops.push((color, *b));
            }
        }
    }
    (stops.len() >= 2 || stops.len() == 1).then(|| {
        if stops.len() == 1 {
            stops.push(stops[0]);
        }
        LinearGradient { angle, stops }
    })
}
