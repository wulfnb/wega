//! Paint one box: decorations and contents.
use crate::*;

pub(crate) fn context(b: &LayoutBox, parent_flex: bool) -> bool {
    b.style.clip_path.is_some()
        || b.style.transform.is_some()
        || b.style.opacity < 1.0
        || (b.style.z_index.is_some() && (b.style.position != Position::Static || parent_flex))
        || b.style.position == Position::Fixed
}

pub(crate) fn clipped(b: &LayoutBox) -> bool {
    b.style.overflow_x != Overflow::Visible || b.style.overflow_y != Overflow::Visible
}

pub(crate) fn push_contents(b: &LayoutBox, list: &mut DisplayList, s: f64) {
    if clipped(b) {
        list.items.push(DisplayItem::PushClip {
            rect: scaled(b.padding_box(), s),
            // The padding box's corners: each outer radius less its adjacent borders.
            border_radius: {
                let [tl, tr, br, bl] = b.radii();
                let (t, r, bo, l) = (b.border.top, b.border.right, b.border.bottom, b.border.left);
                kurbo::RoundedRectRadii::new(
                    (tl - l.max(t)).max(0.0) * s,
                    (tr - r.max(t)).max(0.0) * s,
                    (br - r.max(bo)).max(0.0) * s,
                    (bl - l.max(bo)).max(0.0) * s,
                )
            },
        });
    }
    list.items.push(DisplayItem::PushTransform {
        transform: Affine::translate((-b.scroll_offset.x * s, -b.scroll_offset.y * s)),
    });
}

pub(crate) fn pop_contents(b: &LayoutBox, list: &mut DisplayList) {
    list.items.push(DisplayItem::PopTransform);
    if clipped(b) {
        list.items.push(DisplayItem::PopClip);
    }
}

pub(crate) fn decorations(b: &LayoutBox, list: &mut DisplayList, s: f64) {
    if b.box_type == BoxType::Text {
        return;
    }
    let rects = if b.fragments.is_empty() {
        std::slice::from_ref(&b.border_box)
    } else {
        &b.fragments
    };
    for rect in rects {
        let rect = scaled(*rect, s);
        let radius = radii(b, s);
        if let Some(k) = b.node {
            list.items.push(DisplayItem::HitRegion {
                node: k,
                enabled: b.style.visible && b.style.pointer_events,
                rect,
                border_radius: radius,
            });
        }
        if !b.style.visible {
            continue;
        }
        if let Some(shadow) = b.style.box_shadow {
            list.items.push(DisplayItem::BoxShadow {
                rect: Rect {
                    x: rect.x - shadow.spread * s,
                    y: rect.y - shadow.spread * s,
                    width: (rect.width + 2.0 * shadow.spread * s).max(0.0),
                    height: (rect.height + 2.0 * shadow.spread * s).max(0.0),
                },
                offset_x: shadow.offset_x * s,
                offset_y: shadow.offset_y * s,
                blur_radius: shadow.blur_radius * s,
                color: shadow.color,
                border_radius: radius,
                clip_out: rect,
            });
        }
        if b.style.background_color.a > 0 {
            list.items.push(DisplayItem::FillRect {
                rect,
                color: b.style.background_color,
                border_radius: radius,
            });
        }
        if let Some(gradient) = &b.style.background_gradient {
            // CSS gradient line: through the center at `angle`, long enough
            // that the corners reach the 0% and 100% stops.
            let a = (gradient.angle as f64).to_radians();
            let (dx, dy) = (a.sin(), -a.cos());
            let half = (rect.width * dx.abs() + rect.height * dy.abs()) / 2.0;
            let (cx, cy) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            list.items.push(DisplayItem::LinearGradient {
                rect,
                start: (cx - dx * half, cy - dy * half),
                end: (cx + dx * half, cy + dy * half),
                stops: gradient.resolved_stops(),
                border_radius: radius,
            });
        }
        background(b, list, s);
        let border = b.style.borders;
        if border.top == border.right && border.top == border.bottom && border.top == border.left {
            if border.top.width > 0.0 {
                list.items.push(DisplayItem::StrokeRect {
                    rect,
                    color: border.top.color,
                    width: border.top.width * s,
                    border_radius: radius,
                });
            }
        } else {
            list.items.push(DisplayItem::PushClip {
                rect,
                border_radius: radius,
            });
            let (x, y, r, bot) = (rect.x, rect.y, rect.right(), rect.bottom());
            let (l, t, rr, bb) = (
                border.left.width * s,
                border.top.width * s,
                border.right.width * s,
                border.bottom.width * s,
            );
            for (side, points) in [
                (
                    border.top,
                    [(x, y), (r, y), (r - rr, y + t), (x + l, y + t)],
                ),
                (
                    border.right,
                    [(r, y), (r, bot), (r - rr, bot - bb), (r - rr, y + t)],
                ),
                (
                    border.bottom,
                    [(r, bot), (x, bot), (x + l, bot - bb), (r - rr, bot - bb)],
                ),
                (
                    border.left,
                    [(x, bot), (x, y), (x + l, y + t), (x + l, bot - bb)],
                ),
            ] {
                if side.width == 0.0 {
                    continue;
                }
                let mut path = kurbo::BezPath::new();
                path.move_to(points[0]);
                for p in &points[1..] {
                    path.line_to(*p);
                }
                path.close_path();
                list.items.push(DisplayItem::FillPath {
                    path,
                    color: side.color,
                });
            }
            list.items.push(DisplayItem::PopClip);
        }
    }
}

pub(crate) fn content(
    b: &LayoutBox,
    list: &mut DisplayList,
    embedded: &[(NodeKey, anyrender::Scene)],
    s: f64,
) {
    if let Some((_, scene)) = embedded
        .iter()
        .find(|(k, _)| Some(*k) == b.node)
        .filter(|_| b.style.visible)
    {
        list.items.push(DisplayItem::EmbeddedScene {
            rect: scaled(b.content_box(), s),
            scene: scene.clone(),
        });
    }
    let c = b.content_box();
    if let Some(tree) = b.svg.as_ref().filter(|_| b.style.visible) {
        list.items.push(DisplayItem::Svg {
            rect: scaled(c, s),
            tree: tree.clone(),
        });
    }
    if let Some(image) = b.image.as_ref().filter(|_| b.style.visible) {
        let iw = image.width as f64;
        let ih = image.height as f64;
        let (dest_w, dest_h) = if iw > 0.0 && ih > 0.0 {
            let ar = iw / ih;
            match b.style.object_fit {
                ObjectFit::Fill => (c.width, c.height),
                ObjectFit::Contain => {
                    if c.height <= 0.0 || (c.width / c.height) > ar {
                        (c.height * ar, c.height)
                    } else {
                        (c.width, c.width / ar)
                    }
                }
                ObjectFit::Cover => {
                    if c.height <= 0.0 || (c.width / c.height) > ar {
                        (c.width, c.width / ar)
                    } else {
                        (c.height * ar, c.height)
                    }
                }
                ObjectFit::None => (iw, ih),
                ObjectFit::ScaleDown => {
                    let contain_w = if c.height <= 0.0 || (c.width / c.height) > ar {
                        c.height * ar
                    } else {
                        c.width
                    };
                    let contain_h = if c.height <= 0.0 || (c.width / c.height) > ar {
                        c.height
                    } else {
                        c.width / ar
                    };
                    if iw <= contain_w && ih <= contain_h {
                        (iw, ih)
                    } else {
                        (contain_w, contain_h)
                    }
                }
            }
        } else {
            (c.width, c.height)
        };

        let resolve_pos = |len: &Length, total: f64| match len {
            Length::Percent(p) => total * (p / 100.0),
            Length::Px(px) => *px,
            Length::Em(em) => em * b.style.font_size,
            Length::Rem(rem) => rem * 16.0,
            _ => total * 0.5,
        };
        let off_x = resolve_pos(&b.style.object_position.0, c.width - dest_w);
        let off_y = resolve_pos(&b.style.object_position.1, c.height - dest_h);

        let dest_rect = Rect {
            x: c.x + off_x,
            y: c.y + off_y,
            width: dest_w,
            height: dest_h,
        };

        let needs_clip = b.style.object_fit == ObjectFit::Cover
            || b.style.overflow_x != Overflow::Visible
            || b.style.overflow_y != Overflow::Visible;

        if needs_clip {
            list.items.push(DisplayItem::PushClip {
                rect: scaled(c, s),
                border_radius: radii(b, s),
            });
        }
        list.items.push(DisplayItem::Image {
            rect: scaled(dest_rect, s),
            image: image.clone(),
        });
        if needs_clip {
            list.items.push(DisplayItem::PopClip);
        }
    }
    if let Some(ref form) = b.form_state {
        for sel in &form.selection_rects {
            list.items.push(DisplayItem::FillRect {
                rect: scaled(*sel, s),
                color: Color {
                    r: 180,
                    g: 213,
                    b: 254,
                    a: 180,
                },
                border_radius: 0.0.into(),
            });
        }
    }
    for line in &b.lines {
        for run in &line.text_runs {
            if let Some(node) = run.node {
                list.items.push(DisplayItem::HitRegion {
                    node,
                    enabled: run.hit,
                    rect: scaled(
                        Rect {
                            x: c.x + run.rect.x,
                            y: c.y + run.rect.y,
                            ..run.rect
                        },
                        s,
                    ),
                    border_radius: 0.0.into(),
                });
            }
            list.items.push(DisplayItem::Text {
                run: run.clone(),
                transform: Affine::translate((c.x * s, c.y * s)) * Affine::scale(s),
            });
        }
    }
    if let Some(ref form) = b.form_state {
        if form.is_focused
            && let Some(caret) = form.caret_rect
        {
            list.items.push(DisplayItem::FillRect {
                rect: scaled(caret, s),
                color: b.style.color,
                border_radius: 0.0.into(),
            });
        }
        if matches!(form.kind, Some(FormControlKind::Checkbox)) {
            let rect = scaled(b.content_box(), s);
            if form.checked {
                let accent = if form.disabled {
                    Color {
                        r: 200,
                        g: 200,
                        b: 200,
                        a: 255,
                    }
                } else {
                    Color {
                        r: 0,
                        g: 117,
                        b: 255,
                        a: 255,
                    }
                };
                list.items.push(DisplayItem::FillRect {
                    rect,
                    color: accent,
                    border_radius: (2.0 * s).into(),
                });
                let mut path = kurbo::BezPath::new();
                let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
                path.move_to((x + w * 0.22, y + h * 0.52));
                path.line_to((x + w * 0.44, y + h * 0.74));
                path.line_to((x + w * 0.80, y + h * 0.26));
                list.items.push(DisplayItem::StrokePath {
                    path,
                    color: Color::WHITE,
                    width: 1.8 * s,
                });
            }
        } else if matches!(form.kind, Some(FormControlKind::Radio)) {
            let rect = scaled(b.content_box(), s);
            let center = kurbo::Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            let radius = (rect.width.min(rect.height) / 2.0).max(1.0);
            let outer = kurbo::Circle::new(center, radius).to_path(0.1);
            list.items.push(DisplayItem::StrokePath {
                path: outer,
                color: Color {
                    r: 118,
                    g: 118,
                    b: 118,
                    a: 255,
                },
                width: 1.0 * s,
            });
            if form.checked {
                let dot_r = radius * 0.5;
                let inner = kurbo::Circle::new(center, dot_r).to_path(0.1);
                let accent = if form.disabled {
                    Color {
                        r: 200,
                        g: 200,
                        b: 200,
                        a: 255,
                    }
                } else {
                    Color {
                        r: 0,
                        g: 117,
                        b: 255,
                        a: 255,
                    }
                };
                list.items.push(DisplayItem::FillPath {
                    path: inner,
                    color: accent,
                });
            }
        } else if matches!(form.kind, Some(FormControlKind::Select)) {
            let rect = scaled(b.content_box(), s);
            let arrow_x = rect.right() - 14.0 * s;
            let arrow_y = rect.y + rect.height / 2.0 - 2.0 * s;
            let mut arrow = kurbo::BezPath::new();
            arrow.move_to((arrow_x, arrow_y));
            arrow.line_to((arrow_x + 8.0 * s, arrow_y));
            arrow.line_to((arrow_x + 4.0 * s, arrow_y + 5.0 * s));
            arrow.close_path();
            list.items.push(DisplayItem::FillPath {
                path: arrow,
                color: Color {
                    r: 100,
                    g: 100,
                    b: 100,
                    a: 255,
                },
            });
        }
    }
}
