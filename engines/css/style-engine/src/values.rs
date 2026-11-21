//! Helpers that read specified values for `apply_declaration`.
use crate::*;
use wega_css_values::*;

/// A plain or `calc()`-computed number.
pub(crate) fn css_number(val: &str) -> Option<f64> {
    let v = val.trim();
    v.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .or_else(|| calc_number(v))
}

/// Top-level comma-separated parts, keeping function arguments together.
pub(crate) fn top_level_commas(val: &str) -> Vec<&str> {
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
pub(crate) fn components(val: &str) -> Vec<&str> {
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

pub(crate) fn color_value(style: &ComputedStyle, val: &str) -> Option<Color> {
    if val.trim().eq_ignore_ascii_case("currentcolor") {
        Some(style.color)
    } else {
        Color::parse(val)
    }
}

pub(crate) fn border_width(style: &ComputedStyle, val: &str) -> Option<f64> {
    match val.to_ascii_lowercase().as_str() {
        "thin" => Some(1.0),
        "medium" => Some(3.0),
        "thick" => Some(5.0),
        _ => match Length::parse(val)? {
            Length::Auto | Length::Percent(_) => None,
            l => Some(
                l.resolve(0.0, style.font_size, style.root_font_size)
                    .max(0.0),
            ),
        },
    }
}

pub(crate) fn border_side(style: &mut ComputedStyle, index: usize) -> &mut BorderSide {
    match index {
        0 => &mut style.borders.top,
        1 => &mut style.borders.right,
        2 => &mut style.borders.bottom,
        _ => &mut style.borders.left,
    }
}

pub(crate) fn background_repeat(parts: &[&str]) -> Option<(bool, bool)> {
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

/// `background: <image> <position> [/ <size>] <repeat> ... <color>`. The
/// shorthand resets every longhand; only the final layer carries a color, and
/// only the final layer is painted.
pub(crate) fn background_shorthand(style: &mut ComputedStyle, val: &str) {
    style.background_image = None;
    style.background_gradient = None;
    style.background_position = (Length::Percent(0.0), Length::Percent(0.0));
    style.background_size = BackgroundSize::Auto;
    style.background_repeat = (true, true);
    style.background_color = Color::TRANSPARENT;
    for layer in top_level_commas(val) {
        let gradient = components(layer)
            .into_iter()
            .find(|p| p.to_ascii_lowercase().contains("linear-gradient("));
        if let Some(g) = gradient
            && style.background_gradient.is_none()
        {
            style.background_gradient = linear_gradient(style, g);
        }
    }
    // Split layers at top-level commas. Only one image layer is painted: the
    // topmost one that has an image; the final layer carries the color.
    let mut layers = Vec::new();
    let (mut depth, mut start) = (0usize, 0);
    for (i, ch) in val.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                layers.push(&val[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    layers.push(&val[start..]);
    let image_layer = layers
        .iter()
        .position(|l| l.to_ascii_lowercase().contains("url("))
        .unwrap_or(layers.len() - 1);
    for (index, layer) in layers.iter().enumerate() {
        if index == image_layer {
            background_layer(style, layer);
        }
        if index == layers.len() - 1
            && let Some(c) = components(layer).into_iter().find_map(|part| {
                let lower = part.to_ascii_lowercase();
                (!lower.contains('(')
                    || lower.starts_with("rgb")
                    || lower.starts_with("hsl")
                    || lower.starts_with("hwb"))
                .then(|| color_value(style, part))
                .flatten()
            })
        {
            style.background_color = c;
        }
    }
}

pub(crate) fn background_layer(style: &mut ComputedStyle, layer: &str) {
    let mut depth = 0usize;
    let mut slash = None;
    for (i, ch) in layer.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '/' if depth == 0 => slash = Some(i),
            _ => {}
        }
    }
    let (before, after) = match slash {
        Some(i) => (&layer[..i], &layer[i + 1..]),
        None => (layer, ""),
    };
    let mut position = Vec::new();
    let mut repeat = Vec::new();
    let mut rest: Vec<&str> = components(before);
    let after = components(after);
    let size_len = after
        .iter()
        .take(2)
        .take_while(|p| matches!(**p, "auto" | "cover" | "contain") || Length::parse(p).is_some())
        .count();
    if size_len > 0
        && let Some(size) = BackgroundSize::parse(&after[..size_len].join(" "))
    {
        style.background_size = size;
    }
    rest.extend_from_slice(&after[size_len..]);
    for part in rest {
        let lower = part.to_ascii_lowercase();
        if lower.starts_with("url(") || lower.starts_with('"') || lower.starts_with('\'') {
            style.background_image = image_url(part);
        } else if matches!(
            lower.as_str(),
            "repeat" | "no-repeat" | "repeat-x" | "repeat-y" | "space" | "round"
        ) {
            repeat.push(part);
        } else if matches!(
            lower.as_str(),
            "left" | "right" | "top" | "bottom" | "center"
        ) || Length::parse(part).is_some()
        {
            position.push(part);
        }
    }
    if let Some(r) = background_repeat(&repeat) {
        style.background_repeat = r;
    }
    if let Some(p) = wega_css_values::position(&position.join(" ")) {
        style.background_position = p;
    }
}

pub(crate) enum GridLine {
    Auto,
    Number(i32),
    Span(usize),
    Name(String),
}

pub(crate) fn grid_line(val: &str) -> GridLine {
    let v = val.trim();
    let lower = v.to_ascii_lowercase();
    if lower.is_empty() || lower == "auto" {
        GridLine::Auto
    } else if let Some(n) = lower.strip_prefix("span") {
        GridLine::Span(n.trim().parse::<usize>().unwrap_or(1).clamp(1, 256))
    } else if let Ok(n) = lower.parse::<i32>() {
        if n == 0 {
            GridLine::Auto
        } else {
            GridLine::Number(n.clamp(-256, 256))
        }
    } else {
        // `main-start`/`main-end` name the area's edges.
        let name = v.trim_end_matches("-start").trim_end_matches("-end");
        GridLine::Name(name.to_owned())
    }
}

pub(crate) fn grid_axis(style: &mut ComputedStyle, column: bool, start: GridLine, end: GridLine) {
    let (mut s, mut e, mut span, mut name) = (None, None, 1usize, None);
    match start {
        GridLine::Number(n) => s = Some(n),
        GridLine::Span(n) => span = n,
        GridLine::Name(n) => name = Some(n),
        GridLine::Auto => {}
    }
    match end {
        GridLine::Number(n) => e = Some(n),
        GridLine::Span(n) => span = n,
        GridLine::Name(n) => {
            name.get_or_insert(n);
        }
        GridLine::Auto => {}
    }
    if column {
        style.grid_column_start = s;
        style.grid_column_end = e;
        style.grid_column_span = span;
        style.grid_column_name = name;
    } else {
        style.grid_row_start = s;
        style.grid_row_end = e;
        style.grid_row_span = span;
        style.grid_row_name = name;
    }
}

pub(crate) fn grid_areas(val: &str) -> Option<Vec<Vec<String>>> {
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

/// Map a logical property to its physical equivalents for horizontal,
/// left-to-right text: `(longhands, value split into start/end)`.
pub(crate) fn logical_longhands(prop: &str) -> Option<(&'static str, &'static str, bool)> {
    Some(match prop {
        "margin-inline" => ("margin-left", "margin-right", true),
        "margin-block" => ("margin-top", "margin-bottom", true),
        "padding-inline" => ("padding-left", "padding-right", true),
        "padding-block" => ("padding-top", "padding-bottom", true),
        "inset-inline" => ("left", "right", true),
        "inset-block" => ("top", "bottom", true),
        "border-inline" => ("border-left", "border-right", false),
        "border-block" => ("border-top", "border-bottom", false),
        "border-inline-width" => ("border-left-width", "border-right-width", true),
        "border-block-width" => ("border-top-width", "border-bottom-width", true),
        "border-inline-color" => ("border-left-color", "border-right-color", true),
        "border-block-color" => ("border-top-color", "border-bottom-color", true),
        "border-inline-style" => ("border-left-style", "border-right-style", true),
        "border-block-style" => ("border-top-style", "border-bottom-style", true),
        _ => return None,
    })
}

pub(crate) fn logical_single(prop: &str) -> Option<&'static str> {
    Some(match prop {
        "margin-inline-start" => "margin-left",
        "margin-inline-end" => "margin-right",
        "margin-block-start" => "margin-top",
        "margin-block-end" => "margin-bottom",
        "padding-inline-start" => "padding-left",
        "padding-inline-end" => "padding-right",
        "padding-block-start" => "padding-top",
        "padding-block-end" => "padding-bottom",
        "inset-inline-start" => "left",
        "inset-inline-end" => "right",
        "inset-block-start" => "top",
        "inset-block-end" => "bottom",
        "inline-size" => "width",
        "block-size" => "height",
        "min-inline-size" => "min-width",
        "max-inline-size" => "max-width",
        "min-block-size" => "min-height",
        "max-block-size" => "max-height",
        "border-inline-start" => "border-left",
        "border-inline-end" => "border-right",
        "border-block-start" => "border-top",
        "border-block-end" => "border-bottom",
        "border-inline-start-width" => "border-left-width",
        "border-inline-end-width" => "border-right-width",
        "border-inline-start-color" => "border-left-color",
        "border-inline-end-color" => "border-right-color",
        "border-start-start-radius" => "border-top-left-radius",
        "border-start-end-radius" => "border-top-right-radius",
        "border-end-start-radius" => "border-bottom-left-radius",
        "border-end-end-radius" => "border-bottom-right-radius",
        _ => return None,
    })
}
