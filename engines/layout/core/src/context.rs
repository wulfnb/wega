//! The layout context and the formatting-context dispatch hook.
//!
//! Formatting contexts (block, inline, flex, grid, table) live in their own crates above
//! this one and recurse into each other through [`layout_box`], which calls the dispatcher
//! the `wega-layout` crate installs in [`LayoutContext::dispatch`].
use crate::*;

/// Lays out one box (border box, then its contents) in a containing block and the
/// containing block for its positioned descendants.
pub type LayoutBoxFn = fn(&mut LayoutBox, Rect, Rect, &LayoutContext);

#[derive(Clone, Copy)]
pub struct LayoutContext {
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub font_size_root: f64,
    /// Dispatches a box to its formatting context.
    pub dispatch: LayoutBoxFn,
}

/// Lay out `box_node` by its formatting context.
pub fn layout_box(
    box_node: &mut LayoutBox,
    containing_block: Rect,
    positioned_cb: Rect,
    ctx: &LayoutContext,
) {
    (ctx.dispatch)(box_node, containing_block, positioned_cb, ctx)
}

pub fn has_percent(l: Length) -> bool {
    match l {
        Length::Percent(_) => true,
        Length::Calc(terms) => terms[1] != 0.0,
        Length::Clamp(parts) => parts.iter().any(|t| t[1] != 0.0),
        _ => false,
    }
}

pub fn resolve_insets(
    lengths: &RectLengths,
    parent_w: f64,
    font_size: f64,
    root_font_size: f64,
) -> Insets {
    Insets {
        top: lengths.top.resolve(parent_w, font_size, root_font_size),
        right: lengths.right.resolve(parent_w, font_size, root_font_size),
        bottom: lengths.bottom.resolve(parent_w, font_size, root_font_size),
        left: lengths.left.resolve(parent_w, font_size, root_font_size),
    }
}

pub fn clamp_dimension(
    val: f64,
    min_len: Length,
    max_len: Length,
    parent_dim: f64,
    fs: f64,
    rfs: f64,
) -> f64 {
    let mut clamped = val;
    if max_len != Length::Auto {
        let max_val = max_len.resolve(parent_dim, fs, rfs);
        if clamped > max_val {
            clamped = max_val;
        }
    }
    if min_len != Length::Auto {
        let min_val = min_len.resolve(parent_dim, fs, rfs);
        if clamped < min_val {
            clamped = min_val;
        }
    }
    clamped.max(0.0)
}
