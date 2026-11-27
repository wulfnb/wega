//! Inline formatting context.
use crate::*;
use wega_dom::NodeKey;

pub fn layout_inline_formatting_context(
    children: &mut [LayoutBox],
    avail_w: f64,
    style: &ComputedStyle,
) -> Vec<LineBox> {
    wega_layout_text::shape_inline_with_style(children, avail_w, style.text_align, Some(style))
}

// ---------------------------------------------------------------------------
// FLEXBOX ENGINE
// ---------------------------------------------------------------------------

pub fn assign_inline_geometry(children: &mut [LayoutBox], lines: &[LineBox], x: f64, y: f64) {
    fn keys(b: &LayoutBox, out: &mut std::collections::HashSet<NodeKey>) {
        if let Some(k) = b.node {
            out.insert(k);
        }
        for c in &b.children {
            keys(c, out);
        }
    }
    for child in children {
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        if child.is_atomic_inline() || child.form_state.is_some() || child.replaced_size().is_some()
        {
            if let Some(ibox) = lines
                .iter()
                .flat_map(|l| &l.inline_boxes)
                .find(|ib| child.node.is_some_and(|k| k.as_u64() == ib.id))
            {
                let target_x = x + ibox.rect.x + child.margin.left;
                let target_y = y + ibox.rect.y + child.margin.top;
                let dx = target_x - child.border_box.x;
                let dy = target_y - child.border_box.y;
                translate_tree(child, dx, dy);
                child.fragments = vec![child.border_box];
            }
        } else {
            let mut descendants = std::collections::HashSet::new();
            keys(child, &mut descendants);
            let mut frags: Vec<Rect> = lines
                .iter()
                .flat_map(|l| &l.text_runs)
                .filter(|r| r.node.is_some_and(|k| descendants.contains(&k)))
                .map(|r| Rect {
                    x: x + r.rect.x,
                    y: y + r.rect.y,
                    ..r.rect
                })
                .collect();
            if child.box_type == BoxType::Inline && !frags.is_empty() {
                let pad_l = child.padding.left + child.border.left;
                let pad_r = child.padding.right + child.border.right;
                let pad_t = child.padding.top + child.border.top;
                let pad_b = child.padding.bottom + child.border.bottom;
                let len = frags.len();
                for (idx, f) in frags.iter_mut().enumerate() {
                    f.y -= pad_t;
                    f.height += pad_t + pad_b;
                    if idx == 0 {
                        f.x -= pad_l;
                        f.width += pad_l;
                    }
                    if idx == len - 1 {
                        f.width += pad_r;
                    }
                }
            }
            child.fragments = frags;
            if let Some(first) = child.fragments.first().copied() {
                child.border_box = child.fragments.iter().fold(first, |a, b| {
                    let left = a.x.min(b.x);
                    let top = a.y.min(b.y);
                    Rect {
                        x: left,
                        y: top,
                        width: a.right().max(b.right()) - left,
                        height: a.bottom().max(b.bottom()) - top,
                    }
                });
            }
            assign_inline_geometry(&mut child.children, lines, x, y);
        }
    }
}
