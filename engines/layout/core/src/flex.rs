//! Flexbox.
use crate::*;

pub(crate) fn layout_flex(b: &mut LayoutBox, cb: Rect, positioned: Rect, ctx: &LayoutContext) {
    let inner = init_container(b, cb, positioned, ctx);
    let is_row = matches!(
        b.style.flex_direction,
        FlexDirection::Row | FlexDirection::RowReverse
    );
    let reverse = matches!(
        b.style.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let definite_height = b.style.height != Length::Auto;
    let main_available = if is_row {
        inner.width
    } else if definite_height {
        inner.height
    } else {
        f64::INFINITY
    };
    let main_gap = if is_row {
        b.style.column_gap
    } else {
        b.style.row_gap
    };
    let cross_gap = if is_row {
        b.style.row_gap
    } else {
        b.style.column_gap
    };
    let positioned = if b.style.position != Position::Static {
        b.padding_box()
    } else {
        positioned
    };
    let mut bases = vec![0.0; b.children.len()];
    let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
    let mut used = 0.0;
    for (i, c) in b.children.iter_mut().enumerate() {
        if matches!(c.style.position, Position::Absolute | Position::Fixed) {
            layout_box(c, inner, positioned, ctx);
            continue;
        }
        let natural = if is_row && c.style.width == Length::Auto {
            let padding = resolve_insets(
                &c.style.padding,
                inner.width,
                c.style.font_size,
                ctx.font_size_root,
            );
            Some(
                intrinsic_width(c, false)
                    + padding.left
                    + padding.right
                    + c.style.borders.left.width
                    + c.style.borders.right.width,
            )
        } else {
            None
        };
        forced_size(c, inner, positioned, ctx, natural, None);
        // Percentage bases resolve to content when the container main size
        // is indefinite (including 0% from flex:1). Resolving against zero
        // collapses nested auto-height columns and overlaps following content.
        let indefinite_basis = !main_available.is_finite()
            && match c.style.flex_basis {
                Length::Percent(_) => true,
                Length::Calc(v) => v[1] != 0.0,
                Length::Clamp(v) => v.iter().any(|t| t[1] != 0.0),
                _ => false,
            };
        let base = if c.style.flex_basis != Length::Auto && !indefinite_basis {
            c.style.flex_basis.resolve(
                if is_row { inner.width } else { inner.height },
                c.style.font_size,
                ctx.font_size_root,
            ) + if c.style.box_sizing == BoxSizing::ContentBox {
                if is_row {
                    c.padding.left + c.padding.right + c.border.left + c.border.right
                } else {
                    c.padding.top + c.padding.bottom + c.border.top + c.border.bottom
                }
            } else {
                0.0
            }
        } else if is_row {
            c.border_box.width
        } else {
            c.border_box.height
        };
        bases[i] = base;
        let outer = base
            + if is_row {
                c.margin.left + c.margin.right
            } else {
                c.margin.top + c.margin.bottom
            };
        if b.style.flex_wrap
            && !lines.last().unwrap().is_empty()
            && used + main_gap + outer > main_available
        {
            lines.push(Vec::new());
            used = 0.0;
        }
        if !lines.last().unwrap().is_empty() {
            used += main_gap;
        }
        lines.last_mut().unwrap().push(i);
        used += outer;
    }
    let mut cross_cursor = if is_row { inner.y } else { inner.x };
    let mut total_main: f64 = 0.0;
    let mut sizes = bases.clone();
    let mut frozen = vec![false; b.children.len()];
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let margin_sum: f64 = line
            .iter()
            .map(|i| {
                let c = &b.children[*i];
                if is_row {
                    c.margin.left + c.margin.right
                } else {
                    c.margin.top + c.margin.bottom
                }
            })
            .sum();
        let available = if main_available.is_finite() {
            main_available
        } else {
            line.iter().map(|i| bases[*i]).sum::<f64>()
                + margin_sum
                + main_gap * (line.len() - 1) as f64
        };
        let target_space = (available - margin_sum - main_gap * (line.len() - 1) as f64).max(0.0);
        let growing = target_space > line.iter().map(|i| bases[*i]).sum::<f64>();
        for i in &line {
            sizes[*i] = bases[*i];
            frozen[*i] = false;
        }
        // Freeze at min/max constraints, then redistribute only among flexible
        // items. Each pass freezes at least one item or finishes.
        for _ in 0..=line.len() {
            let free = target_space
                - line
                    .iter()
                    .map(|i| if frozen[*i] { sizes[*i] } else { bases[*i] })
                    .sum::<f64>();
            let factors: f64 = line
                .iter()
                .filter(|i| !frozen[**i])
                .map(|i| {
                    let c = &b.children[*i];
                    if growing {
                        c.style.flex_grow
                    } else {
                        c.style.flex_shrink * bases[*i]
                    }
                })
                .sum();
            let mut violation = false;
            for i in &line {
                if frozen[*i] {
                    continue;
                }
                let c = &b.children[*i];
                let factor = if growing {
                    c.style.flex_grow
                } else {
                    c.style.flex_shrink * bases[*i]
                };
                let proposed = bases[*i]
                    + if factors > 0.0 {
                        free * factor / factors
                    } else {
                        0.0
                    };
                let min = border_main_limit(c, available, is_row, true, ctx);
                let max = border_main_limit(c, available, is_row, false, ctx).max(min);
                sizes[*i] = proposed.clamp(min, max);
                if (sizes[*i] - proposed).abs() > 0.001 {
                    frozen[*i] = true;
                    violation = true;
                }
            }
            if !violation {
                break;
            }
        }
        let occupied = line.iter().map(|i| sizes[*i]).sum::<f64>()
            + margin_sum
            + main_gap * (line.len() - 1) as f64;
        let free = (available - occupied).max(0.0);
        let n = line.len() as f64;
        let (offset, extra) = match b.style.justify_content {
            JustifyContent::FlexEnd => (free, 0.0),
            JustifyContent::Center => (free / 2.0, 0.0),
            JustifyContent::SpaceBetween if n > 1.0 => (0.0, free / (n - 1.0)),
            JustifyContent::SpaceAround => (free / (2.0 * n), free / n),
            JustifyContent::SpaceEvenly => (free / (n + 1.0), free / (n + 1.0)),
            _ => (0.0, 0.0),
        };
        let mut cross: f64 = if !b.style.flex_wrap {
            if is_row && definite_height {
                inner.height
            } else if !is_row {
                inner.width
            } else {
                0.0
            }
        } else {
            0.0
        };
        for i in &line {
            let c = &mut b.children[*i];
            forced_size(
                c,
                inner,
                positioned,
                ctx,
                if is_row { Some(sizes[*i]) } else { None },
                if is_row { None } else { Some(sizes[*i]) },
            );
            cross = cross.max(if is_row {
                c.border_box.height + c.margin.top + c.margin.bottom
            } else {
                c.border_box.width + c.margin.left + c.margin.right
            });
        }
        let origin = if is_row { inner.x } else { inner.y };
        let mut cursor = if reverse {
            origin + available - offset
        } else {
            origin + offset
        };
        for i in line {
            let c = &mut b.children[i];
            let auto_cross = if is_row {
                c.style.height == Length::Auto
            } else {
                c.style.width == Length::Auto
            };
            if b.style.align_items == AlignItems::Stretch && auto_cross {
                let size = if is_row {
                    (cross - c.margin.top - c.margin.bottom).max(0.0)
                } else {
                    (cross - c.margin.left - c.margin.right).max(0.0)
                };
                forced_size(
                    c,
                    inner,
                    positioned,
                    ctx,
                    if is_row { Some(sizes[i]) } else { Some(size) },
                    if is_row { Some(size) } else { Some(sizes[i]) },
                );
            }
            let (before, after, cross_before, cross_after) = if is_row {
                (c.margin.left, c.margin.right, c.margin.top, c.margin.bottom)
            } else {
                (c.margin.top, c.margin.bottom, c.margin.left, c.margin.right)
            };
            let cross_size = if is_row {
                c.border_box.height
            } else {
                c.border_box.width
            };
            let extra_cross = (cross - cross_size - cross_before - cross_after).max(0.0);
            let cross_pos = cross_cursor
                + cross_before
                + match b.style.align_items {
                    AlignItems::Center => extra_cross / 2.0,
                    AlignItems::FlexEnd => extra_cross,
                    _ => 0.0,
                };
            let main = if reverse {
                cursor -= after + sizes[i];
                cursor
            } else {
                cursor + before
            };
            let (x, y) = if is_row {
                (main, cross_pos)
            } else {
                (cross_pos, main)
            };
            translate_tree(c, x - c.border_box.x, y - c.border_box.y);
            if reverse {
                cursor -= before + main_gap + extra;
            } else {
                cursor += before + sizes[i] + after + main_gap + extra;
            }
        }
        cross_cursor += cross + cross_gap;
        total_main = total_main.max(occupied);
    }
    if !definite_height {
        let height = if is_row {
            (cross_cursor - inner.y - cross_gap).max(0.0)
        } else {
            total_main
        };
        b.border_box.height =
            height + b.border.top + b.border.bottom + b.padding.top + b.padding.bottom;
    }
    update_overflow(b);
}

// ---------------------------------------------------------------------------
// GRID & TABLE LAYOUT BASICS
// ---------------------------------------------------------------------------
