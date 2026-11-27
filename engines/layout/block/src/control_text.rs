//! Text placement inside form controls laid out as blocks.
use crate::*;

/// Single-line controls center their text in the content box. Glyphs are positioned
/// individually, so this moves the runs, not just the line.
pub(crate) fn center_control_text(box_node: &mut LayoutBox) {
    if let Some(ref form) = box_node.form_state
        && matches!(
            form.kind,
            Some(
                FormControlKind::TextInput {
                    is_textarea: false,
                    ..
                } | FormControlKind::Button
                    | FormControlKind::Select
            )
        )
        && !box_node.lines.is_empty()
    {
        // Single-line controls center their text in the content box. Glyphs
        // are positioned individually, so move the runs, not just the line.
        let cb = box_node.content_box();
        let is_button = matches!(form.kind, Some(FormControlKind::Button));
        let lines_h: f64 = box_node.lines.iter().map(|l| l.rect.height).sum();
        let top = box_node.lines[0].rect.y;
        let dy = if cb.height > lines_h {
            (cb.height - lines_h) / 2.0 - top
        } else {
            0.0
        };
        let runs = || box_node.lines.iter().flat_map(|l| &l.text_runs);
        let left = runs().map(|r| r.rect.x).fold(f64::INFINITY, f64::min);
        let right = runs()
            .map(|r| r.rect.right())
            .fold(f64::NEG_INFINITY, f64::max);
        let dx = if is_button && left.is_finite() && cb.width > right - left {
            (cb.width - (right - left)) / 2.0 - left
        } else {
            0.0
        };
        if dx != 0.0 || dy != 0.0 {
            for line in &mut box_node.lines {
                line.rect.y += dy;
                for run in &mut line.text_runs {
                    run.rect.x += dx;
                    run.rect.y += dy;
                    for g in &mut run.glyphs {
                        g.x += dx as f32;
                        g.y += dy as f32;
                    }
                }
                for ib in &mut line.inline_boxes {
                    ib.rect.x += dx;
                    ib.rect.y += dy;
                }
            }
        }
    }
}
