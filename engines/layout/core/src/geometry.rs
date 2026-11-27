//! Box tree geometry shared by all formatting contexts: moving subtrees, overflow, and the
//! style of anonymous boxes.
use crate::*;

pub fn translate_tree(b: &mut LayoutBox, dx: f64, dy: f64) {
    b.border_box.x += dx;
    b.border_box.y += dy;
    for r in &mut b.fragments {
        r.x += dx;
        r.y += dy;
    }
    if let Some(ref mut form) = b.form_state {
        if let Some(ref mut caret) = form.caret_rect {
            caret.x += dx;
            caret.y += dy;
        }
        for sel in &mut form.selection_rects {
            sel.x += dx;
            sel.y += dy;
        }
    }
    for c in &mut b.children {
        if c.style.position != Position::Fixed {
            translate_tree(c, dx, dy);
        }
    }
}

pub fn update_overflow(b: &mut LayoutBox) {
    let content = b.content_box();
    let mut width = content.width;
    let mut height = content.height;
    for c in &b.children {
        if c.style.position == Position::Fixed {
            continue;
        }
        width = width.max(c.border_box.right() - content.x + c.margin.right);
        height = height.max(c.border_box.bottom() - content.y + c.margin.bottom);
        if c.style.overflow_x == Overflow::Visible {
            width = width.max(c.border_box.x - content.x + c.scroll_width);
        }
        if c.style.overflow_y == Overflow::Visible {
            height = height.max(c.border_box.y - content.y + c.scroll_height);
        }
    }
    for line in &b.lines {
        width = width.max(line.rect.x + line.rect.width);
        height = height.max(line.rect.y + line.rect.height);
    }
    b.scroll_width = width + b.padding.left + b.padding.right;
    b.scroll_height = height + b.padding.top + b.padding.bottom;
}

pub fn anonymous_style(parent: &ComputedStyle) -> ComputedStyle {
    ComputedStyle {
        display: Display::Block,
        color: parent.color,
        writing_mode: parent.writing_mode,
        direction_rtl: parent.direction_rtl,
        visible: parent.visible,
        pointer_events: parent.pointer_events,
        font_size: parent.font_size,
        line_height: parent.line_height,
        line_height_factor: parent.line_height_factor,
        font_family: parent.font_family.clone(),
        font_weight: parent.font_weight,
        font_italic: parent.font_italic,
        white_space: parent.white_space,
        text_align: parent.text_align,
        custom_properties: parent.custom_properties.clone(),
        ..Default::default()
    }
}
