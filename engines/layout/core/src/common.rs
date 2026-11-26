//! Shared helpers: containing blocks, margin collapsing, overflow and hit testing.
use crate::*;

// Initialize the container through the same block sizing/positioning code used
// elsewhere; layout algorithms below only decide the positions of its contents.
pub(crate) fn init_container(
    b: &mut LayoutBox,
    cb: Rect,
    positioned: Rect,
    ctx: &LayoutContext,
) -> Rect {
    // Inline-level containers and auto-width tables shrink to fit their contents. Measure while the
    // children are still attached; the container is then sized without them.
    let width = b.style.width;
    let absolute = matches!(b.style.position, Position::Absolute | Position::Fixed);
    if (b.inline_level || b.is_floating() || (b.box_type == BoxType::Table && !absolute))
        && width == Length::Auto
    {
        let padding = resolve_insets(
            &b.style.padding,
            cb.width,
            b.style.font_size,
            ctx.font_size_root,
        );
        let frame =
            padding.left + padding.right + b.style.borders.left.width + b.style.borders.right.width;
        let margin = resolve_insets(
            &b.style.margin,
            cb.width,
            b.style.font_size,
            ctx.font_size_root,
        );
        let available = (cb.width - frame - margin.left - margin.right).max(0.0);
        let fit = intrinsic_width(b, false)
            .min(available)
            .max(intrinsic_width(b, true));
        let extra = if b.style.box_sizing == BoxSizing::BorderBox {
            frame
        } else {
            0.0
        };
        b.style.width = Length::Px(fit + extra);
    }
    let children = std::mem::take(&mut b.children);
    layout_block_or_flow(b, cb, positioned, ctx);
    b.children = children;
    b.style.width = width;
    b.content_box()
}

pub(crate) fn translate_tree(b: &mut LayoutBox, dx: f64, dy: f64) {
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

pub(crate) fn update_overflow(b: &mut LayoutBox) {
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

pub(crate) fn anonymous_style(parent: &ComputedStyle) -> ComputedStyle {
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

pub(crate) fn collapse_margins(a: f64, b: f64) -> f64 {
    a.max(b).max(0.0) + a.min(b).min(0.0)
}

pub(crate) fn collapses_top(b: &LayoutBox, width: f64, ctx: &LayoutContext) -> bool {
    !b.is_root
        && matches!(b.box_type, BoxType::Block | BoxType::AnonymousBlock)
        && b.style.overflow_x == Overflow::Visible
        && b.style.overflow_y == Overflow::Visible
        && b.style.borders.top.width == 0.0
        && b.style
            .padding
            .top
            .resolve(width, b.style.font_size, ctx.font_size_root)
            == 0.0
        && b.children
            .iter()
            .find(|c| !matches!(c.style.position, Position::Absolute | Position::Fixed))
            .is_some_and(|c| matches!(c.box_type, BoxType::Block | BoxType::AnonymousBlock))
}

pub(crate) fn collapsed_top(b: &LayoutBox, width: f64, ctx: &LayoutContext) -> f64 {
    let own = b
        .style
        .margin
        .top
        .resolve(width, b.style.font_size, ctx.font_size_root);
    if collapses_top(b, width, ctx) {
        let child = b
            .children
            .iter()
            .find(|c| !matches!(c.style.position, Position::Absolute | Position::Fixed))
            .unwrap();
        collapse_margins(own, collapsed_top(child, width, ctx))
    } else {
        own
    }
}

pub(crate) fn content_constraint(
    length: Length,
    extra: f64,
    sizing: BoxSizing,
    parent: f64,
    fs: f64,
    rfs: f64,
) -> Length {
    if length == Length::Auto || sizing == BoxSizing::ContentBox {
        length
    } else {
        Length::Px((length.resolve(parent, fs, rfs) - extra).max(0.0))
    }
}
