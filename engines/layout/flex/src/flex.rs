//! Flexbox.
use crate::*;

/// One flex line after its items are flexed and measured, before cross placement.
struct FlexLine {
    items: Vec<usize>,
    available: f64,
    occupied: f64,
    /// Main-axis offset of the first item and the extra space between items
    /// (`justify-content`).
    offset: f64,
    extra: f64,
    /// The line's cross size, after `align-content: stretch`.
    cross: f64,
}

pub fn layout_flex(b: &mut LayoutBox, cb: Rect, positioned: Rect, ctx: &LayoutContext) {
    let inner = init_container(b, cb, positioned, ctx);
    // `row` follows the inline axis and `column` the block axis (CSS Flexbox 1 §5.1). In a
    // vertical writing mode the inline axis is vertical: top to bottom, or bottom to top in
    // `sideways-lr`, and `direction: rtl` reverses it. Its block axis runs right to left in
    // `vertical-rl`/`sideways-rl` and left to right in `vertical-lr`/`sideways-lr`.
    let wm = b.style.writing_mode;
    let rtl = b.style.direction_rtl;
    let row_like = matches!(
        b.style.flex_direction,
        FlexDirection::Row | FlexDirection::RowReverse
    );
    let inline_reversed = if wm.is_vertical() {
        wm.inline_bottom_to_top() != rtl
    } else {
        rtl
    };
    let block_reversed = matches!(wm, WritingMode::VerticalRl | WritingMode::SidewaysRl);
    // `is_row`: the main axis is physically horizontal.
    let is_row = row_like != wm.is_vertical();
    let reverse = matches!(
        b.style.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    ) != if row_like {
        inline_reversed
    } else {
        block_reversed
    };
    // Cross-start is at the physical right or bottom; `wrap-reverse` swaps the cross edges.
    let cross_reversed = if row_like {
        block_reversed
    } else {
        inline_reversed
    } != b.style.flex_wrap_reverse;
    let definite_height = b.style.height != Length::Auto;
    // In a vertical writing mode the width is the block size: when it is `auto` it fits the
    // content, so the cross size of a vertical main axis is not known until lines are laid
    // out (CSS Writing Modes 4 §7.3).
    let content_width = wm.is_vertical() && !is_row && b.style.width == Length::Auto;
    let main_available = if is_row {
        inner.width
    } else if definite_height {
        inner.height
    } else {
        f64::INFINITY
    };
    // `column-gap` separates items along a `row`-like main axis and `row-gap` separates its
    // lines, whatever the writing mode (CSS Box Alignment 3 §8.1).
    let (row_gap, column_gap) = b
        .style
        .gaps(inner.width, definite_height.then_some(inner.height));
    let (main_gap, cross_gap) = if row_like {
        (column_gap, row_gap)
    } else {
        (row_gap, column_gap)
    };
    let positioned = if b.style.position != Position::Static {
        b.padding_box()
    } else {
        positioned
    };
    let mut bases = vec![0.0; b.children.len()];
    let mut outers: Vec<(usize, f64)> = Vec::new();
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
        outers.push((i, outer));
    }
    let line_items: Vec<Vec<usize>> = if !b.style.flex_wrap {
        vec![outers.iter().map(|(i, _)| *i).collect()]
    } else {
        let sizes: Vec<f64> = outers.iter().map(|(_, o)| *o).collect();
        let counts = if b.style.flex_balance {
            balanced_line_counts(&sizes, main_available, main_gap)
        } else {
            greedy_line_counts(&sizes, main_available, main_gap)
        };
        let mut items = outers.iter().map(|(i, _)| *i);
        counts
            .into_iter()
            .map(|n| items.by_ref().take(n).collect())
            .collect()
    };

    // Flex and measure each line.
    let mut sizes = bases.clone();
    let mut frozen = vec![false; b.children.len()];
    let mut lines: Vec<FlexLine> = Vec::new();
    for line in line_items {
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
        // A single-line container's line is as large as the container's cross size.
        let mut cross: f64 = if !b.style.flex_wrap {
            if is_row && definite_height {
                inner.height
            } else if !is_row && !content_width {
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
        lines.push(FlexLine {
            items: line,
            available,
            occupied,
            offset,
            extra,
            cross,
        });
    }

    // `align-content` distributes the container's free cross space among the lines of a
    // multi-line container (CSS Flexbox 1 §8.4); `normal` behaves as `stretch`.
    let n = lines.len() as f64;
    let lines_cross: f64 =
        lines.iter().map(|l| l.cross).sum::<f64>() + cross_gap * (n - 1.0).max(0.0);
    let definite_cross = if is_row {
        definite_height.then_some(inner.height)
    } else {
        (!content_width).then_some(inner.width)
    };
    let (mut lead, mut between) = (0.0, 0.0);
    if b.style.flex_wrap
        && let Some(container_cross) = definite_cross
        && n > 0.0
    {
        let free = container_cross - lines_cross;
        match b.style.align_content {
            AlignContent::Normal | AlignContent::Stretch if free > 0.0 => {
                for line in &mut lines {
                    line.cross += free / n;
                }
            }
            AlignContent::FlexEnd => lead = free,
            AlignContent::Center => lead = free / 2.0,
            AlignContent::SpaceBetween if free > 0.0 && n > 1.0 => between = free / (n - 1.0),
            AlignContent::SpaceAround if free > 0.0 => {
                between = free / n;
                lead = between / 2.0;
            }
            AlignContent::SpaceEvenly if free > 0.0 => {
                between = free / (n + 1.0);
                lead = between;
            }
            // Negative free space centers `space-around` and `space-evenly` lines.
            AlignContent::SpaceAround | AlignContent::SpaceEvenly => lead = free / 2.0,
            _ => {}
        }
    }
    let total_cross = definite_cross
        .filter(|_| b.style.flex_wrap)
        .unwrap_or(lines.iter().map(|l| l.cross).sum::<f64>() + cross_gap * (n - 1.0).max(0.0));
    let cross_origin = if is_row { inner.y } else { inner.x };

    // Place the items.
    let mut cross_cursor = lead;
    let mut total_main: f64 = 0.0;
    for line in lines {
        let cross = line.cross;
        let origin = if is_row { inner.x } else { inner.y };
        let mut cursor = if reverse {
            origin + line.available - line.offset
        } else {
            origin + line.offset
        };
        for i in line.items {
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
            let (before, after) = if is_row {
                (c.margin.left, c.margin.right)
            } else {
                (c.margin.top, c.margin.bottom)
            };
            // Margins on the cross-start and cross-end sides.
            let (mut cross_before, mut cross_after) = if is_row {
                (c.margin.top, c.margin.bottom)
            } else {
                (c.margin.left, c.margin.right)
            };
            if cross_reversed {
                std::mem::swap(&mut cross_before, &mut cross_after);
            }
            let cross_size = if is_row {
                c.border_box.height
            } else {
                c.border_box.width
            };
            let extra_cross = (cross - cross_size - cross_before - cross_after).max(0.0);
            // Distance from the container's cross-start edge to the item's cross-start edge.
            let logical = cross_cursor
                + cross_before
                + match b.style.align_items {
                    AlignItems::Center => extra_cross / 2.0,
                    AlignItems::FlexEnd => extra_cross,
                    _ => 0.0,
                };
            let cross_pos = if cross_reversed {
                cross_origin + total_cross - logical - cross_size
            } else {
                cross_origin + logical
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
                cursor -= before + main_gap + line.extra;
            } else {
                cursor += before + sizes[i] + after + main_gap + line.extra;
            }
        }
        cross_cursor += cross + cross_gap + between;
        total_main = total_main.max(line.occupied);
    }
    if !definite_height {
        let height = if is_row { total_cross } else { total_main };
        b.border_box.height =
            height + b.border.top + b.border.bottom + b.padding.top + b.padding.bottom;
    }
    if content_width {
        b.border_box.width =
            total_cross + b.border.left + b.border.right + b.padding.left + b.padding.right;
    }
    update_overflow(b);
}

/// Greedy line breaking: each line takes items until the next one no longer fits; an item
/// that does not fit on its own takes a line by itself.
fn greedy_line_counts(sizes: &[f64], limit: f64, gap: f64) -> Vec<usize> {
    let mut counts = Vec::new();
    let (mut count, mut used) = (0usize, 0.0);
    for size in sizes {
        if count > 0 && used + gap + size > limit {
            counts.push(count);
            (count, used) = (0, 0.0);
        }
        if count > 0 {
            used += gap;
        }
        used += size;
        count += 1;
    }
    if count > 0 || counts.is_empty() {
        counts.push(count);
    }
    counts
}

/// Balanced line breaking for `flex-wrap: balance` (CSS Flexbox 2 §9.4.1), after Taffy's
/// implementation: keep the greedy line count, but choose the division into that many
/// contiguous lines that minimizes the sum of squared line sizes. No line may exceed `limit`
/// unless it holds a single item. Ties give the most items to the first line, then the
/// second, and so on. (Taffy's zero-sized-item rule is not applied.)
fn balanced_line_counts(sizes: &[f64], limit: f64, gap: f64) -> Vec<usize> {
    let greedy = greedy_line_counts(sizes, limit, gap);
    let (n, lines) = (sizes.len(), greedy.len());
    // The dynamic program is O(lines * n²); very long lines keep the greedy division.
    if lines <= 1 || lines == n || n > 2048 {
        return greedy;
    }
    let mut prefix = vec![0.0; n + 1];
    for (i, size) in sizes.iter().enumerate() {
        prefix[i + 1] = prefix[i] + size.max(0.0);
    }
    let line_cost = |start: usize, end: usize| {
        let size = prefix[end + 1] - prefix[start] + gap * (end - start) as f64;
        if end > start && size > limit {
            f64::INFINITY
        } else {
            size * size
        }
    };
    // best[k][start]: the least cost of dividing items `start..` into `k + 1` lines, with the
    // chosen end of the first of those lines.
    let mut best = vec![vec![(f64::INFINITY, 0usize); n]; lines];
    for (start, entry) in best[0].iter_mut().enumerate() {
        *entry = (line_cost(start, n - 1), n - 1);
    }
    for k in 1..lines {
        for start in 0..n {
            // Leave at least one item for each of the remaining `k` lines; scanning ends
            // from the largest keeps the largest first line on ties.
            let Some(max_end) = (n - 1).checked_sub(k) else {
                continue;
            };
            for end in (start..=max_end).rev() {
                let cost = line_cost(start, end) + best[k - 1][end + 1].0;
                if cost < best[k][start].0 {
                    best[k][start] = (cost, end);
                }
            }
        }
    }
    if !best[lines - 1][0].0.is_finite() {
        return greedy;
    }
    let mut counts = Vec::with_capacity(lines);
    let mut start = 0;
    for k in (0..lines).rev() {
        let end = best[k][start].1;
        counts.push(end - start + 1);
        start = end + 1;
    }
    counts
}

#[cfg(test)]
mod line_tests {
    use super::*;

    #[test]
    fn balanced_lines_keep_the_greedy_count_and_even_out_sizes() {
        // Four 25px items, 10px gaps, 100px column: greedy 3 + 1, balanced 2 + 2.
        assert_eq!(greedy_line_counts(&[25.0; 4], 100.0, 10.0), [3, 1]);
        assert_eq!(balanced_line_counts(&[25.0; 4], 100.0, 10.0), [2, 2]);
        // Ties put the most items in the first line.
        assert_eq!(balanced_line_counts(&[10.0; 5], 30.0, 0.0), [3, 2]);
        // An item wider than the line stays alone.
        assert_eq!(
            greedy_line_counts(&[50.0, 200.0, 50.0], 100.0, 0.0),
            [1, 1, 1]
        );
        assert_eq!(balanced_line_counts(&[], 100.0, 0.0), [0]);
    }
}

// ---------------------------------------------------------------------------
// GRID & TABLE LAYOUT BASICS
// ---------------------------------------------------------------------------
