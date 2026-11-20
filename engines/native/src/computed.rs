//! Serialization of actual native computed scalars, shared by runtime and cached reads.
use crate::css::*;
pub fn color(value: Color) -> String {
    if value.a == 255 {
        format!("rgb({}, {}, {})", value.r, value.g, value.b)
    } else {
        format!(
            "rgba({}, {}, {}, {})",
            value.r,
            value.g,
            value.b,
            value.a as f64 / 255.0
        )
    }
}
pub fn length(value: Length) -> String {
    match value {
        Length::Auto => "auto".into(),
        Length::Px(v) => format!("{v}px"),
        Length::Percent(v) => format!("{v}%"),
        Length::Em(v) => format!("{v}em"),
        Length::Rem(v) => format!("{v}rem"),
        Length::Vw(v) => format!("{v}vw"),
        Length::Vh(v) => format!("{v}vh"),
        Length::Calc([px, percent, em, rem]) => {
            format!("calc({px}px + {percent}% + {em}em + {rem}rem)")
        }
        Length::Clamp(terms) => {
            let [lo, value, hi] = terms.map(|[px, percent, em, rem]| {
                format!("calc({px}px + {percent}% + {em}em + {rem}rem)")
            });
            format!("clamp({lo}, {value}, {hi})")
        }
    }
}
pub fn scalar(style: &ComputedStyle, property: &str) -> Option<String> {
    Some(match property {
        "display" => match style.display {
            Display::InlineBlock => "inline-block".into(),
            Display::TableRow => "table-row".into(),
            Display::TableCell => "table-cell".into(),
            Display::InlineFlex => "inline-flex".into(),
            Display::InlineGrid => "inline-grid".into(),
            Display::InlineTable => "inline-table".into(),
            Display::ListItem => "list-item".into(),
            Display::FlowRoot => "flow-root".into(),
            Display::TableRowGroup => "table-row-group".into(),
            Display::TableHeaderGroup => "table-header-group".into(),
            Display::TableFooterGroup => "table-footer-group".into(),
            Display::TableCaption => "table-caption".into(),
            Display::TableColumn => "table-column".into(),
            Display::TableColumnGroup => "table-column-group".into(),
            value => format!("{value:?}").to_ascii_lowercase(),
        },
        "position" => format!("{:?}", style.position).to_ascii_lowercase(),
        "box-sizing" => if style.box_sizing == BoxSizing::BorderBox {
            "border-box"
        } else {
            "content-box"
        }
        .into(),
        "color" => color(style.color),
        "background-color" => color(style.background_color),
        "width" => length(style.width),
        "height" => length(style.height),
        "min-width" => length(style.min_width),
        "min-height" => length(style.min_height),
        "max-width" => {
            if style.max_width == Length::Auto {
                "none".into()
            } else {
                length(style.max_width)
            }
        }
        "max-height" => {
            if style.max_height == Length::Auto {
                "none".into()
            } else {
                length(style.max_height)
            }
        }
        "margin-top" => length(style.margin.top),
        "margin-right" => length(style.margin.right),
        "margin-bottom" => length(style.margin.bottom),
        "margin-left" => length(style.margin.left),
        "padding-top" => length(style.padding.top),
        "padding-right" => length(style.padding.right),
        "padding-bottom" => length(style.padding.bottom),
        "padding-left" => length(style.padding.left),
        "font-size" => format!("{}px", style.font_size),
        "font-weight" => style.font_weight.to_string(),
        "cursor" => style.cursor.clone(),
        "font-family" => style.font_family.clone(),
        "font-style" => if style.font_italic {
            "italic"
        } else {
            "normal"
        }
        .into(),
        "line-height" => format!("{}px", style.line_height),
        "opacity" => style.opacity.to_string(),
        "visibility" => if style.visible { "visible" } else { "hidden" }.into(),
        "pointer-events" => if style.pointer_events { "auto" } else { "none" }.into(),
        "overflow-x" => format!("{:?}", style.overflow_x).to_ascii_lowercase(),
        "overflow-y" => format!("{:?}", style.overflow_y).to_ascii_lowercase(),
        "top" => length(style.top),
        "right" => length(style.right),
        "bottom" => length(style.bottom),
        "left" => length(style.left),
        "z-index" => style
            .z_index
            .map_or_else(|| "auto".into(), |v| v.to_string()),
        "flex-grow" => style.flex_grow.to_string(),
        "flex-shrink" => style.flex_shrink.to_string(),
        "flex-basis" => length(style.flex_basis),
        "row-gap" => format!("{}px", style.row_gap),
        "column-gap" => format!("{}px", style.column_gap),
        name if name.starts_with("--") => style.custom_properties.get(name)?.clone(),
        _ => return None,
    })
}
