//! Tables.
use crate::*;

pub fn row_paths(b: &LayoutBox, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    for (i, c) in b.children.iter().enumerate() {
        path.push(i);
        if c.box_type == BoxType::TableRow {
            out.push(path.clone());
        } else if matches!(c.box_type, BoxType::Block | BoxType::Inline) {
            row_paths(c, path, out);
        }
        path.pop();
    }
}

pub fn at_path<'a>(b: &'a mut LayoutBox, path: &[usize]) -> &'a mut LayoutBox {
    if let Some((first, rest)) = path.split_first() {
        at_path(&mut b.children[*first], rest)
    } else {
        b
    }
}

pub fn layout_table(b: &mut LayoutBox, cb: Rect, positioned: Rect, ctx: &LayoutContext) {
    let inner = init_container(b, cb, positioned, ctx);
    let spacing = b.style.border_spacing;
    let mut paths = Vec::new();
    row_paths(b, &mut Vec::new(), &mut paths);
    if paths.is_empty() {
        return;
    }
    let count = paths.len();
    let mut places = Vec::new();
    let mut occupied = std::collections::HashSet::new();
    let mut cols = 0;
    for (r, path) in paths.iter().enumerate() {
        let row = at_path(b, path);
        let mut col = 0;
        for (i, cell) in row.children.iter().enumerate() {
            if cell.box_type != BoxType::TableCell {
                continue;
            }
            let cs = cell.table_col_span;
            let rs = cell.table_row_span.min(count - r);
            while (col..col + cs).any(|c| occupied.contains(&(r, c))) && col < 256 {
                col += 1;
            }
            if col + cs > 256 {
                continue;
            }
            for rr in r..r + rs {
                for cc in col..col + cs {
                    occupied.insert((rr, cc));
                }
            }
            places.push((r, i, col, rs, cs));
            cols = cols.max(col + cs);
            col += cs;
        }
    }
    if cols == 0 {
        return;
    }
    let mut mins = vec![0.0_f64; cols];
    let mut maxs = mins.clone();
    // Columns with an authored width keep it; only auto columns absorb space.
    let mut sized = vec![false; cols];
    for (r, i, col, _, cs) in &places {
        let c = &at_path(b, &paths[*r]).children[*i];
        let padding = resolve_insets(
            &c.style.padding,
            inner.width,
            c.style.font_size,
            ctx.font_size_root,
        );
        let extra =
            padding.left + padding.right + c.style.borders.left.width + c.style.borders.right.width;
        let explicit = if c.style.width == Length::Auto {
            0.0
        } else {
            c.style
                .width
                .resolve(inner.width, c.style.font_size, ctx.font_size_root)
        };
        let min = (intrinsic_width(c, true) + extra).max(explicit) / *cs as f64;
        let max = (intrinsic_width(c, false) + extra).max(explicit) / *cs as f64;
        if c.style.width != Length::Auto && *cs == 1 {
            sized[*col] = true;
        }
        for cc in *col..col + cs {
            mins[cc] = mins[cc].max(min);
            maxs[cc] = maxs[cc].max(max);
        }
    }
    let room = (inner.width - spacing * (cols + 1) as f64).max(0.0);
    let min_sum: f64 = mins.iter().sum();
    let max_sum: f64 = maxs.iter().sum();
    let mut widths = mins.clone();
    if room > min_sum {
        if max_sum > room {
            let factor = (room - min_sum) / (max_sum - min_sum).max(0.001);
            for c in 0..cols {
                widths[c] += factor * (maxs[c] - mins[c]);
            }
        } else {
            // Spare width goes to auto columns in proportion to their
            // max-content widths (all columns if every one is sized).
            let extra = room - max_sum;
            let pool: Vec<usize> = (0..cols).filter(|c| !sized[*c]).collect();
            let pool = if pool.is_empty() {
                (0..cols).collect()
            } else {
                pool
            };
            let weight: f64 = pool.iter().map(|c| maxs[*c]).sum();
            widths.clone_from(&maxs);
            for &c in &pool {
                widths[c] += if weight > 0.0 {
                    extra * maxs[c] / weight
                } else {
                    extra / pool.len() as f64
                };
            }
        }
    }
    let mut heights = vec![0.0_f64; count];
    for (r, i, col, rs, cs) in &places {
        let c = &mut at_path(b, &paths[*r]).children[*i];
        let width = widths[*col..col + cs].iter().sum::<f64>() + spacing * (cs - 1) as f64;
        forced_size(
            c,
            Rect { width, ..inner },
            positioned,
            ctx,
            Some(width),
            None,
        );
        let height = (c.border_box.height - spacing * (rs - 1) as f64).max(0.0) / *rs as f64;
        for h in heights.iter_mut().skip(*r).take(*rs) {
            *h = h.max(height);
        }
    }
    for (r, path) in paths.iter().enumerate() {
        let row = at_path(b, path);
        if row.style.height != Length::Auto {
            heights[r] = heights[r].max(row.style.height.resolve(
                inner.height,
                row.style.font_size,
                ctx.font_size_root,
            ));
        }
        row.border_box = Rect {
            x: inner.x + spacing,
            y: inner.y + spacing + heights[..r].iter().sum::<f64>() + spacing * r as f64,
            width: widths.iter().sum::<f64>() + spacing * cols.saturating_sub(1) as f64,
            height: heights[r],
        };
    }
    for (r, i, col, rs, cs) in places {
        let row_y = at_path(b, &paths[r]).border_box.y;
        let c = &mut at_path(b, &paths[r]).children[i];
        let x = inner.x + spacing + widths[..col].iter().sum::<f64>() + spacing * col as f64;
        let w = widths[col..col + cs].iter().sum::<f64>() + spacing * (cs - 1) as f64;
        let h = heights[r..r + rs].iter().sum::<f64>() + spacing * (rs - 1) as f64;
        forced_size(
            c,
            Rect {
                x,
                y: row_y,
                width: w,
                height: h,
            },
            positioned,
            ctx,
            Some(w),
            Some(h),
        );
    }
    // Section boxes enclose their rows, so their backgrounds and geometry agree.
    for section in &mut b.children {
        if section.box_type == BoxType::TableRow || section.children.is_empty() {
            continue;
        }
        let first = section.children.first().unwrap().border_box;
        let last = section.children.last().unwrap().border_box;
        section.border_box = Rect {
            x: inner.x,
            y: first.y,
            width: inner.width,
            height: (last.bottom() - first.y).max(0.0),
        };
    }
    b.border_box.height = heights.iter().sum::<f64>()
        + spacing * (count + 1) as f64
        + b.padding.top
        + b.padding.bottom
        + b.border.top
        + b.border.bottom;
    update_overflow(b);
}

// ---------------------------------------------------------------------------
// HIT TESTING & GEOMETRY QUERIES
// ---------------------------------------------------------------------------
