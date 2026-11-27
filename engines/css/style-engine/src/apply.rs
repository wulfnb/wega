//! Apply one declaration to a computed style.
use crate::*;
use wega_css_parser::{Parsed, PropertyDeclaration};
use wega_css_values::*;

pub(crate) fn apply_declaration(style: &mut ComputedStyle, prop: &str, val: &str) {
    if let Some(physical) = logical_single(prop) {
        let physical = flow_relative(physical, style.writing_mode, style.direction_rtl);
        return apply_declaration(style, &physical, val);
    }
    if let Some((start, end, split)) = logical_longhands(prop) {
        let (start, end) = (
            flow_relative(start, style.writing_mode, style.direction_rtl),
            flow_relative(end, style.writing_mode, style.direction_rtl),
        );
        let (start, end) = (start.as_str(), end.as_str());
        let parts = components(val);
        let (a, b) = match (split, parts.as_slice()) {
            (true, [one]) => (*one, *one),
            (true, [one, two]) => (*one, *two),
            (false, _) => (val, val),
            _ => return,
        };
        apply_declaration(style, start, a);
        apply_declaration(style, end, b);
        return;
    }
    if prop == "inset" {
        let parts = components(val);
        if (1..=4).contains(&parts.len()) {
            let quad = [
                parts[0],
                *parts.get(1).unwrap_or(&parts[0]),
                *parts.get(2).unwrap_or(&parts[0]),
                *parts.get(3).or(parts.get(1)).unwrap_or(&parts[0]),
            ];
            for (side, v) in ["top", "right", "bottom", "left"].into_iter().zip(quad) {
                apply_declaration(style, side, v);
            }
        }
        return;
    }
    match PropertyDeclaration::parse_value(prop, val) {
        Parsed::Typed(typed) => return apply_typed(style, typed),
        Parsed::Invalid => return,
        Parsed::Deferred => {}
    }
    match prop {
        "margin" => style.margin = RectLengths::parse_quad(val),
        "padding" => style.padding = RectLengths::parse_quad(val),
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            // Initial values: medium width, no style, currentcolor.
            let mut side = BorderSide {
                width: 3.0,
                color: style.color,
            };
            let mut styled = false;
            for part in components(val) {
                if let Some(w) = border_width(style, part) {
                    side.width = w;
                } else if let Some(c) = color_value(style, part) {
                    side.color = c;
                } else if matches!(
                    part.to_ascii_lowercase().as_str(),
                    "solid"
                        | "dotted"
                        | "dashed"
                        | "double"
                        | "groove"
                        | "ridge"
                        | "inset"
                        | "outset"
                ) {
                    styled = true;
                }
            }
            let indices: &[usize] = match prop {
                "border-top" => &[0],
                "border-right" => &[1],
                "border-bottom" => &[2],
                "border-left" => &[3],
                _ => &[0, 1, 2, 3],
            };
            for &i in indices {
                style.border_hidden[i] = !styled;
                *border_side(style, i) = side;
            }
        }
        "border-style" => {
            let p: Vec<_> = val.split_whitespace().collect();
            if !p.is_empty() {
                let values = [
                    p[0],
                    *p.get(1).unwrap_or(&p[0]),
                    *p.get(2).unwrap_or(&p[0]),
                    *p.get(3).or(p.get(1)).unwrap_or(&p[0]),
                ];
                style.border_hidden = values.map(|p| matches!(p, "none" | "hidden"));
            }
        }
        "border-width" | "border-color" => {
            let parts = components(val);
            if (1..=4).contains(&parts.len()) {
                let quad = [
                    parts[0],
                    *parts.get(1).unwrap_or(&parts[0]),
                    *parts.get(2).unwrap_or(&parts[0]),
                    *parts.get(3).or(parts.get(1)).unwrap_or(&parts[0]),
                ];
                for (i, part) in quad.into_iter().enumerate() {
                    if prop == "border-width" {
                        if let Some(w) = border_width(style, part) {
                            border_side(style, i).width = w;
                        }
                    } else if let Some(c) = color_value(style, part) {
                        border_side(style, i).color = c;
                    }
                }
            }
        }
        // Elliptical and per-corner radii are approximated by one circular
        // radius: the first horizontal value.
        p if p == "border-radius" || corner(p).is_some() => {
            let radius = |v: &str| match Length::parse(v)? {
                Length::Percent(pct) => Some((0.0, pct.max(0.0))),
                Length::Auto => None,
                l => Some((
                    l.resolve(0.0, style.font_size, style.root_font_size)
                        .max(0.0),
                    0.0,
                )),
            };
            // Horizontal radii only: the part before `/`.
            let horizontal = val.split('/').next().unwrap_or("");
            let parts = components(horizontal);
            if let Some(index) = corner(p) {
                if let Some(r) = parts.first().and_then(|v| radius(v)) {
                    style.border_radii[index] = r;
                }
                return;
            }
            let Some(values) = parts.iter().map(|v| radius(v)).collect::<Option<Vec<_>>>() else {
                return;
            };
            // CSS Backgrounds 3 §5.5: top-left, top-right, bottom-right, bottom-left.
            style.border_radii = match values.as_slice() {
                [a] => [*a; 4],
                [a, b] => [*a, *b, *a, *b],
                [a, b, c] => [*a, *b, *c, *b],
                [a, b, c, d] => [*a, *b, *c, *d],
                _ => return,
            };
        }
        "background-image" => {
            style.background_image = None;
            style.background_gradient = None;
            for layer in top_level_commas(val) {
                let lower = layer.trim().to_ascii_lowercase();
                if lower.starts_with("url(") && style.background_image.is_none() {
                    style.background_image = image_url(layer.trim());
                } else if lower.contains("linear-gradient(") && style.background_gradient.is_none()
                {
                    style.background_gradient = linear_gradient(style, layer);
                }
            }
        }
        "background" => background_shorthand(style, val),
        "font-size" => {
            if let Some(l) = Length::parse(val).map(viewport_px) {
                let size = l.resolve(style.font_size, style.font_size, style.root_font_size);
                if size >= 0.0 && size.is_finite() {
                    style.font_size = size.min(4096.0);
                }
            }
        }
        "mask-image" | "-webkit-mask-image" => style.mask_image = image_url(val),
        "mask" | "-webkit-mask" => {
            style.mask_image = components(val)
                .into_iter()
                .find(|p| p.to_ascii_lowercase().starts_with("url("))
                .and_then(image_url);
        }
        "mask-size" | "-webkit-mask-size" => {
            style.mask_size = BackgroundSize::parse(val.trim());
        }
        "mask-position" | "-webkit-mask-position" => {
            style.mask_position = position(val.trim());
        }
        "mask-repeat" | "-webkit-mask-repeat" => {
            style.mask_repeat = background_repeat(&components(val));
        }
        "overflow" => {
            if let Some(o) = Overflow::parse(val) {
                style.overflow_x = o;
                style.overflow_y = o;
            }
        }
        "grid-column" | "grid-row" => {
            let mut parts = val.split('/');
            let first = parts.next().unwrap_or("");
            let second = parts.next().unwrap_or("auto");
            grid_axis(
                style,
                prop == "grid-column",
                grid_line(first),
                grid_line(second),
            );
        }
        "grid-area" => {
            let parts: Vec<&str> = val.split('/').map(str::trim).collect();
            let line = |i: usize, fallback: usize| {
                parts.get(i).map(|p| grid_line(p)).unwrap_or_else(|| {
                    match grid_line(parts[fallback.min(parts.len() - 1)]) {
                        GridLine::Name(n) => GridLine::Name(n),
                        _ => GridLine::Auto,
                    }
                })
            };
            grid_axis(style, false, line(0, 0), line(2, 0));
            grid_axis(style, true, line(1, 0), line(3, 1));
        }
        "grid-column-start" | "grid-row-start" | "grid-column-end" | "grid-row-end" => {
            let column = prop.starts_with("grid-column");
            let (start, end) = if column {
                (style.grid_column_start, style.grid_column_end)
            } else {
                (style.grid_row_start, style.grid_row_end)
            };
            let keep = |n: Option<i32>| n.map_or(GridLine::Auto, GridLine::Number);
            if prop.ends_with("-start") {
                grid_axis(style, column, grid_line(val), keep(end));
            } else {
                grid_axis(style, column, keep(start), grid_line(val));
            }
        }
        "grid-template" => {
            let (rows, columns) = val.split_once('/').unwrap_or((val, ""));
            if rows.contains('"') || rows.contains('\'') {
                let mut areas = Vec::new();
                let mut sizes = Vec::new();
                let mut rest = rows;
                while let Some(open) = rest.find(['"', '\'']) {
                    let quote = rest.as_bytes()[open] as char;
                    let after = &rest[open + 1..];
                    let Some(close) = after.find(quote) else {
                        break;
                    };
                    areas.push(
                        after[..close]
                            .split_whitespace()
                            .map(str::to_owned)
                            .collect(),
                    );
                    rest = &after[close + 1..];
                    let size = rest.split(['"', '\'']).next().unwrap_or("").trim();
                    sizes.push(if size.is_empty() { "auto" } else { size });
                }
                style.grid_template_areas = areas;
                if let Some(tracks) = tracks(&sizes.join(" ")) {
                    style.grid_template_rows = tracks;
                }
            } else if let Some(tracks) = tracks(rows.trim()) {
                style.grid_template_rows = tracks;
                style.grid_template_areas.clear();
            }
            if let Some(tracks) = tracks(columns.trim()) {
                style.grid_template_columns = tracks;
            }
        }
        _ => {}
    }
}

/// The corner index of a physical `border-*-radius` longhand.
fn corner(property: &str) -> Option<usize> {
    match property {
        "border-top-left-radius" => Some(0),
        "border-top-right-radius" => Some(1),
        "border-bottom-right-radius" => Some(2),
        "border-bottom-left-radius" => Some(3),
        _ => None,
    }
}
