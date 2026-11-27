//! Block formatting context, floats and positioned children.
use crate::*;

pub fn layout_block_or_flow(
    box_node: &mut LayoutBox,
    containing_block: Rect,
    positioned_cb: Rect,
    ctx: &LayoutContext,
) {
    let fs = box_node.style.font_size;
    let rfs = ctx.font_size_root;
    let cb_w = containing_block.width;
    let cb_h = containing_block.height;

    box_node.padding = resolve_insets(&box_node.style.padding, cb_w, fs, rfs);
    box_node.border = Insets {
        top: box_node.style.borders.top.width,
        right: box_node.style.borders.right.width,
        bottom: box_node.style.borders.bottom.width,
        left: box_node.style.borders.left.width,
    };
    box_node.margin = resolve_insets(&box_node.style.margin, cb_w, fs, rfs);
    box_node.flow_margin_top = collapsed_top(box_node, cb_w, ctx);
    let collapse_children = collapses_top(box_node, cb_w, ctx);

    let total_border_w = box_node.border.left + box_node.border.right;
    let total_padding_w = box_node.padding.left + box_node.padding.right;
    let total_border_h = box_node.border.top + box_node.border.bottom;
    let total_padding_h = box_node.padding.top + box_node.padding.bottom;

    let is_absolute =
        box_node.style.position == Position::Absolute || box_node.style.position == Position::Fixed;
    let effective_cb = if box_node.style.position == Position::Fixed {
        Rect {
            x: 0.0,
            y: 0.0,
            width: ctx.viewport_width,
            height: ctx.viewport_height,
        }
    } else if is_absolute {
        positioned_cb
    } else {
        containing_block
    };

    // 1. Resolve content width
    let mut content_w = match box_node.style.width {
        Length::Px(px) => {
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (px - total_border_w - total_padding_w).max(0.0)
            } else {
                px
            }
        }
        Length::Percent(pct) => {
            let total = effective_cb.width * (pct / 100.0);
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (total - total_border_w - total_padding_w).max(0.0)
            } else {
                total
            }
        }
        Length::Auto => {
            if is_absolute {
                // If both left and right are specified, width is stretch
                if box_node.style.left != Length::Auto && box_node.style.right != Length::Auto {
                    let left_v = box_node.style.left.resolve(effective_cb.width, fs, rfs);
                    let right_v = box_node.style.right.resolve(effective_cb.width, fs, rfs);
                    (effective_cb.width
                        - left_v
                        - right_v
                        - box_node.margin.left
                        - box_node.margin.right
                        - total_border_w
                        - total_padding_w)
                        .max(0.0)
                } else {
                    intrinsic_width(box_node, false)
                        .min(effective_cb.width)
                        .max(intrinsic_width(box_node, true))
                }
            } else if !box_node.inline_level
                && !box_node.is_floating()
                && matches!(
                    box_node.box_type,
                    BoxType::Block
                        | BoxType::AnonymousBlock
                        | BoxType::TableCell
                        | BoxType::Flex
                        | BoxType::Grid
                )
            {
                (cb_w
                    - box_node.margin.left
                    - box_node.margin.right
                    - total_border_w
                    - total_padding_w)
                    .max(0.0)
            } else {
                intrinsic_width(box_node, false)
                    .min(cb_w)
                    .max(intrinsic_width(box_node, true))
            }
        }
        Length::Em(_)
        | Length::Rem(_)
        | Length::Calc(_)
        | Length::Clamp(_)
        | Length::Vw(_)
        | Length::Vh(_) => {
            let width = box_node.style.width.resolve(cb_w, fs, rfs);
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (width - total_border_w - total_padding_w).max(0.0)
            } else {
                width
            }
        }
    };

    if box_node.style.width == Length::Auto
        && let Some((iw, ih)) = box_node.replaced_size()
    {
        if box_node.style.height != Length::Auto {
            content_w =
                box_node.style.height.resolve(effective_cb.height, fs, rfs) * iw / ih.max(1.0);
        } else {
            content_w = iw;
        }
    }
    if let Some(ref form) = box_node.form_state
        && matches!(
            form.kind,
            Some(FormControlKind::Checkbox | FormControlKind::Radio)
        )
        && box_node.style.width == Length::Auto
    {
        content_w = 13.0;
    }
    // Apply min-width and max-width constraints
    content_w = clamp_dimension(
        content_w,
        content_constraint(
            box_node.style.min_width,
            total_border_w + total_padding_w,
            box_node.style.box_sizing,
            effective_cb.width,
            fs,
            rfs,
        ),
        content_constraint(
            box_node.style.max_width,
            total_border_w + total_padding_w,
            box_node.style.box_sizing,
            effective_cb.width,
            fs,
            rfs,
        ),
        effective_cb.width,
        fs,
        rfs,
    );

    let border_box_w = content_w + total_border_w + total_padding_w;

    // Auto margins use the final constrained width. A block whose auto width
    // was reduced by max-width still distributes its remaining inline space.
    let block_axis = matches!(
        box_node.box_type,
        BoxType::Block | BoxType::AnonymousBlock | BoxType::Flex | BoxType::Grid | BoxType::Table
    );
    if !is_absolute && (box_node.style.width != Length::Auto || block_axis) {
        let remaining =
            (cb_w - border_box_w - box_node.margin.left - box_node.margin.right).max(0.0);
        match (
            box_node.style.margin.left == Length::Auto,
            box_node.style.margin.right == Length::Auto,
        ) {
            (true, true) => {
                box_node.margin.left = remaining / 2.0;
                box_node.margin.right = remaining / 2.0;
            }
            (true, false) => box_node.margin.left = remaining,
            (false, true) => box_node.margin.right = remaining,
            _ => {}
        }
    }
    // 2. Resolve X coordinate
    let x = if is_absolute {
        if box_node.style.left != Length::Auto {
            effective_cb.x
                + box_node.style.left.resolve(effective_cb.width, fs, rfs)
                + box_node.margin.left
        } else if box_node.style.right != Length::Auto {
            effective_cb.x + effective_cb.width
                - box_node.style.right.resolve(effective_cb.width, fs, rfs)
                - box_node.margin.right
                - border_box_w
        } else {
            effective_cb.x + box_node.margin.left
        }
    } else {
        containing_block.x + box_node.margin.left
    };

    // 3. Resolve Y coordinate
    let y = if is_absolute {
        if box_node.style.top != Length::Auto {
            effective_cb.y
                + box_node.style.top.resolve(effective_cb.height, fs, rfs)
                + box_node.margin.top
        } else if box_node.style.bottom != Length::Auto {
            effective_cb.y + effective_cb.height
                - box_node.style.bottom.resolve(effective_cb.height, fs, rfs)
                - box_node.margin.bottom
        } else {
            effective_cb.y + box_node.margin.top
        }
    } else {
        containing_block.y + box_node.flow_margin_top
    };

    box_node.border_box.x = x;
    box_node.border_box.y = y;
    box_node.border_box.width = border_box_w;
    box_node.border_box.height = if box_node.style.height == Length::Auto {
        0.0
    } else {
        let h = box_node.style.height.resolve(effective_cb.height, fs, rfs);
        if box_node.style.box_sizing == BoxSizing::BorderBox {
            h
        } else {
            h + total_border_h + total_padding_h
        }
    };

    // Determine positioned containing block for children
    let this_is_pos = box_node.style.position != Position::Static;
    let next_positioned_cb = if this_is_pos {
        box_node.padding_box()
    } else {
        positioned_cb
    };

    let is_inline_level = |c: &LayoutBox| {
        c.box_type == BoxType::Inline || c.box_type == BoxType::Text || c.is_atomic_inline()
    };
    let is_ifc = !box_node.children.is_empty() && box_node.children.iter().all(is_inline_level);

    let mut content_h = if let Some((iw, ih)) = box_node.replaced_size() {
        if box_node.style.height != Length::Auto {
            box_node.style.height.resolve(effective_cb.height, fs, rfs)
        } else {
            content_w * ih / iw.max(1.0)
        }
    } else {
        0.0
    };

    if is_ifc {
        let inner_x = x + box_node.border.left + box_node.padding.left;
        let inner_y = y + box_node.border.top + box_node.padding.top;
        let inner_cb = Rect {
            x: inner_x,
            y: inner_y,
            width: content_w,
            height: 0.0,
        };
        fn prepare_inline(b: &mut LayoutBox, cb: Rect, positioned: Rect, ctx: &LayoutContext) {
            if b.is_atomic_inline() || b.form_state.is_some() || b.replaced_size().is_some() {
                layout_box(b, cb, positioned, ctx);
            } else {
                for child in &mut b.children {
                    prepare_inline(child, cb, positioned, ctx);
                }
            }
        }
        for child in &mut box_node.children {
            prepare_inline(child, inner_cb, next_positioned_cb, ctx);
        }
        let lines =
            layout_inline_formatting_context(&mut box_node.children, content_w, &box_node.style);
        content_h = lines.iter().map(|l| l.rect.height).sum();
        box_node.lines = lines;
        assign_inline_geometry(&mut box_node.children, &box_node.lines, inner_x, inner_y);
    } else {
        let mut cursor_y = y + box_node.border.top + box_node.padding.top;
        let inner_cb_x = x + box_node.border.left + box_node.padding.left;
        let inner_cb = Rect {
            x: inner_cb_x,
            y: cursor_y,
            width: content_w,
            height: box_node.content_box().height,
        };
        let mut previous_bottom: f64 = 0.0;
        let mut first = true;
        // Margin boxes of floats placed in this block, with their side.
        let mut floats: Vec<(Rect, bool)> = Vec::new();
        let edges = |floats: &[(Rect, bool)], top: f64, bottom: f64| {
            let (mut left, mut right) = (inner_cb.x, inner_cb.x + inner_cb.width);
            for (f, is_left) in floats {
                if f.y < bottom && f.bottom() > top {
                    if *is_left {
                        left = left.max(f.right());
                    } else {
                        right = right.min(f.x);
                    }
                }
            }
            (left, right)
        };
        let clearance = |floats: &[(Rect, bool)], clear: (bool, bool)| {
            floats
                .iter()
                .filter(|(_, is_left)| if *is_left { clear.0 } else { clear.1 })
                .map(|(f, _)| f.bottom())
                .fold(f64::NEG_INFINITY, f64::max)
        };
        for child in &mut box_node.children {
            let child_is_out_of_flow = child.style.position == Position::Absolute
                || child.style.position == Position::Fixed;

            if child_is_out_of_flow {
                continue;
            }
            if child.is_floating() {
                let is_left = child.style.float == Float::Left;
                let probe = Rect {
                    x: inner_cb.x,
                    y: cursor_y,
                    width: inner_cb.width,
                    height: inner_cb.height,
                };
                layout_box(child, probe, next_positioned_cb, ctx);
                let w = child.border_box.width + child.margin.left + child.margin.right;
                let h = child.border_box.height + child.margin.top + child.margin.bottom;
                // A float's top is no higher than earlier floats or its clearance.
                let mut top = floats
                    .iter()
                    .map(|(f, _)| f.y)
                    .fold(cursor_y, f64::max)
                    .max(clearance(&floats, child.style.clear));
                let mut x = if is_left {
                    inner_cb.x
                } else {
                    inner_cb.x + inner_cb.width - w
                };
                for _ in 0..64 {
                    let (left, right) = edges(&floats, top, top + h.max(0.01));
                    let blocked = floats
                        .iter()
                        .any(|(f, _)| f.y < top + h.max(0.01) && f.bottom() > top);
                    if right - left >= w || !blocked {
                        x = if is_left { left } else { right - w };
                        break;
                    }
                    let next = floats
                        .iter()
                        .map(|(f, _)| f.bottom())
                        .filter(|b| *b > top)
                        .fold(f64::INFINITY, f64::min);
                    if !next.is_finite() {
                        break;
                    }
                    top = next;
                }
                let dx = x - (child.border_box.x - child.margin.left);
                let dy = top - (child.border_box.y - child.margin.top);
                translate_tree(child, dx, dy);
                floats.push((
                    Rect {
                        x,
                        y: top,
                        width: w,
                        height: h,
                    },
                    is_left,
                ));
                continue;
            }
            let cleared = clearance(&floats, child.style.clear);
            if cleared > cursor_y {
                cursor_y = cleared;
                previous_bottom = 0.0;
                first = true;
            }
            let top = collapsed_top(child, content_w, ctx);
            if !first {
                let collapsed =
                    previous_bottom.max(top).max(0.0) + previous_bottom.min(top).min(0.0);
                cursor_y += collapsed - previous_bottom - top;
            }
            // Content beside a float is narrowed by the part of the float that
            // reaches past the box's own margin (approximating short lines).
            let (left_edge, right_edge) = edges(&floats, cursor_y, cursor_y + 1.0);
            let margins = resolve_insets(
                &child.style.margin,
                inner_cb.width,
                child.style.font_size,
                ctx.font_size_root,
            );
            let intrude_left = (left_edge - inner_cb.x - margins.left).max(0.0);
            let intrude_right = (inner_cb.x + inner_cb.width - right_edge - margins.right).max(0.0);
            let child_cb = Rect {
                x: inner_cb.x + intrude_left,
                y: cursor_y - if first && collapse_children { top } else { 0.0 },
                width: (inner_cb.width - intrude_left - intrude_right).max(0.0),
                height: inner_cb.height,
            };
            layout_box(child, child_cb, next_positioned_cb, ctx);

            if !child_is_out_of_flow {
                let child_outer_h =
                    child.border_box.height + child.flow_margin_top + child.margin.bottom
                        - if first && collapse_children {
                            child.flow_margin_top
                        } else {
                            0.0
                        };
                cursor_y += child_outer_h;
                content_h = cursor_y - inner_cb.y;
                previous_bottom = child.margin.bottom;
                first = false;
            }
        }
        // Blocks grow to contain their floats. Strictly only formatting-context
        // roots do; this also stands in for clearfix pseudo-elements, which
        // are not generated yet.
        let float_bottom = floats
            .iter()
            .map(|(f, _)| f.bottom())
            .fold(f64::NEG_INFINITY, f64::max);
        if float_bottom.is_finite() {
            content_h = content_h.max(float_bottom - inner_cb.y);
        }
    }

    // 4. Resolve content height
    let mut final_content_h = match box_node.style.height {
        Length::Px(px) => {
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (px - total_border_h - total_padding_h).max(0.0)
            } else {
                px
            }
        }
        Length::Percent(pct) => {
            let total = effective_cb.height * (pct / 100.0);
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (total - total_border_h - total_padding_h).max(0.0)
            } else {
                total
            }
        }
        Length::Auto => {
            if let Some(ref form) = box_node.form_state {
                if matches!(
                    form.kind,
                    Some(FormControlKind::Checkbox | FormControlKind::Radio)
                ) {
                    13.0
                } else if !matches!(
                    form.kind,
                    Some(FormControlKind::TextInput {
                        is_textarea: true,
                        ..
                    })
                ) && content_h < fs * box_node.style.line_height_factor.unwrap_or(1.25)
                    && !is_absolute
                {
                    // Empty single-line controls keep one line of height.
                    fs * box_node.style.line_height_factor.unwrap_or(1.25)
                } else if is_absolute
                    && box_node.style.top != Length::Auto
                    && box_node.style.bottom != Length::Auto
                {
                    let top_v = box_node.style.top.resolve(effective_cb.height, fs, rfs);
                    let bottom_v = box_node.style.bottom.resolve(effective_cb.height, fs, rfs);
                    (effective_cb.height
                        - top_v
                        - bottom_v
                        - box_node.margin.top
                        - box_node.margin.bottom
                        - total_border_h
                        - total_padding_h)
                        .max(0.0)
                } else {
                    content_h
                }
            } else if is_absolute
                && box_node.style.top != Length::Auto
                && box_node.style.bottom != Length::Auto
            {
                let top_v = box_node.style.top.resolve(effective_cb.height, fs, rfs);
                let bottom_v = box_node.style.bottom.resolve(effective_cb.height, fs, rfs);
                (effective_cb.height
                    - top_v
                    - bottom_v
                    - box_node.margin.top
                    - box_node.margin.bottom
                    - total_border_h
                    - total_padding_h)
                    .max(0.0)
            } else {
                content_h
            }
        }
        Length::Em(_)
        | Length::Rem(_)
        | Length::Calc(_)
        | Length::Clamp(_)
        | Length::Vw(_)
        | Length::Vh(_) => {
            let height = box_node.style.height.resolve(cb_h, fs, rfs);
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (height - total_border_h - total_padding_h).max(0.0)
            } else {
                height
            }
        }
    };

    // Apply min-height and max-height constraints
    final_content_h = clamp_dimension(
        final_content_h,
        content_constraint(
            box_node.style.min_height,
            total_border_h + total_padding_h,
            box_node.style.box_sizing,
            effective_cb.height,
            fs,
            rfs,
        ),
        content_constraint(
            box_node.style.max_height,
            total_border_h + total_padding_h,
            box_node.style.box_sizing,
            effective_cb.height,
            fs,
            rfs,
        ),
        effective_cb.height,
        fs,
        rfs,
    );

    let border_box_h = final_content_h + total_border_h + total_padding_h;
    box_node.border_box.height = border_box_h;
    center_control_text(box_node);

    // If absolute element was positioned by bottom only, adjust y now that height is known
    if is_absolute && box_node.style.top == Length::Auto && box_node.style.bottom != Length::Auto {
        box_node.border_box.y = effective_cb.y + effective_cb.height
            - box_node.style.bottom.resolve(effective_cb.height, fs, rfs)
            - box_node.margin.bottom
            - border_box_h;
    }

    // 5. Apply relative positioning offsets
    if box_node.style.position == Position::Relative {
        if box_node.style.left != Length::Auto {
            let left_off = box_node.style.left.resolve(cb_w, fs, rfs);
            box_node.border_box.x += left_off;
        } else if box_node.style.right != Length::Auto {
            let right_off = box_node.style.right.resolve(cb_w, fs, rfs);
            box_node.border_box.x -= right_off;
        }

        if box_node.style.top != Length::Auto {
            let top_off = box_node.style.top.resolve(cb_h, fs, rfs);
            box_node.border_box.y += top_off;
        } else if box_node.style.bottom != Length::Auto {
            let bottom_off = box_node.style.bottom.resolve(cb_h, fs, rfs);
            box_node.border_box.y -= bottom_off;
        }
    }

    // Move descendants with relative/bottom positioning; local line geometry
    // stays relative to its owning block.
    let dx = box_node.border_box.x - x;
    let dy = box_node.border_box.y - y;
    for child in &mut box_node.children {
        if !matches!(child.style.position, Position::Absolute | Position::Fixed) {
            translate_tree(child, dx, dy);
        }
    }
    let positioned = if this_is_pos {
        box_node.padding_box()
    } else {
        positioned_cb
    };
    let content = box_node.content_box();
    for child in &mut box_node.children {
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            layout_box(child, content, positioned, ctx);
        }
    }
    update_overflow(box_node);
}
