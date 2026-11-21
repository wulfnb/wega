//! Stacking contexts, paint order and backgrounds.
use crate::*;

pub(crate) struct Entry<'a> {
    b: &'a LayoutBox,
    path: Vec<&'a LayoutBox>,
    z: i32,
    is_context: bool,
}

pub(crate) fn collect<'a>(
    b: &'a LayoutBox,
    path: &mut Vec<&'a LayoutBox>,
    entries: &mut Vec<Entry<'a>>,
) {
    for child in &b.children {
        let is_context = context(child, matches!(b.box_type, BoxType::Flex | BoxType::Grid));
        if is_context || child.style.position != Position::Static {
            entries.push(Entry {
                b: child,
                path: path.clone(),
                z: if is_context {
                    child.style.z_index.unwrap_or(0)
                } else {
                    0
                },
                is_context,
            });
        }
        if !is_context {
            path.push(child);
            collect(child, path, entries);
            path.pop();
        }
    }
}

pub(crate) fn flow(
    b: &LayoutBox,
    list: &mut DisplayList,
    embedded: &[(NodeKey, anyrender::Scene)],
    s: f64,
    scroll: Point,
) {
    decorations(b, list, s);
    push_contents(b, list, s);
    let scroll = Point {
        x: scroll.x + b.scroll_offset.x,
        y: scroll.y + b.scroll_offset.y,
    };
    for c in &b.children {
        if !context(c, matches!(b.box_type, BoxType::Flex | BoxType::Grid))
            && c.style.position == Position::Static
        {
            flow(c, list, embedded, s, scroll);
        }
    }
    content(b, list, embedded, s);
    pop_contents(b, list);
}

pub(crate) fn paint_entry(
    entry: &Entry<'_>,
    list: &mut DisplayList,
    embedded: &[(NodeKey, anyrender::Scene)],
    s: f64,
    mut scroll: Point,
) {
    // CSS 2.1 §11.1.1: an ancestor's overflow clips (and scrolls) a descendant
    // only when it is in the descendant's containing-block chain. Fixed boxes
    // escape every ancestor here; absolute boxes escape the ones between them
    // and their nearest positioned ancestor.
    let chain = match entry.b.style.position {
        Position::Fixed => 0,
        Position::Absolute => entry
            .path
            .iter()
            .rposition(|a| a.style.position != Position::Static || a.style.transform.is_some())
            .map_or(0, |i| i + 1),
        _ => entry.path.len(),
    };
    let ancestors = &entry.path[..chain];
    for ancestor in ancestors {
        push_contents(ancestor, list, s);
        scroll.x += ancestor.scroll_offset.x;
        scroll.y += ancestor.scroll_offset.y;
    }
    if entry.is_context {
        paint_context(entry.b, list, embedded, s, scroll);
    } else {
        flow(entry.b, list, embedded, s, scroll);
    }
    for ancestor in ancestors.iter().rev() {
        pop_contents(ancestor, list);
    }
}

pub(crate) fn paint_context(
    b: &LayoutBox,
    list: &mut DisplayList,
    embedded: &[(NodeKey, anyrender::Scene)],
    s: f64,
    scroll: Point,
) {
    let fixed = b.style.position == Position::Fixed;
    if fixed {
        list.items.push(DisplayItem::PushTransform {
            transform: Affine::translate((scroll.x * s, scroll.y * s)),
        });
    }
    let transform = b.style.transform.map(|t| {
        let r = b.border_box;
        let ox = r.x
            + b.style
                .transform_origin
                .0
                .resolve(r.width, b.style.font_size, 16.0);
        let oy = r.y
            + b.style
                .transform_origin
                .1
                .resolve(r.height, b.style.font_size, 16.0);
        Affine::translate((ox * s, oy * s))
            * Affine::scale(s)
            * t.to_kurbo()
            * Affine::scale(1.0 / s)
            * Affine::translate((-ox * s, -oy * s))
    });
    if let Some(transform) = transform {
        list.items.push(DisplayItem::PushTransform { transform });
    }
    if b.style.opacity < 1.0 {
        list.items.push(DisplayItem::PushOpacity {
            opacity: b.style.opacity,
        });
    }
    if let Some(clip) = &b.style.clip_path {
        list.items.push(DisplayItem::PushPathClip {
            path: Affine::scale(s)
                * clip.path(
                    kurbo::Rect::new(
                        b.border_box.x,
                        b.border_box.y,
                        b.border_box.right(),
                        b.border_box.bottom(),
                    ),
                    b.style.font_size,
                ),
        });
    }
    decorations(b, list, s);
    push_contents(b, list, s);
    let scroll = Point {
        x: if fixed {
            b.scroll_offset.x
        } else {
            scroll.x + b.scroll_offset.x
        },
        y: if fixed {
            b.scroll_offset.y
        } else {
            scroll.y + b.scroll_offset.y
        },
    };
    let mut entries = Vec::new();
    collect(b, &mut Vec::new(), &mut entries);
    entries.sort_by_key(|e| e.z); // stable source order for equal levels
    for e in entries.iter().filter(|e| e.z < 0) {
        paint_entry(e, list, embedded, s, scroll);
    }
    for child in &b.children {
        if !context(child, matches!(b.box_type, BoxType::Flex | BoxType::Grid))
            && child.style.position == Position::Static
        {
            flow(child, list, embedded, s, scroll);
        }
    }
    content(b, list, embedded, s);
    for e in entries.iter().filter(|e| e.z >= 0) {
        paint_entry(e, list, embedded, s, scroll);
    }
    pop_contents(b, list);
    if b.style.opacity < 1.0 {
        list.items.push(DisplayItem::PopOpacity);
    }
    if b.style.clip_path.is_some() {
        list.items.push(DisplayItem::PopClip);
    }
    if transform.is_some() {
        list.items.push(DisplayItem::PopTransform);
    }
    if fixed {
        list.items.push(DisplayItem::PopTransform);
    }
}

pub(crate) fn background(b: &LayoutBox, list: &mut DisplayList, s: f64) {
    let size = b
        .background_image
        .as_ref()
        .map(|i| (i.width as f64, i.height as f64))
        .or_else(|| {
            b.background_svg
                .as_ref()
                .map(|t| (t.tree.size().width() as f64, t.tree.size().height() as f64))
        });
    let Some((iw, ih)) = size else {
        return;
    };
    let r = b.padding_box();
    let (w, h) = b
        .style
        .background_size
        .resolve(r.width, r.height, iw, ih, b.style.font_size);
    if w <= 0.0 || h <= 0.0 || !w.is_finite() || !h.is_finite() {
        return;
    }
    let x = b
        .style
        .background_position
        .0
        .resolve(r.width - w, b.style.font_size, 16.0);
    let y = b
        .style
        .background_position
        .1
        .resolve(r.height - h, b.style.font_size, 16.0);
    let (rx, ry) = b.style.background_repeat;
    let x = if rx { x.rem_euclid(w) - w } else { x };
    let y = if ry { y.rem_euclid(h) - h } else { y };
    let nx = if rx {
        ((r.width - x) / w).ceil().min(256.0) as usize
    } else {
        1
    };
    let ny = if ry {
        ((r.height - y) / h).ceil().min(256.0) as usize
    } else {
        1
    };
    list.items.push(DisplayItem::PushClip {
        rect: scaled(b.border_box, s),
        border_radius: b.radius() * s,
    });
    let mut budget = 4096;
    for row in 0..ny {
        for col in 0..nx {
            if budget == 0 {
                break;
            }
            budget -= 1;
            let rect = scaled(
                Rect {
                    x: r.x + x + col as f64 * w,
                    y: r.y + y + row as f64 * h,
                    width: w,
                    height: h,
                },
                s,
            );
            if let Some(image) = &b.background_image {
                list.items.push(DisplayItem::Image {
                    rect,
                    image: image.clone(),
                });
            }
            if let Some(tree) = &b.background_svg {
                list.items.push(DisplayItem::Svg {
                    rect,
                    tree: tree.clone(),
                });
            }
        }
    }
    list.items.push(DisplayItem::PopClip);
}
