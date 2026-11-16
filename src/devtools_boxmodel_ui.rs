//! Read-only Box Model panel: the four CSS boxes of the selected element from the inspector's
//! cached geometry report. It states freshness and every unavailable or unsupported case instead
//! of showing invented numbers, and offers no editing.
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, Ui, Vec2};
use wega::engine_protocol::inspector::geometry::{
    BoxSet, Edges, GeometryReport, GeometryState, PaintMapping, UnsupportedPaint,
};

fn num(v: f64) -> String {
    if (v - v.round()).abs() < 0.005 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.2}")
    }
}

/// What the panel says about a report (also what tests assert).
pub(crate) fn status(report: Option<&GeometryReport>, current_epoch: Option<u64>) -> String {
    let Some(report) = report else {
        return "Select an element to see its box model. Geometry is read from cached layout; it never forces layout.".into();
    };
    let mut text = match report.state {
        GeometryState::Current => "Current layout".to_string(),
        GeometryState::Stale => {
            "Stale: layout is older than the document (it updates when the page lays out again)"
                .to_string()
        }
        GeometryState::Pending => "Pending: the page has not laid out yet".to_string(),
        GeometryState::NoBoxes => {
            "This element generates no boxes (display: none/contents)".to_string()
        }
        GeometryState::Refused => {
            "Unavailable: the document is nested too deeply to be laid out".to_string()
        }
        GeometryState::Invalid => "The element no longer exists".to_string(),
    };
    if report.state == GeometryState::Current && current_epoch.is_some_and(|e| e != report.epoch) {
        text.push_str(" · refreshing");
    }
    if report.boxes.is_some() {
        match report.paint {
            PaintMapping::Exact { clip: Some(_) } => {
                text.push_str(" · clipped by an ancestor (boxes shown unclipped)")
            }
            PaintMapping::Exact { clip: None } => {}
            PaintMapping::Unsupported { reason } => text.push_str(match reason {
                UnsupportedPaint::Transform => {
                    " · transformed: painted position differs from these layout boxes"
                }
                UnsupportedPaint::ClipPositioned => {
                    " · positioned under a clip: painted extent not modelled"
                }
                UnsupportedPaint::Unknown => " · painted geometry unknown",
            }),
        }
    }
    text
}

fn edge_text(e: f64) -> String {
    num(e)
}

pub(crate) fn draw(ui: &mut Ui, report: Option<&GeometryReport>, current_epoch: Option<u64>) {
    ui.separator();
    ui.strong("Box model");
    ui.weak(status(report, current_epoch));
    let Some(boxes) = report.and_then(|r| r.boxes.as_ref()) else {
        return;
    };
    diagram(ui, boxes);
    let border = boxes.border_box;
    ui.monospace(format!(
        "position {}, {}  ·  size {} × {} (border box)",
        num(border.x),
        num(border.y),
        num(border.width),
        num(border.height)
    ));
    let content = boxes.content_box();
    ui.monospace(format!(
        "content {} × {}  ·  viewport-relative CSS px",
        num(content.width),
        num(content.height)
    ));
}

fn diagram(ui: &mut Ui, boxes: &BoxSet) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().min(300.0), 170.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    let layers: [(&str, Color32, &Edges); 3] = [
        ("margin", Color32::from_rgb(246, 178, 107), &boxes.margin),
        ("border", Color32::from_rgb(255, 229, 153), &boxes.border),
        ("padding", Color32::from_rgb(147, 196, 125), &boxes.padding),
    ];
    let mut current = rect.shrink(2.0);
    let step = Vec2::new(
        ((rect.width() - 4.0) / 2.0 / 4.0).min(46.0),
        ((rect.height() - 4.0) / 2.0 / 4.0).min(34.0),
    );
    for (name, color, edges) in layers {
        painter.rect_filled(current, 3.0, color.gamma_multiply(0.55));
        painter.rect_stroke(
            current,
            3.0,
            Stroke::new(1.0, Color32::from_gray(90)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            current.left_top() + Vec2::new(4.0, 1.0),
            Align2::LEFT_TOP,
            name,
            FontId::proportional(10.0),
            Color32::from_gray(40),
        );
        let font = FontId::monospace(11.0);
        let ink = Color32::BLACK;
        let center = current.center();
        painter.text(
            Pos2::new(center.x, current.top() + 1.0),
            Align2::CENTER_TOP,
            edge_text(edges.top),
            font.clone(),
            ink,
        );
        painter.text(
            Pos2::new(center.x, current.bottom() - 1.0),
            Align2::CENTER_BOTTOM,
            edge_text(edges.bottom),
            font.clone(),
            ink,
        );
        painter.text(
            Pos2::new(current.left() + 3.0, center.y),
            Align2::LEFT_CENTER,
            edge_text(edges.left),
            font.clone(),
            ink,
        );
        painter.text(
            Pos2::new(current.right() - 3.0, center.y),
            Align2::RIGHT_CENTER,
            edge_text(edges.right),
            font,
            ink,
        );
        current = Rect::from_min_max(current.min + step, current.max - step);
    }
    painter.rect_filled(
        current,
        2.0,
        Color32::from_rgb(111, 168, 220).gamma_multiply(0.7),
    );
    let content = boxes.content_box();
    painter.text(
        current.center(),
        Align2::CENTER_CENTER,
        format!("{} × {}", num(content.width), num(content.height)),
        FontId::monospace(11.0),
        Color32::BLACK,
    );
}

#[cfg(test)]
#[path = "devtools_boxmodel_ui_tests.rs"]
mod tests;
