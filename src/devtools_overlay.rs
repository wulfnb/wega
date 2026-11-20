//! The Inspect Element overlay: a browser-owned layer painted over the page rectangle after the
//! page texture. It never touches the page DOM, layout, page hit testing or page screenshots.
//!
//! [`plan`] is pure: it turns a geometry report into the rectangles to draw (viewport CSS
//! pixels, already clipped) or an explicit refusal. It draws **nothing** when the engine says the
//! boxes are not the painted geometry (`PaintMapping::Unsupported`) or when they are unavailable,
//! because a highlight in the wrong place is worse than none.
use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use wega::engine_protocol::inspector::geometry::{
    GeomRect, GeometryReport, GeometryState, PaintMapping, UnsupportedPaint,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layer {
    Margin,
    Border,
    Padding,
    Content,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Plan {
    Draw {
        /// Exclusive bands, outermost first, in viewport CSS px and clipped to what is visible.
        bands: Vec<(Layer, GeomRect)>,
        /// The border box (clipped), for the outline.
        outline: GeomRect,
        label: String,
        /// The report describes an older document state: draw dashed.
        stale: bool,
    },
    /// Nothing is drawn; the reason is shown in the panel.
    Suppressed(String),
}

fn intersect(a: GeomRect, b: GeomRect) -> Option<GeomRect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > x && bottom > y).then_some(GeomRect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

/// `outer` minus `inner` as up to four rectangles (top, bottom, left, right).
fn ring(outer: GeomRect, inner: GeomRect) -> Vec<GeomRect> {
    let Some(inner) = intersect(outer, inner) else {
        return vec![outer];
    };
    let (ox2, oy2) = (outer.x + outer.width, outer.y + outer.height);
    let (ix2, iy2) = (inner.x + inner.width, inner.y + inner.height);
    [
        GeomRect {
            x: outer.x,
            y: outer.y,
            width: outer.width,
            height: inner.y - outer.y,
        },
        GeomRect {
            x: outer.x,
            y: iy2,
            width: outer.width,
            height: oy2 - iy2,
        },
        GeomRect {
            x: outer.x,
            y: inner.y,
            width: inner.x - outer.x,
            height: inner.height,
        },
        GeomRect {
            x: ix2,
            y: inner.y,
            width: ox2 - ix2,
            height: inner.height,
        },
    ]
    .into_iter()
    .filter(|r| r.width > 0.0 && r.height > 0.0)
    .collect()
}

pub(crate) fn label(report: &GeometryReport) -> String {
    let mut text = report.label.tag.clone();
    if !report.label.id.is_empty() {
        text.push('#');
        text.push_str(&report.label.id);
    }
    for class in report.label.classes.split_whitespace().take(3) {
        text.push('.');
        text.push_str(class);
    }
    if let Some(b) = &report.boxes {
        text.push_str(&format!(
            "  {} × {}",
            round(b.border_box.width),
            round(b.border_box.height)
        ));
    }
    text
}
fn round(v: f64) -> String {
    if (v - v.round()).abs() < 0.005 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

pub(crate) fn plan(report: &GeometryReport, current_epoch: Option<u64>) -> Plan {
    let Some(boxes) = report.boxes else {
        return Plan::Suppressed(
            match report.state {
                GeometryState::Pending => "Layout has not run yet; nothing to highlight.",
                GeometryState::NoBoxes => "This element generates no boxes.",
                GeometryState::Refused => {
                    "The document is nested too deeply to be laid out; no geometry exists."
                }
                GeometryState::Invalid => "The element no longer exists.",
                GeometryState::Current | GeometryState::Stale => "Geometry is unavailable.",
            }
            .into(),
        );
    };
    let clip = match report.paint {
        PaintMapping::Unsupported { reason } => {
            return Plan::Suppressed(
                match reason {
                    UnsupportedPaint::Transform => {
                        "Highlight suppressed: this element or an ancestor is transformed, so layout boxes are not where it paints."
                    }
                    UnsupportedPaint::ClipPositioned => {
                        "Highlight suppressed: a positioned element under a clipping ancestor cannot be placed exactly."
                    }
                    UnsupportedPaint::Unknown => {
                        "Highlight suppressed: paint geometry is not known."
                    }
                }
                .into(),
            );
        }
        PaintMapping::Exact { clip } => clip,
    };
    let viewport = GeomRect {
        x: 0.0,
        y: 0.0,
        width: report.viewport.width,
        height: report.viewport.height,
    };
    let visible = match clip {
        Some(clip) => match intersect(viewport, clip) {
            Some(v) => v,
            None => return Plan::Suppressed("The element is clipped out of view.".into()),
        },
        None => viewport,
    };
    let stale = report.state == GeometryState::Stale
        || current_epoch.is_some_and(|epoch| epoch != report.epoch);
    let layers = [
        (Layer::Margin, boxes.margin_box()),
        (Layer::Border, boxes.border_box),
        (Layer::Padding, boxes.padding_box()),
        (Layer::Content, boxes.content_box()),
    ];
    let mut bands = Vec::new();
    for (index, (layer, outer)) in layers.iter().enumerate() {
        let pieces = match layers.get(index + 1) {
            Some((_, inner)) => ring(*outer, *inner),
            None => vec![*outer],
        };
        for piece in pieces {
            if let Some(visible_piece) = intersect(piece, visible) {
                bands.push((*layer, visible_piece));
            }
        }
    }
    let Some(outline) = intersect(boxes.border_box, visible) else {
        return Plan::Suppressed("The element is outside the visible area.".into());
    };
    Plan::Draw {
        bands,
        outline,
        label: label(report),
        stale,
    }
}

fn color(layer: Layer, strong: bool) -> Color32 {
    let alpha = if strong { 110 } else { 70 };
    match layer {
        Layer::Margin => Color32::from_rgba_unmultiplied(246, 178, 107, alpha),
        Layer::Border => Color32::from_rgba_unmultiplied(255, 229, 153, alpha),
        Layer::Padding => Color32::from_rgba_unmultiplied(147, 196, 125, alpha),
        Layer::Content => Color32::from_rgba_unmultiplied(111, 168, 220, alpha),
    }
}
/// Viewport CSS px to screen points: `page.min + css * zoom`. The engine reports CSS pixels, so
/// device pixel ratio is already handled by egui's points-to-pixels step.
pub(crate) fn to_screen(page: Rect, zoom: f32, r: GeomRect) -> Rect {
    Rect::from_min_size(
        page.min + Vec2::new(r.x as f32 * zoom, r.y as f32 * zoom),
        Vec2::new(r.width as f32 * zoom, r.height as f32 * zoom),
    )
}

/// Draw a plan above the page. `strong` is the selected element; hover is lighter.
pub(crate) fn paint(painter: &egui::Painter, page: Rect, zoom: f32, plan: &Plan, strong: bool) {
    let Plan::Draw {
        bands,
        outline,
        label,
        stale,
    } = plan
    else {
        return;
    };
    let painter = painter.with_clip_rect(page);
    for (layer, rect) in bands {
        painter.rect_filled(
            to_screen(page, zoom, *rect),
            0.0,
            color(layer.to_owned(), strong),
        );
    }
    let outline = to_screen(page, zoom, *outline);
    let stroke = Stroke::new(1.0, Color32::from_rgb(60, 120, 216));
    if *stale {
        // Dashed: four corner ticks stand in for a full dashed outline.
        let tick = 6.0_f32
            .min(outline.width() / 2.0)
            .min(outline.height() / 2.0);
        for (corner, dx, dy) in [
            (outline.left_top(), 1.0, 1.0),
            (outline.right_top(), -1.0, 1.0),
            (outline.left_bottom(), 1.0, -1.0),
            (outline.right_bottom(), -1.0, -1.0),
        ] {
            painter.line_segment([corner, corner + Vec2::new(dx * tick, 0.0)], stroke);
            painter.line_segment([corner, corner + Vec2::new(0.0, dy * tick)], stroke);
        }
    } else {
        painter.rect_stroke(outline, 0.0, stroke, egui::StrokeKind::Inside);
    }
    let font = egui::FontId::monospace(11.0);
    let text = if *stale {
        format!("{label} · stale")
    } else {
        label.clone()
    };
    let galley = painter.layout_no_wrap(text, font, Color32::WHITE);
    let size = galley.size() + Vec2::new(8.0, 4.0);
    // Above the element when there is room, otherwise just below its top edge.
    let mut at = Pos2::new(outline.left(), outline.top() - size.y - 2.0);
    if at.y < page.top() {
        at.y = outline.top() + 2.0;
    }
    at.x = at.x.min(page.right() - size.x).max(page.left());
    let tag = Rect::from_min_size(at, size);
    painter.rect_filled(tag, 3.0, Color32::from_rgba_unmultiplied(40, 44, 52, 230));
    painter.galley(at + Vec2::new(4.0, 2.0), galley, Color32::WHITE);
}

#[cfg(test)]
#[path = "devtools_overlay_tests.rs"]
mod tests;
