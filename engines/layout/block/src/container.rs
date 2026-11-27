//! Containers, margin collapsing and constraint helpers for the block formatting context.
use crate::*;

// Initialize the container through the same block sizing/positioning code used
// elsewhere; layout algorithms below only decide the positions of its contents.
pub fn init_container(b: &mut LayoutBox, cb: Rect, positioned: Rect, ctx: &LayoutContext) -> Rect {
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

pub fn collapse_margins(a: f64, b: f64) -> f64 {
    a.max(b).max(0.0) + a.min(b).min(0.0)
}

pub fn collapses_top(b: &LayoutBox, width: f64, ctx: &LayoutContext) -> bool {
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

pub fn collapsed_top(b: &LayoutBox, width: f64, ctx: &LayoutContext) -> f64 {
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

pub fn content_constraint(
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
