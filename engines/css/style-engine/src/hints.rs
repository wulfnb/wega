//! Presentational hints from HTML attributes.
use wega_css_values::*;
use wega_dom::{DocumentView, NodeKey, NodeView};

/// HTML dimension values: `120`, `120px`, `50%`. Zero-or-positive only.
pub(crate) fn html_dimension(value: &str) -> Option<String> {
    let v = value.trim();
    let digits = v.trim_end_matches(['%', 'x', 'p']).trim();
    let n: f64 = digits
        .parse()
        .ok()
        .filter(|n: &f64| n.is_finite() && *n >= 0.0)?;
    Some(if v.ends_with('%') {
        format!("{n}%")
    } else {
        format!("{n}px")
    })
}

pub(crate) fn html_color(value: &str) -> Option<String> {
    let v = value.trim();
    if Color::parse(v).is_some() {
        return Some(v.to_owned());
    }
    // Legacy `bgcolor="ffffff"` without the hash.
    (v.len() == 6 && v.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("#{v}"))
}

pub(crate) fn presentational_hints<D: DocumentView>(
    doc: &D,
    key: NodeKey,
) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let Some(node) = doc.node(key) else {
        return out;
    };
    let Some(tag) = node.element_name().map(|n| n.local.to_ascii_lowercase()) else {
        return out;
    };
    let attr = |name: &str| node.attr(name).filter(|v| !v.trim().is_empty());
    let tag = tag.as_str();
    if matches!(
        tag,
        "img"
            | "iframe"
            | "video"
            | "canvas"
            | "svg"
            | "embed"
            | "object"
            | "table"
            | "td"
            | "th"
            | "col"
            | "hr"
            | "input"
    ) {
        for property in ["width", "height"] {
            if (tag == "input" && attr("type").is_none_or(|t| !t.eq_ignore_ascii_case("image")))
                || (tag == "hr" && property == "height")
            {
                continue;
            }
            if let Some(v) = attr(property).and_then(html_dimension) {
                out.push((property, v));
            }
        }
    }
    if matches!(
        tag,
        "body" | "table" | "tr" | "td" | "th" | "thead" | "tbody" | "tfoot"
    ) && let Some(c) = attr("bgcolor").and_then(html_color)
    {
        out.push(("background-color", c));
    }
    if tag == "body"
        && let Some(c) = attr("text").and_then(html_color)
    {
        out.push(("color", c));
    }
    if let Some(align) = attr("align").map(str::to_ascii_lowercase) {
        match (tag, align.as_str()) {
            ("table", "center") => {
                out.push(("margin-left", "auto".into()));
                out.push(("margin-right", "auto".into()));
            }
            ("table" | "img" | "iframe" | "object" | "embed", "left" | "right") => {
                out.push(("float", align.clone()));
            }
            (
                "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "td" | "th" | "tr"
                | "thead" | "tbody" | "tfoot" | "caption" | "legend",
                "left" | "right" | "center" | "justify" | "middle",
            ) => {
                let value = if align == "middle" {
                    "center"
                } else {
                    align.as_str()
                };
                out.push(("text-align", value.into()));
            }
            _ => {}
        }
    }
    if matches!(tag, "td" | "th" | "tr" | "thead" | "tbody" | "tfoot")
        && let Some(v) = attr("valign").map(str::to_ascii_lowercase)
        && matches!(v.as_str(), "top" | "middle" | "bottom" | "baseline")
    {
        out.push(("vertical-align", v));
    }
    if matches!(tag, "td" | "th") && node.attr("nowrap").is_some() {
        out.push(("white-space", "nowrap".into()));
    }
    if tag == "font" {
        if let Some(c) = attr("color").and_then(html_color) {
            out.push(("color", c));
        }
        if let Some(face) = attr("face") {
            out.push(("font-family", face.to_owned()));
        }
        if let Some(size) = attr("size") {
            let size = size.trim();
            let n: i32 = size.trim_start_matches(['+', '-']).parse().unwrap_or(3);
            let n = if size.starts_with('+') {
                3 + n
            } else if size.starts_with('-') {
                3 - n
            } else {
                n
            };
            let px = [10.0, 13.0, 16.0, 18.0, 24.0, 32.0, 48.0][(n.clamp(1, 7) - 1) as usize];
            out.push(("font-size", format!("{px}px")));
        }
    }
    if tag == "table" {
        if let Some(spacing) = attr("cellspacing").and_then(html_dimension) {
            out.push(("border-spacing", spacing));
        }
        if let Some(border) = attr("border").and_then(html_dimension) {
            out.push(("border", format!("{border} outset #808080")));
        }
    }
    if matches!(tag, "td" | "th") {
        // Cells inherit padding and borders from the nearest table's attributes.
        let mut ancestor = node.parent();
        for _ in 0..3 {
            let Some(table) = ancestor.and_then(|k| doc.node(k)) else {
                break;
            };
            if table
                .element_name()
                .is_some_and(|n| n.local.eq_ignore_ascii_case("table"))
            {
                if let Some(padding) = table.attr("cellpadding").and_then(html_dimension) {
                    out.push(("padding", padding));
                }
                if table
                    .attr("border")
                    .and_then(|b| b.trim().parse::<f64>().ok())
                    .is_some_and(|b| b > 0.0)
                {
                    out.push(("border", "1px inset #808080".into()));
                }
                break;
            }
            ancestor = table.parent();
        }
    }
    if tag == "hr" && node.attr("noshade").is_some() {
        out.push(("border-style", "solid".into()));
    }
    out
}
