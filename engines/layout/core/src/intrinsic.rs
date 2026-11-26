//! Intrinsic (min-/max-content) sizing.
use crate::*;

/// Min-content (`min`) or max-content width of `b`'s content box. The box's
/// own `width` is the caller's concern; descendants contribute their outer
/// sizes, honouring definite widths. Atomic inlines are measured from their
/// contents because their own geometry is not laid out yet.
pub(crate) fn intrinsic_width(b: &LayoutBox, min: bool) -> f64 {
    intrinsic_content(b, min, 0)
}

/// Shape as layout will: with the owning block's style as the root, so the
/// measured width matches the width the line breaker later sees.
pub(crate) fn shaped_width(children: &[LayoutBox], min: bool, owner: &ComputedStyle) -> f64 {
    crate::text::shape_inline_with_style(
        children,
        if min { 0.0 } else { 1e7 },
        TextAlign::Left,
        Some(owner),
    )
    .iter()
    .map(|l| l.rect.width)
    .fold(0.0, f64::max)
}

pub(crate) fn definite(length: Length, b: &LayoutBox) -> Option<f64> {
    match length {
        Length::Auto | Length::Percent(_) => None,
        Length::Calc(v) if v[1] != 0.0 => None,
        Length::Clamp(v) if v.iter().any(|t| t[1] != 0.0) => None,
        l => Some(l.resolve(0.0, b.style.font_size, b.style.root_font_size)),
    }
}

pub(crate) fn has_atomic_inline(children: &[LayoutBox]) -> bool {
    children.iter().any(|c| {
        c.is_atomic_inline()
            || c.form_state.is_some()
            || (c.box_type == BoxType::Inline && has_atomic_inline(&c.children))
    })
}

pub(crate) fn intrinsic_content(b: &LayoutBox, min: bool, depth: usize) -> f64 {
    if let Some((iw, _)) = b.replaced_size() {
        return iw;
    }
    if b.box_type == BoxType::Text || b.text_content.is_some() {
        return shaped_width(std::slice::from_ref(b), min, &b.style);
    }
    if b.children.is_empty() {
        return 0.0;
    }
    // Controls whose content is generated text (an input's value, a select's
    // label) are measured as that text; a <button> with element content is
    // measured like any other box.
    if b.form_state.is_some() && b.children.iter().all(|c| c.box_type == BoxType::Text) {
        return shaped_width(&b.children, min, &b.style);
    }
    if depth > 96 {
        return shaped_width(&b.children, min, &b.style);
    }
    let in_flow = || {
        b.children
            .iter()
            .filter(|c| !matches!(c.style.position, Position::Absolute | Position::Fixed))
    };
    // A flex container whose main axis is horizontal: `row` in horizontal writing modes,
    // `column` in vertical ones (as in `layout_flex`).
    let row_like = matches!(
        b.style.flex_direction,
        FlexDirection::Row | FlexDirection::RowReverse
    );
    let row_flex = b.box_type == BoxType::Flex && row_like != b.style.writing_mode.is_vertical();
    if row_flex || b.box_type == BoxType::TableRow {
        let sizes: Vec<f64> = in_flow().map(|c| outer_intrinsic(c, min, depth)).collect();
        return if min && b.style.flex_wrap {
            sizes.into_iter().fold(0.0, f64::max)
        } else {
            // The main-axis gap: `column-gap` for `row`, `row-gap` for `column`.
            let gap = if row_like || b.box_type == BoxType::TableRow {
                b.style.column_gap
            } else {
                b.style.row_gap
            };
            let gaps = gap * sizes.len().saturating_sub(1) as f64;
            sizes.into_iter().sum::<f64>() + gaps
        };
    }
    if b.box_type == BoxType::Grid && b.style.grid_template_columns.len() > 1 {
        let columns = b.style.grid_template_columns.len();
        let sizes: Vec<f64> = in_flow().map(|c| outer_intrinsic(c, min, depth)).collect();
        let gaps = b.style.column_gap * (columns.min(sizes.len()).saturating_sub(1)) as f64;
        return sizes
            .chunks(columns)
            .map(|row| row.iter().sum::<f64>())
            .fold(0.0, f64::max)
            + gaps;
    }
    let inline_level = |c: &LayoutBox| {
        matches!(c.box_type, BoxType::Inline | BoxType::Text) || c.is_atomic_inline()
    };
    if b.box_type != BoxType::Flex && b.children.iter().all(inline_level) {
        if !has_atomic_inline(&b.children) {
            return shaped_width(&b.children, min, &b.style);
        }
        return inline_pieces(&b.children, min, depth);
    }
    in_flow()
        .map(|c| outer_intrinsic(c, min, depth))
        .fold(0.0, f64::max)
}

/// Inline content containing atomic boxes: text runs are shaped on their own
/// and atoms contribute their outer sizes. Max-content sums the pieces on one
/// line; min-content is the widest unbreakable piece.
pub(crate) fn inline_pieces(children: &[LayoutBox], min: bool, depth: usize) -> f64 {
    let mut total = 0.0;
    let mut widest: f64 = 0.0;
    for c in children {
        if matches!(c.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        let w = if c.is_atomic_inline() || c.form_state.is_some() || c.replaced_size().is_some() {
            outer_intrinsic(c, min, depth)
        } else if c.box_type == BoxType::Inline && has_atomic_inline(&c.children) {
            let frame = resolve_insets(
                &c.style.padding,
                0.0,
                c.style.font_size,
                c.style.root_font_size,
            );
            let margin = resolve_insets(
                &c.style.margin,
                0.0,
                c.style.font_size,
                c.style.root_font_size,
            );
            let edges = frame.left
                + frame.right
                + margin.left
                + margin.right
                + c.style.borders.left.width
                + c.style.borders.right.width;
            inline_pieces(&c.children, min, depth + 1) + if min { 0.0 } else { edges }
        } else {
            shaped_width(std::slice::from_ref(c), min, &c.style)
        };
        total += w;
        widest = widest.max(w);
    }
    if min { widest } else { total }
}

/// Margin-box contribution of a child to its parent's intrinsic width.
pub(crate) fn outer_intrinsic(c: &LayoutBox, min: bool, depth: usize) -> f64 {
    if c.box_type == BoxType::Text {
        return shaped_width(std::slice::from_ref(c), min, &c.style);
    }
    let fs = c.style.font_size;
    let rfs = c.style.root_font_size;
    let padding = resolve_insets(&c.style.padding, 0.0, fs, rfs);
    let margin = resolve_insets(&c.style.margin, 0.0, fs, rfs);
    let frame =
        padding.left + padding.right + c.style.borders.left.width + c.style.borders.right.width;
    let content = match definite(c.style.width, c) {
        Some(w) if c.style.box_sizing == BoxSizing::BorderBox => (w - frame).max(0.0),
        Some(w) => w,
        None => {
            let mut w = intrinsic_content(c, min, depth + 1);
            let border_box = c.style.box_sizing == BoxSizing::BorderBox;
            if let Some(max) = definite(c.style.max_width, c) {
                w = w.min(if border_box {
                    (max - frame).max(0.0)
                } else {
                    max
                });
            }
            if let Some(lo) = definite(c.style.min_width, c) {
                w = w.max(if border_box {
                    (lo - frame).max(0.0)
                } else {
                    lo
                });
            }
            w
        }
    };
    content + frame + margin.left.max(0.0) + margin.right.max(0.0)
}

pub(crate) fn border_main_limit(
    b: &LayoutBox,
    main: f64,
    is_row: bool,
    min: bool,
    ctx: &LayoutContext,
) -> f64 {
    let length = match (is_row, min) {
        (true, true) => b.style.min_width,
        (true, false) => b.style.max_width,
        (false, true) => b.style.min_height,
        (false, false) => b.style.max_height,
    };
    if length == Length::Auto {
        return if min {
            // Flexbox 4.5: scrollable computed overflow has a zero auto
            // minimum. A definite preferred size also caps the content minimum.
            // Otherwise a sidebar's unbreakable text can displace the main page.
            if matches!(
                b.style.overflow_x,
                Overflow::Hidden | Overflow::Auto | Overflow::Scroll
            ) || matches!(
                b.style.overflow_y,
                Overflow::Hidden | Overflow::Auto | Overflow::Scroll
            ) {
                0.0
            } else if is_row {
                let content = intrinsic_width(b, true)
                    + b.padding.left
                    + b.padding.right
                    + b.border.left
                    + b.border.right;
                if b.style.width != Length::Auto {
                    let extra = if b.style.box_sizing == BoxSizing::BorderBox {
                        0.0
                    } else {
                        b.padding.left + b.padding.right + b.border.left + b.border.right
                    };
                    content.min(
                        b.style
                            .width
                            .resolve(main, b.style.font_size, ctx.font_size_root)
                            + extra,
                    )
                } else {
                    content
                }
            } else {
                b.border_box.height
            }
        } else {
            f64::INFINITY
        };
    }
    let value = length.resolve(main, b.style.font_size, ctx.font_size_root);
    let extra = if b.style.box_sizing == BoxSizing::BorderBox {
        0.0
    } else if is_row {
        b.padding.left + b.padding.right + b.border.left + b.border.right
    } else {
        b.padding.top + b.padding.bottom + b.border.top + b.border.bottom
    };
    (value + extra).max(0.0)
}

pub(crate) fn forced_size(
    b: &mut LayoutBox,
    cb: Rect,
    positioned: Rect,
    ctx: &LayoutContext,
    width: Option<f64>,
    height: Option<f64>,
) {
    let old = (b.style.width, b.style.height);
    if let Some(w) = width {
        let padding = resolve_insets(
            &b.style.padding,
            cb.width,
            b.style.font_size,
            ctx.font_size_root,
        );
        let extra = if b.style.box_sizing == BoxSizing::BorderBox {
            0.0
        } else {
            padding.left + padding.right + b.style.borders.left.width + b.style.borders.right.width
        };
        b.style.width = Length::Px((w - extra).max(0.0));
    }
    if let Some(h) = height {
        let padding = resolve_insets(
            &b.style.padding,
            cb.width,
            b.style.font_size,
            ctx.font_size_root,
        );
        let extra = if b.style.box_sizing == BoxSizing::BorderBox {
            0.0
        } else {
            padding.top + padding.bottom + b.style.borders.top.width + b.style.borders.bottom.width
        };
        b.style.height = Length::Px((h - extra).max(0.0));
    }
    layout_box(b, cb, positioned, ctx);
    b.style.width = old.0;
    b.style.height = old.1;
}
