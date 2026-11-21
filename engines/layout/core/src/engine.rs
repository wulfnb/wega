//! Layout entry point and per-box dispatch.
use crate::*;

pub struct LayoutContext {
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub font_size_root: f64,
}

pub fn perform_layout(root: &mut LayoutBox, ctx: &LayoutContext) {
    fn units(b: &mut LayoutBox, ctx: &LayoutContext) {
        let cv = |l: &mut Length| {
            *l = match *l {
                Length::Vw(v) => Length::Px(v * ctx.viewport_width / 100.0),
                Length::Vh(v) => Length::Px(v * ctx.viewport_height / 100.0),
                Length::Px(v) => Length::Px(v.clamp(-16_777_216.0, 16_777_216.0)),
                l => l,
            };
        };
        let s = &mut b.style;
        for l in [
            &mut s.width,
            &mut s.height,
            &mut s.min_width,
            &mut s.max_width,
            &mut s.min_height,
            &mut s.max_height,
            &mut s.top,
            &mut s.left,
            &mut s.right,
            &mut s.bottom,
            &mut s.flex_basis,
            &mut s.transform_origin.0,
            &mut s.transform_origin.1,
        ] {
            cv(l);
        }
        for r in [&mut s.margin, &mut s.padding] {
            for l in [&mut r.top, &mut r.right, &mut r.bottom, &mut r.left] {
                cv(l);
            }
        }
        fn track(t: &mut GridTrack, ctx: &LayoutContext) {
            match t {
                GridTrack::Length(l) => {
                    *l = match *l {
                        Length::Vw(v) => Length::Px(v * ctx.viewport_width / 100.0),
                        Length::Vh(v) => Length::Px(v * ctx.viewport_height / 100.0),
                        Length::Px(v) => Length::Px(v.clamp(-16_777_216.0, 16_777_216.0)),
                        l => l,
                    }
                }
                GridTrack::MinMax(l, max) => {
                    let mut min = GridTrack::Length(*l);
                    track(&mut min, ctx);
                    if let GridTrack::Length(v) = min {
                        *l = v;
                    }
                    track(max, ctx);
                }
                _ => {}
            }
        }
        for t in s
            .grid_template_columns
            .iter_mut()
            .chain(&mut s.grid_template_rows)
        {
            track(t, ctx);
        }
        for c in &mut b.children {
            units(c, ctx);
        }
    }
    units(root, ctx);
    propagate_viewport_overflow(root);
    indefinite_percent_heights(root, true, ctx);
    let containing_block = Rect {
        x: 0.0,
        y: 0.0,
        width: ctx.viewport_width,
        height: ctx.viewport_height,
    };
    layout_box(root, containing_block, containing_block, ctx);
}

/// CSS 2.1 §10.5 / css-sizing-3 §5.1: a percentage `height` (or `min-height`,
/// `max-height`) whose containing block's height is not definite behaves as
/// `auto` (`0` / `none`). The root element resolves against the initial
/// containing block, which is the viewport. Absolutely positioned boxes keep
/// their percentages because they resolve against a positioned padding box,
/// which is sized before they are laid out.
///
/// Reference: Stylo/Taffy treat these as "indefinite" through `MaybeResolve`
/// (`stylo_taffy/convert.rs`, `taffy::util::resolve`).
pub(crate) fn indefinite_percent_heights(b: &mut LayoutBox, definite: bool, ctx: &LayoutContext) {
    for c in &mut b.children {
        let absolute = matches!(c.style.position, Position::Absolute | Position::Fixed);
        let s = &mut c.style;
        if c.is_root {
            // The html element (a child of the document box).
            for l in [&mut s.height, &mut s.min_height, &mut s.max_height] {
                if let Length::Percent(p) = *l {
                    *l = Length::Px(p * ctx.viewport_height / 100.0);
                }
            }
        } else if !definite && !absolute {
            if has_percent(s.height) {
                s.height = Length::Auto;
            }
            if has_percent(s.min_height) {
                s.min_height = Length::Px(0.0);
            }
            if has_percent(s.max_height) {
                s.max_height = Length::Auto;
            }
        }
        let s = &c.style;
        let child_definite = match s.height {
            Length::Auto => {
                absolute && s.top != Length::Auto && s.bottom != Length::Auto
                    || matches!(c.box_type, BoxType::TableCell)
                    // Anonymous wrappers are transparent for percentage resolution.
                    || (c.node.is_none() && definite && !c.is_root)
            }
            l => !has_percent(l) || definite || absolute,
        };
        indefinite_percent_heights(c, child_definite, ctx);
    }
}

/// css-overflow-3 §3.3: `overflow` on the root element, or on `body` when the
/// root's is `visible`, applies to the viewport, not to the element's box. So
/// neither box clips its content (Google sets `body{overflow:hidden;height:100%}`).
/// The viewport itself does not yet honour `hidden`: the page still scrolls.
pub(crate) fn propagate_viewport_overflow(root: &mut LayoutBox) {
    let visible = |b: &LayoutBox| {
        b.style.overflow_x == Overflow::Visible && b.style.overflow_y == Overflow::Visible
    };
    let Some(html) = root.children.iter_mut().find(|c| c.is_root) else {
        return;
    };
    let html_visible = visible(html);
    html.style.overflow_x = Overflow::Visible;
    html.style.overflow_y = Overflow::Visible;
    if !html_visible {
        return;
    }
    if let Some(body) = html.children.iter_mut().find(|c| c.is_body) {
        body.style.overflow_x = Overflow::Visible;
        body.style.overflow_y = Overflow::Visible;
    }
}

pub(crate) fn has_percent(l: Length) -> bool {
    match l {
        Length::Percent(_) => true,
        Length::Calc(terms) => terms[1] != 0.0,
        Length::Clamp(parts) => parts.iter().any(|t| t[1] != 0.0),
        _ => false,
    }
}

pub(crate) fn layout_box(
    box_node: &mut LayoutBox,
    containing_block: Rect,
    positioned_cb: Rect,
    ctx: &LayoutContext,
) {
    match box_node.box_type {
        BoxType::Flex => layout_flex(box_node, containing_block, positioned_cb, ctx),
        BoxType::Grid => layout_grid(box_node, containing_block, positioned_cb, ctx),
        BoxType::Table => layout_table(box_node, containing_block, positioned_cb, ctx),
        _ => layout_block_or_flow(box_node, containing_block, positioned_cb, ctx),
    }
}

pub(crate) fn resolve_insets(
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

pub(crate) fn clamp_dimension(
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
