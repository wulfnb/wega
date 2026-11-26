//! Grid.
use crate::*;

pub(crate) fn track_sizes(
    tracks: &[GridTrack],
    mins: &[f64],
    maxs: &[f64],
    available: Option<f64>,
    gap: f64,
    ctx: &LayoutContext,
    fs: f64,
) -> Vec<f64> {
    let count = mins.len();
    let mut sizes = vec![0.0; count];
    let mut flex = vec![0.0; count];
    let mut autos = Vec::new();
    for i in 0..count {
        let track = tracks
            .get(i)
            .cloned()
            .unwrap_or(GridTrack::Length(Length::Auto));
        let (min, max) = match track {
            GridTrack::MinMax(min, max) => (min, *max),
            other => (Length::Auto, other),
        };
        let minimum = if min == Length::Auto {
            mins[i]
        } else {
            min.resolve(available.unwrap_or(0.0), fs, ctx.font_size_root)
        };
        match max {
            GridTrack::Fraction(fr) => {
                sizes[i] = minimum;
                flex[i] = fr;
            }
            GridTrack::Length(Length::Auto) => {
                sizes[i] = maxs[i].max(minimum);
                autos.push(i);
            }
            GridTrack::Length(length) => {
                sizes[i] = length
                    .resolve(available.unwrap_or(0.0), fs, ctx.font_size_root)
                    .max(if min == Length::Auto { 0.0 } else { minimum })
            }
            GridTrack::MinMax(_, _) => sizes[i] = minimum, // nested minmax is rejected by syntax
            GridTrack::AutoRepeat(_) => sizes[i] = minimum, // expanded before sizing
        }
    }
    if let Some(available) = available {
        let room = (available - gap * count.saturating_sub(1) as f64).max(0.0);
        let minimums = sizes.clone();
        let mut frozen = vec![false; count];
        for _ in 0..=count {
            let fixed: f64 = (0..count)
                .filter(|i| flex[*i] == 0.0 || frozen[*i])
                .map(|i| sizes[i])
                .sum();
            let factor: f64 = (0..count).filter(|i| !frozen[*i]).map(|i| flex[i]).sum();
            if factor == 0.0 {
                break;
            }
            let unit = ((room - fixed) / factor).max(0.0);
            let mut violation = false;
            for i in 0..count {
                if flex[i] > 0.0 && !frozen[i] {
                    if unit * flex[i] < minimums[i] {
                        sizes[i] = minimums[i];
                        frozen[i] = true;
                        violation = true;
                    } else {
                        sizes[i] = unit * flex[i];
                    }
                }
            }
            if !violation {
                break;
            }
        }
        let total: f64 = sizes.iter().sum();
        if total > room && !autos.is_empty() {
            // Auto tracks start at max-content but give back space down to
            // their min-content contribution when the grid would overflow.
            let slack: f64 = autos.iter().map(|&i| (sizes[i] - mins[i]).max(0.0)).sum();
            if slack > 0.0 {
                let shrink = ((total - room) / slack).min(1.0);
                for &i in &autos {
                    sizes[i] -= (sizes[i] - mins[i]).max(0.0) * shrink;
                }
            }
        } else if flex.iter().all(|f| *f == 0.0) && !autos.is_empty() {
            let extra = ((room - total) / autos.len() as f64).max(0.0);
            for i in autos {
                sizes[i] += extra;
            }
        }
    }
    sizes
}

/// Expand `repeat(auto-fill, …)` to as many repetitions as fit `available`.
pub(crate) fn expand_auto_repeat(
    tracks: &[GridTrack],
    available: f64,
    gap: f64,
    fs: f64,
    rfs: f64,
) -> Vec<GridTrack> {
    let size = |t: &GridTrack| match t {
        GridTrack::Length(l) | GridTrack::MinMax(l, _) if *l != Length::Auto => {
            l.resolve(available, fs, rfs).max(0.0)
        }
        GridTrack::MinMax(_, max) => match **max {
            GridTrack::Length(l) if l != Length::Auto => l.resolve(available, fs, rfs).max(0.0),
            _ => 0.0,
        },
        _ => 0.0,
    };
    let fixed: f64 = tracks
        .iter()
        .filter(|t| !matches!(t, GridTrack::AutoRepeat(_)))
        .map(|t| size(t) + gap)
        .sum();
    let mut out = Vec::new();
    for t in tracks {
        if let GridTrack::AutoRepeat(items) = t {
            let unit: f64 = items.iter().map(|t| size(t) + gap).sum();
            let count = if unit > gap * items.len() as f64 {
                (((available - fixed + gap) / unit).floor() as usize).clamp(1, 256)
            } else {
                1
            };
            for _ in 0..count {
                out.extend(items.iter().cloned());
            }
        } else {
            out.push(t.clone());
        }
    }
    out.truncate(256);
    out
}

/// Turn named areas and end lines into the start/span form placement uses.
pub(crate) fn resolve_grid_item(
    c: &mut LayoutBox,
    areas: &[Vec<String>],
    cols: usize,
    rows: usize,
) {
    let find = |name: &str| {
        let mut bounds: Option<(usize, usize, usize, usize)> = None;
        for (r, row) in areas.iter().enumerate() {
            for (col, cell) in row.iter().enumerate() {
                if cell == name {
                    let b = bounds.get_or_insert((r, r, col, col));
                    *b = (b.0.min(r), b.1.max(r), b.2.min(col), b.3.max(col));
                }
            }
        }
        bounds
    };
    let style = &mut c.style;
    if let Some(name) = style.grid_row_name.clone()
        && let Some((r0, r1, _, _)) = find(&name)
    {
        style.grid_row_start = Some(r0 as i32 + 1);
        style.grid_row_span = r1 - r0 + 1;
    }
    if let Some(name) = style.grid_column_name.clone()
        && let Some((_, _, c0, c1)) = find(&name)
    {
        style.grid_column_start = Some(c0 as i32 + 1);
        style.grid_column_span = c1 - c0 + 1;
    }
    let line = |n: i32, count: usize| if n < 0 { count as i32 + 2 + n } else { n };
    if let Some(end) = style.grid_column_end {
        let end = line(end, cols);
        match style.grid_column_start {
            Some(start) => {
                let start = line(start, cols);
                let (a, b) = (start.min(end), start.max(end));
                style.grid_column_start = Some(a.max(1));
                style.grid_column_span = ((b - a).max(1) as usize).min(256);
            }
            None => {
                style.grid_column_start = Some((end - style.grid_column_span as i32).max(1));
            }
        }
    } else if let Some(start) = style.grid_column_start {
        style.grid_column_start = Some(line(start, cols).max(1));
    }
    if let Some(end) = style.grid_row_end {
        let end = line(end, rows);
        match style.grid_row_start {
            Some(start) => {
                let start = line(start, rows);
                let (a, b) = (start.min(end), start.max(end));
                style.grid_row_start = Some(a.max(1));
                style.grid_row_span = ((b - a).max(1) as usize).min(256);
            }
            None => {
                style.grid_row_start = Some((end - style.grid_row_span as i32).max(1));
            }
        }
    } else if let Some(start) = style.grid_row_start {
        style.grid_row_start = Some(line(start, rows).max(1));
    }
}

pub(crate) fn layout_grid(b: &mut LayoutBox, cb: Rect, positioned: Rect, ctx: &LayoutContext) {
    let inner = init_container(b, cb, positioned, ctx);
    let (row_gap, column_gap) = b.style.gaps(
        inner.width,
        (b.style.height != Length::Auto).then_some(inner.height),
    );
    let positioned = if b.style.position != Position::Static {
        b.padding_box()
    } else {
        positioned
    };
    let columns = expand_auto_repeat(
        &b.style.grid_template_columns,
        inner.width,
        column_gap,
        b.style.font_size,
        ctx.font_size_root,
    );
    let areas = b.style.grid_template_areas.clone();
    let template_cols = columns.len().max(areas.first().map_or(0, Vec::len));
    let template_rows = b.style.grid_template_rows.len().max(areas.len());
    for c in &mut b.children {
        resolve_grid_item(c, &areas, template_cols, template_rows);
    }
    let cols = template_cols
        .max(
            b.children
                .iter()
                .map(|c| {
                    c.style.grid_column_start.unwrap_or(1).max(1) as usize
                        + c.style.grid_column_span
                        - 1
                })
                .max()
                .unwrap_or(1),
        )
        .clamp(1, 256);
    let mut occupied = std::collections::HashSet::new();
    let mut cursor = 0;
    let mut places = Vec::new();
    let mut rows = template_rows.max(1);
    for (i, c) in b.children.iter_mut().enumerate() {
        if matches!(c.style.position, Position::Absolute | Position::Fixed) {
            layout_box(c, inner, positioned, ctx);
            continue;
        }
        let cs = c.style.grid_column_span.min(cols);
        let rs = c.style.grid_row_span.clamp(1, 256);
        let explicit_col = c
            .style
            .grid_column_start
            .map(|c| (c.max(1) as usize - 1).min(cols - cs));
        let explicit_row = c
            .style
            .grid_row_start
            .map(|r| (r.max(1) as usize - 1).min(4095));
        let mut row = explicit_row.unwrap_or(cursor / cols);
        let mut col = explicit_col.unwrap_or(cursor % cols).min(cols - cs);
        for _ in 0..4096 * cols {
            let free =
                (row..row + rs).all(|r| (col..col + cs).all(|c| !occupied.contains(&(r, c))));
            if free || (explicit_col.is_some() && explicit_row.is_some()) {
                break;
            }
            if explicit_col.is_some() {
                row += 1;
            } else {
                col += 1;
                if col + cs > cols {
                    col = 0;
                    row += 1;
                }
            }
            if row >= 4096 {
                break;
            }
        }
        if row >= 4096 {
            continue;
        }
        for r in row..row + rs {
            for c in col..col + cs {
                occupied.insert((r, c));
            }
        }
        if explicit_row.is_none() {
            cursor = row * cols + col + cs;
        }
        rows = rows.max(row + rs);
        places.push((i, row, col, rs, cs));
    }
    let mut mins = vec![0.0_f64; cols];
    let mut maxs = mins.clone();
    for (i, _, col, _, span) in &places {
        let c = &b.children[*i];
        let min = intrinsic_width(c, true) / *span as f64;
        let max = intrinsic_width(c, false) / *span as f64;
        for j in *col..col + span {
            mins[j] = mins[j].max(min);
            maxs[j] = maxs[j].max(max);
        }
    }
    let widths = track_sizes(
        &columns,
        &mins,
        &maxs,
        Some(inner.width),
        column_gap,
        ctx,
        b.style.font_size,
    );
    let mut heights = vec![0.0_f64; rows];
    for (i, row, col, rs, cs) in &places {
        let width = widths[*col..col + cs].iter().sum::<f64>() + column_gap * (cs - 1) as f64;
        let c = &mut b.children[*i];
        forced_size(
            c,
            Rect { width, ..inner },
            positioned,
            ctx,
            Some(width),
            None,
        );
        let height = (c.border_box.height + c.margin.top + c.margin.bottom
            - row_gap * (rs - 1) as f64)
            .max(0.0)
            / *rs as f64;
        for h in heights.iter_mut().skip(*row).take(*rs) {
            *h = h.max(height);
        }
    }
    let row_sizes = track_sizes(
        &b.style.grid_template_rows,
        &heights,
        &heights,
        if b.style.height == Length::Auto {
            None
        } else {
            Some(inner.height)
        },
        row_gap,
        ctx,
        b.style.font_size,
    );
    for (i, row, col, rs, cs) in places {
        let x = inner.x + widths[..col].iter().sum::<f64>() + column_gap * col as f64;
        let y = inner.y + row_sizes[..row].iter().sum::<f64>() + row_gap * row as f64;
        let w = widths[col..col + cs].iter().sum::<f64>() + column_gap * (cs - 1) as f64;
        let h = row_sizes[row..row + rs].iter().sum::<f64>() + row_gap * (rs - 1) as f64;
        let c = &mut b.children[i];
        let stretch_w = c.style.width == Length::Auto;
        let stretch_h = c.style.height == Length::Auto;
        forced_size(
            c,
            Rect {
                x,
                y,
                width: w,
                height: h,
            },
            positioned,
            ctx,
            stretch_w.then_some((w - c.margin.left - c.margin.right).max(0.0)),
            stretch_h.then_some((h - c.margin.top - c.margin.bottom).max(0.0)),
        );
    }
    if b.style.height == Length::Auto {
        b.border_box.height = row_sizes.iter().sum::<f64>()
            + row_gap * rows.saturating_sub(1) as f64
            + b.border.top
            + b.border.bottom
            + b.padding.top
            + b.padding.bottom;
    }
    update_overflow(b);
}
