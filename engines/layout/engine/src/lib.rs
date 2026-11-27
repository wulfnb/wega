//! Wega-owned layout engine.
//!
//! Converts DOM nodes and `ComputedStyle` into a box tree and lays it out: block and
//! inline formatting, floats, flexbox, grid, tables, min/max constraints, positioning
//! and overflow. Each formatting context is its own crate; this crate installs the
//! dispatcher they recurse through and re-exports the public API.
#![forbid(unsafe_code)]
use wega_css_values::*;
use wega_layout_core::LayoutContext as CoreContext;
pub use wega_layout_core::*;
pub use wega_layout_text as text;

mod viewport;

/// The viewport and root font size layout runs against.
pub struct LayoutContext {
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub font_size_root: f64,
}

/// Lay out the box tree under `root` in the viewport `ctx`.
pub fn perform_layout(root: &mut LayoutBox, ctx: &LayoutContext) {
    let ctx = CoreContext {
        viewport_width: ctx.viewport_width,
        viewport_height: ctx.viewport_height,
        font_size_root: ctx.font_size_root,
        dispatch,
    };
    viewport::layout_root(root, &ctx);
}

/// Lay out one box by its formatting context.
fn dispatch(box_node: &mut LayoutBox, cb: Rect, positioned: Rect, ctx: &CoreContext) {
    match box_node.box_type {
        BoxType::Flex => wega_layout_flex::layout_flex(box_node, cb, positioned, ctx),
        BoxType::Grid => wega_layout_grid::layout_grid(box_node, cb, positioned, ctx),
        BoxType::Table => wega_layout_table::layout_table(box_node, cb, positioned, ctx),
        _ => wega_layout_block::layout_block_or_flow(box_node, cb, positioned, ctx),
    }
}
