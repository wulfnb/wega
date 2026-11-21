//! Apply one declaration to a computed style.
use crate::*;
use wega_css_values::*;

pub(crate) fn apply_declaration(style: &mut ComputedStyle, prop: &str, val: &str) {
    if let Some(physical) = logical_single(prop) {
        return apply_declaration(style, physical, val);
    }
    if let Some((start, end, split)) = logical_longhands(prop) {
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
    match prop {
        "visibility" => {
            if matches!(val, "visible" | "hidden" | "collapse") {
                style.visible = val == "visible";
            }
        }
        "pointer-events" => {
            if matches!(val, "auto" | "none") {
                style.pointer_events = val == "auto";
            }
        }
        "display" => {
            if let Some(d) = Display::parse(val) {
                style.display = d;
            }
        }
        "position" => {
            if let Some(p) = Position::parse(val) {
                style.position = p;
            }
        }
        "box-sizing" => {
            if let Some(b) = BoxSizing::parse(val) {
                style.box_sizing = b;
            }
        }
        "width" => {
            if let Some(l) = Length::parse(val) {
                style.width = l;
            }
        }
        "height" => {
            if let Some(l) = Length::parse(val) {
                style.height = l;
            }
        }
        "min-width" => {
            if let Some(l) = Length::parse(val) {
                style.min_width = l;
            }
        }
        "max-width" => {
            if let Some(l) = Length::parse(val) {
                style.max_width = l;
            }
        }
        "min-height" => {
            if let Some(l) = Length::parse(val) {
                style.min_height = l;
            }
        }
        "max-height" => {
            if let Some(l) = Length::parse(val) {
                style.max_height = l;
            }
        }
        "margin" => style.margin = RectLengths::parse_quad(val),
        "margin-top" => {
            if let Some(l) = Length::parse(val) {
                style.margin.top = l;
            }
        }
        "margin-right" => {
            if let Some(l) = Length::parse(val) {
                style.margin.right = l;
            }
        }
        "margin-bottom" => {
            if let Some(l) = Length::parse(val) {
                style.margin.bottom = l;
            }
        }
        "margin-left" => {
            if let Some(l) = Length::parse(val) {
                style.margin.left = l;
            }
        }
        "padding" => style.padding = RectLengths::parse_quad(val),
        "padding-top" => {
            if let Some(l) = Length::parse(val) {
                style.padding.top = l;
            }
        }
        "padding-right" => {
            if let Some(l) = Length::parse(val) {
                style.padding.right = l;
            }
        }
        "padding-bottom" => {
            if let Some(l) = Length::parse(val) {
                style.padding.bottom = l;
            }
        }
        "padding-left" => {
            if let Some(l) = Length::parse(val) {
                style.padding.left = l;
            }
        }
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
        // Per-side longhands only; `border-width`/`-color`/`-style` themselves
        // are shorthands handled below.
        p if [
            "border-top-",
            "border-right-",
            "border-bottom-",
            "border-left-",
        ]
        .iter()
        .any(|side| p.starts_with(side))
            && (p.ends_with("-width") || p.ends_with("-color") || p.ends_with("-style")) =>
        {
            let index = if p.starts_with("border-top-") {
                0
            } else if p.starts_with("border-right-") {
                1
            } else if p.starts_with("border-bottom-") {
                2
            } else if p.starts_with("border-left-") {
                3
            } else {
                return;
            };
            if p.ends_with("-width") {
                if let Some(w) = border_width(style, val) {
                    border_side(style, index).width = w;
                }
            } else if p.ends_with("-color") {
                if let Some(c) = color_value(style, val) {
                    border_side(style, index).color = c;
                }
            } else {
                style.border_hidden[index] = matches!(val, "none" | "hidden");
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
        "border-spacing" => {
            if let Some(l) = Length::parse(val) {
                style.border_spacing = l
                    .resolve(0.0, style.font_size, style.root_font_size)
                    .max(0.0);
            }
        }
        // Elliptical and per-corner radii are approximated by one circular
        // radius: the first horizontal value.
        p if p == "border-radius" || (p.starts_with("border-") && p.ends_with("-radius")) => {
            if p != "border-radius"
                && (style.border_radius > 0.0 || style.border_radius_percent > 0.0)
            {
                return;
            }
            let first = val
                .split('/')
                .next()
                .and_then(|v| components(v).first().copied());
            match first.and_then(Length::parse) {
                Some(Length::Percent(pct)) => {
                    style.border_radius = 0.0;
                    style.border_radius_percent = pct.max(0.0);
                }
                Some(l) => {
                    style.border_radius = l
                        .resolve(0.0, style.font_size, style.root_font_size)
                        .max(0.0);
                    style.border_radius_percent = 0.0;
                }
                None => {}
            }
        }
        "color" => {
            if val.eq_ignore_ascii_case("currentcolor") {
                // Inherited color is already in place.
            } else if let Some(c) = Color::parse(val) {
                style.color = c;
            }
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
        "background-clip" | "-webkit-background-clip" => {
            style.background_clip_text = val.trim().eq_ignore_ascii_case("text");
        }
        "background-position" => {
            if let Some(p) = position(val) {
                style.background_position = p;
            }
        }
        "background-size" => {
            if let Some(size) = BackgroundSize::parse(val) {
                style.background_size = size;
            }
        }
        "background-repeat" => {
            if let Some(repeat) = background_repeat(&components(val)) {
                style.background_repeat = repeat;
            }
        }
        "clip-path" => style.clip_path = ClipPath::parse(val),
        "background-color" => {
            if let Some(c) = color_value(style, val) {
                style.background_color = c;
            }
        }
        "background" => background_shorthand(style, val),
        "cursor" => {
            if matches!(
                val,
                "auto"
                    | "default"
                    | "none"
                    | "context-menu"
                    | "help"
                    | "pointer"
                    | "progress"
                    | "wait"
                    | "cell"
                    | "crosshair"
                    | "text"
                    | "vertical-text"
                    | "alias"
                    | "copy"
                    | "move"
                    | "no-drop"
                    | "not-allowed"
                    | "grab"
                    | "grabbing"
                    | "all-scroll"
                    | "col-resize"
                    | "row-resize"
                    | "n-resize"
                    | "e-resize"
                    | "s-resize"
                    | "w-resize"
                    | "ne-resize"
                    | "nw-resize"
                    | "se-resize"
                    | "sw-resize"
                    | "ew-resize"
                    | "ns-resize"
                    | "nesw-resize"
                    | "nwse-resize"
                    | "zoom-in"
                    | "zoom-out"
            ) {
                style.cursor = val.into();
            }
        }
        "font-family" => style.font_family = val.to_owned(),
        "font-weight" => {
            style.font_weight = match val {
                "bold" => 700.0,
                "normal" => 400.0,
                _ => val
                    .parse::<f32>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .unwrap_or(style.font_weight)
                    .clamp(1.0, 1000.0),
            };
        }
        "font-style" => style.font_italic = matches!(val, "italic" | "oblique"),
        "white-space" => {
            if let Some(value) = WhiteSpace::parse(val) {
                style.white_space = value;
            }
        }
        "font-size" => {
            if let Some(l) = Length::parse(val).map(viewport_px) {
                let size = l.resolve(style.font_size, style.font_size, style.root_font_size);
                if size >= 0.0 && size.is_finite() {
                    style.font_size = size.min(4096.0);
                }
            }
        }
        "line-height" => {
            if val == "normal" {
                style.line_height_factor = Some(1.25);
            } else if let Ok(factor) = val.parse::<f64>() {
                if factor >= 0.0 && factor.is_finite() {
                    style.line_height_factor = Some(factor);
                }
            } else if let Some(l) =
                Length::parse(val).filter(|l| !matches!(l, Length::Vw(_) | Length::Vh(_)))
            {
                let h = l.resolve(style.font_size, style.font_size, style.root_font_size);
                if h >= 0.0 && h.is_finite() {
                    style.line_height = h;
                    style.line_height_factor = None;
                }
            }
        }
        "text-align" => {
            if let Some(a) = TextAlign::parse(val) {
                style.text_align = a;
            }
        }
        "top" => {
            if let Some(l) = Length::parse(val) {
                style.top = l;
            }
        }
        "right" => {
            if let Some(l) = Length::parse(val) {
                style.right = l;
            }
        }
        "bottom" => {
            if let Some(l) = Length::parse(val) {
                style.bottom = l;
            }
        }
        "left" => {
            if let Some(l) = Length::parse(val) {
                style.left = l;
            }
        }
        "z-index" => {
            style.z_index = if val.trim().eq_ignore_ascii_case("auto") {
                None
            } else {
                css_number(val).map(|n| n.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32)
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
        "float" => {
            style.float = match val.trim().to_ascii_lowercase().as_str() {
                "left" | "inline-start" => Float::Left,
                "right" | "inline-end" => Float::Right,
                _ => Float::None,
            }
        }
        "clear" => {
            style.clear = match val.trim().to_ascii_lowercase().as_str() {
                "left" | "inline-start" => (true, false),
                "right" | "inline-end" => (false, true),
                "both" => (true, true),
                _ => (false, false),
            }
        }
        "opacity" => {
            let v = val.trim();
            let op = v
                .strip_suffix('%')
                .and_then(|p| p.trim().parse::<f64>().ok().map(|p| p / 100.0))
                .or_else(|| css_number(v));
            if let Some(op) = op {
                style.opacity = op.clamp(0.0, 1.0) as f32;
            }
        }
        "overflow" => {
            if let Some(o) = Overflow::parse(val) {
                style.overflow_x = o;
                style.overflow_y = o;
            }
        }
        "overflow-x" => {
            if let Some(o) = Overflow::parse(val) {
                style.overflow_x = o;
            }
        }
        "overflow-y" => {
            if let Some(o) = Overflow::parse(val) {
                style.overflow_y = o;
            }
        }
        "flex-direction" => {
            if let Some(fd) = FlexDirection::parse(val) {
                style.flex_direction = fd;
            }
        }
        "justify-content" => {
            if let Some(jc) = JustifyContent::parse(val) {
                style.justify_content = jc;
            }
        }
        "align-items" => {
            if let Some(ai) = AlignItems::parse(val) {
                style.align_items = ai;
            }
        }
        "flex-grow" => {
            if let Some(g) = css_number(val).filter(|g| *g >= 0.0) {
                style.flex_grow = g;
            }
        }
        "flex-shrink" => {
            if let Some(s) = css_number(val).filter(|s| *s >= 0.0) {
                style.flex_shrink = s;
            }
        }
        "flex-basis" => {
            if let Some(l) = Length::parse(val) {
                style.flex_basis = l;
            }
        }
        "flex-wrap" => style.flex_wrap = val == "wrap",
        "gap" => {
            let values: Vec<_> = components(val)
                .into_iter()
                .filter_map(Length::parse)
                .collect();
            if let Some(v) = values.first() {
                style.row_gap = v
                    .resolve(0.0, style.font_size, style.root_font_size)
                    .max(0.0);
                style.column_gap = values
                    .get(1)
                    .unwrap_or(v)
                    .resolve(0.0, style.font_size, style.root_font_size)
                    .max(0.0);
            }
        }
        "row-gap" => {
            if let Some(v) = Length::parse(val) {
                style.row_gap = v
                    .resolve(0.0, style.font_size, style.root_font_size)
                    .max(0.0);
            }
        }
        "column-gap" => {
            if let Some(v) = Length::parse(val) {
                style.column_gap = v
                    .resolve(0.0, style.font_size, style.root_font_size)
                    .max(0.0);
            }
        }
        "grid-template-columns" => {
            if let Some(tracks) = tracks(val) {
                style.grid_template_columns = tracks;
            }
        }
        "grid-template-rows" => {
            if let Some(tracks) = tracks(val) {
                style.grid_template_rows = tracks;
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
        "grid-template-areas" => {
            style.grid_template_areas = grid_areas(val).unwrap_or_default();
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
        "transform" => style.transform = Transform::parse(val),
        "transform-origin" => {
            let p: Vec<_> = val.split_ascii_whitespace().collect();
            let axis = |v: &str| match v {
                "center" => Some(Length::Percent(50.0)),
                "left" | "top" => Some(Length::Percent(0.0)),
                "right" | "bottom" => Some(Length::Percent(100.0)),
                _ => Length::parse(v),
            };
            if let Some(x) = p.first().and_then(|v| axis(v)) {
                style.transform_origin = (
                    x,
                    p.get(1)
                        .and_then(|v| axis(v))
                        .unwrap_or(Length::Percent(50.0)),
                );
            }
        }
        "object-fit" => {
            if let Some(of) = ObjectFit::parse(val) {
                style.object_fit = of;
            }
        }
        "object-position" => {
            let p: Vec<_> = val.split_ascii_whitespace().collect();
            let axis = |v: &str| match v {
                "center" => Some(Length::Percent(50.0)),
                "left" | "top" => Some(Length::Percent(0.0)),
                "right" | "bottom" => Some(Length::Percent(100.0)),
                _ => Length::parse(v),
            };
            if let Some(x) = p.first().and_then(|v| axis(v)) {
                style.object_position = (
                    x,
                    p.get(1)
                        .and_then(|v| axis(v))
                        .unwrap_or(Length::Percent(50.0)),
                );
            }
        }
        "vertical-align" => {
            if let Some(va) = VerticalAlign::parse(val) {
                style.vertical_align = va;
            }
        }
        "box-shadow" => style.box_shadow = BoxShadow::parse(val),
        _ => {}
    }
}
