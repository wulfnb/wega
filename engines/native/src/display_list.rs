//! Wega display commands carry the geometry used by both rasterization and
//! picking. Stacking contexts are collected across non-context ancestors.
use super::css::{Color, Length, ObjectFit, Overflow, Position};
use super::layout::{BoxType, LayoutBox, Point};
use anyrender::PaintScene;
use kurbo::{Affine, Shape};
use wega_dom::{NodeKey, layout::Rect};

#[derive(Debug, Clone)]
pub enum DisplayItem {
    FillRect {
        rect: Rect,
        color: Color,
        border_radius: f64,
    },
    StrokeRect {
        rect: Rect,
        color: Color,
        width: f64,
        border_radius: f64,
    },
    FillPath {
        path: kurbo::BezPath,
        color: Color,
    },
    StrokePath {
        path: kurbo::BezPath,
        color: Color,
        width: f64,
    },
    /// A linear gradient filling `rect` between two points.
    LinearGradient {
        rect: Rect,
        start: (f64, f64),
        end: (f64, f64),
        stops: Vec<(f32, Color)>,
        border_radius: f64,
    },
    BoxShadow {
        rect: Rect,
        offset_x: f64,
        offset_y: f64,
        blur_radius: f64,
        color: Color,
        border_radius: f64,
        /// The casting box's border box: outer shadows are never painted
        /// beneath it, even when its background is transparent.
        clip_out: Rect,
    },
    Image {
        rect: Rect,
        image: peniko::ImageData,
    },
    Text {
        run: super::text::GlyphRun,
        transform: Affine,
    },
    HitRegion {
        enabled: bool,
        node: NodeKey,
        rect: Rect,
        border_radius: f64,
    },
    PushClip {
        rect: Rect,
        border_radius: f64,
    },
    PushPathClip {
        path: kurbo::BezPath,
    },
    Svg {
        rect: Rect,
        tree: super::graphics::SvgGraphic,
    },
    PopClip,
    PushOpacity {
        opacity: f32,
    },
    PopOpacity,
    PushTransform {
        transform: Affine,
    },
    PopTransform,
    EmbeddedScene {
        rect: Rect,
        scene: anyrender::Scene,
    },
}
#[derive(Debug, Clone, Default)]
pub struct DisplayList {
    pub items: Vec<DisplayItem>,
}
fn kr(r: Rect) -> kurbo::Rect {
    kurbo::Rect::new(r.x, r.y, r.right(), r.bottom())
}
fn color(c: Color) -> peniko::Color {
    peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)
}
fn scaled(r: Rect, s: f64) -> Rect {
    Rect {
        x: r.x * s,
        y: r.y * s,
        width: r.width * s,
        height: r.height * s,
    }
}
impl DisplayList {
    pub fn build(root: &LayoutBox, embedded: &[(NodeKey, anyrender::Scene)], scale: f64) -> Self {
        Self::build_scrolled(root, embedded, scale, Point::default())
    }
    pub fn build_scrolled(
        root: &LayoutBox,
        embedded: &[(NodeKey, anyrender::Scene)],
        scale: f64,
        scroll: Point,
    ) -> Self {
        let mut list = Self::default();
        list.items.push(DisplayItem::PushTransform {
            transform: Affine::translate((-scroll.x * scale, -scroll.y * scale)),
        });
        paint_context(root, &mut list, embedded, scale, scroll);
        list.items.push(DisplayItem::PopTransform);
        list
    }
    pub fn to_anyrender_scene(&self) -> anyrender::Scene {
        let mut scene = anyrender::Scene::new();
        self.render_into(&mut scene);
        scene
    }
    pub fn render_into(&self, scene: &mut anyrender::Scene) {
        let mut current = Affine::IDENTITY;
        let mut transforms = Vec::new();
        for item in &self.items {
            match item {
                DisplayItem::FillRect {
                    rect,
                    color: c,
                    border_radius,
                } => scene.fill(
                    peniko::Fill::NonZero,
                    current,
                    color(*c),
                    None,
                    &kurbo::RoundedRect::from_rect(kr(*rect), *border_radius),
                ),
                DisplayItem::LinearGradient {
                    rect,
                    start,
                    end,
                    stops,
                    border_radius,
                } => {
                    let stops: Vec<peniko::ColorStop> = stops
                        .iter()
                        .map(|(offset, c)| peniko::ColorStop::from((*offset, color(*c))))
                        .collect();
                    let gradient = peniko::Gradient::new_linear(
                        kurbo::Point::new(start.0, start.1),
                        kurbo::Point::new(end.0, end.1),
                    )
                    .with_stops(stops.as_slice());
                    scene.fill(
                        peniko::Fill::NonZero,
                        current,
                        &gradient,
                        None,
                        &kurbo::RoundedRect::from_rect(kr(*rect), *border_radius),
                    );
                }
                DisplayItem::StrokeRect {
                    rect,
                    color: c,
                    width,
                    border_radius,
                } => {
                    if *width <= 0.0 {
                        continue;
                    }
                    let inset = kr(*rect).inset(-width / 2.0);
                    scene.stroke(
                        &kurbo::Stroke::new(*width),
                        current,
                        color(*c),
                        None,
                        &kurbo::RoundedRect::from_rect(
                            inset,
                            (*border_radius - width / 2.0).max(0.0),
                        ),
                    );
                }
                DisplayItem::FillPath { path, color: c } => {
                    scene.fill(peniko::Fill::NonZero, current, color(*c), None, path)
                }
                DisplayItem::StrokePath {
                    path,
                    color: c,
                    width,
                } => {
                    scene.stroke(&kurbo::Stroke::new(*width), current, color(*c), None, path);
                }
                DisplayItem::BoxShadow {
                    rect,
                    offset_x,
                    offset_y,
                    blur_radius,
                    color: c,
                    border_radius,
                    clip_out,
                } => {
                    // Clip to everything outside the border box (even-odd).
                    let reach = blur_radius.max(0.0) * 2.0 + offset_x.abs() + offset_y.abs() + 1.0;
                    let mut clip = kurbo::Rect::new(
                        rect.x - reach,
                        rect.y - reach,
                        rect.x + rect.width + reach,
                        rect.y + rect.height + reach,
                    )
                    .to_path(0.1);
                    clip.extend(
                        kurbo::RoundedRect::from_rect(kr(*clip_out), *border_radius).to_path(0.1),
                    );
                    scene.push_clip_layer(peniko::Fill::EvenOdd, current, &clip);
                    // CSS blur radius is twice the Gaussian standard deviation.
                    scene.draw_box_shadow(
                        current * Affine::translate((*offset_x, *offset_y)),
                        kr(*rect),
                        color(*c),
                        *border_radius,
                        blur_radius.max(0.0) / 2.0,
                    );
                    scene.pop_layer();
                }
                DisplayItem::Image { rect, image } => {
                    let brush = peniko::ImageBrush::new(image.clone());
                    scene.draw_image(
                        (&brush).into(),
                        current
                            * Affine::translate((rect.x, rect.y))
                            * Affine::scale_non_uniform(
                                rect.width / image.width.max(1) as f64,
                                rect.height / image.height.max(1) as f64,
                            ),
                    );
                }
                DisplayItem::Text { run, transform } => scene.draw_glyphs(
                    &run.font,
                    run.font_size,
                    true,
                    &run.coords,
                    run.embolden,
                    peniko::Fill::NonZero,
                    &anyrender::Paint::from(color(run.color)),
                    1.0,
                    current * *transform,
                    run.skew,
                    run.glyphs.iter().copied(),
                ),
                DisplayItem::PushClip {
                    rect,
                    border_radius,
                } => scene.push_clip_layer(
                    peniko::Fill::NonZero,
                    current,
                    &kurbo::RoundedRect::from_rect(kr(*rect), *border_radius),
                ),
                DisplayItem::PopClip | DisplayItem::PopOpacity => scene.pop_layer(),
                DisplayItem::PushOpacity { opacity } => scene.push_layer(
                    peniko::Fill::NonZero,
                    peniko::Mix::Normal,
                    *opacity,
                    Affine::IDENTITY,
                    &kurbo::Rect::new(-1e9, -1e9, 1e9, 1e9),
                    None,
                    None,
                ),
                DisplayItem::PushTransform { transform } => {
                    transforms.push(current);
                    current *= *transform;
                }
                DisplayItem::PopTransform => current = transforms.pop().unwrap_or(Affine::IDENTITY),
                DisplayItem::EmbeddedScene { rect, scene: child } => {
                    scene.push_clip_layer(peniko::Fill::NonZero, current, &kr(*rect));
                    scene
                        .append_scene(child.clone(), current * Affine::translate((rect.x, rect.y)));
                    scene.pop_layer();
                }
                DisplayItem::PushPathClip { path } => {
                    scene.push_clip_layer(peniko::Fill::NonZero, current, path)
                }
                DisplayItem::Svg { rect, tree } => {
                    scene.push_clip_layer(peniko::Fill::NonZero, current, &kr(*rect));
                    if !tree.degenerate {
                        anyrender_svg::render_svg_tree(
                            scene,
                            &tree.tree,
                            current
                                * Affine::translate((rect.x, rect.y))
                                * tree.transform(rect.width, rect.height),
                        );
                    }
                    scene.pop_layer();
                }
                DisplayItem::HitRegion { .. } => {}
            }
        }
    }
    /// Snapshot used by the embedding backend's measured geometry cache.
    pub fn geometry_snapshot(
        &self,
    ) -> (
        std::collections::HashMap<NodeKey, Vec<Rect>>,
        std::collections::HashMap<NodeKey, Affine>,
    ) {
        let mut rects: std::collections::HashMap<NodeKey, Vec<Rect>> =
            std::collections::HashMap::new();
        let mut transforms = std::collections::HashMap::new();
        let mut current = Affine::IDENTITY;
        let mut stack = Vec::new();
        for item in &self.items {
            match item {
                DisplayItem::PushTransform { transform } => {
                    stack.push(current);
                    current *= *transform;
                }
                DisplayItem::PopTransform => current = stack.pop().unwrap_or(Affine::IDENTITY),
                DisplayItem::HitRegion { node, rect, .. } => {
                    transforms.entry(*node).or_insert(current);
                    let r = current.transform_rect_bbox(kr(*rect));
                    rects.entry(*node).or_default().push(Rect {
                        x: r.x0,
                        y: r.y0,
                        width: r.width(),
                        height: r.height(),
                    });
                }
                _ => {}
            }
        }
        (rects, transforms)
    }
    pub fn node_transform(&self, key: NodeKey) -> Option<Affine> {
        let mut current = Affine::IDENTITY;
        let mut stack = Vec::new();
        for item in &self.items {
            match item {
                DisplayItem::PushTransform { transform } => {
                    stack.push(current);
                    current *= *transform;
                }
                DisplayItem::PopTransform => current = stack.pop().unwrap_or(Affine::IDENTITY),
                DisplayItem::HitRegion { node, .. } if *node == key => return Some(current),
                _ => {}
            }
        }
        None
    }
    pub fn node_rects(&self, key: NodeKey) -> Vec<Rect> {
        let mut current = Affine::IDENTITY;
        let mut stack = Vec::new();
        let mut rects = Vec::new();
        for item in &self.items {
            match item {
                DisplayItem::PushTransform { transform } => {
                    stack.push(current);
                    current *= *transform;
                }
                DisplayItem::PopTransform => current = stack.pop().unwrap_or(Affine::IDENTITY),
                DisplayItem::HitRegion { node, rect, .. } if *node == key => {
                    let r = current.transform_rect_bbox(kr(*rect));
                    rects.push(Rect {
                        x: r.x0,
                        y: r.y0,
                        width: r.width(),
                        height: r.height(),
                    });
                }
                _ => {}
            }
        }
        rects
    }
    /// Resolve in the exact command order, applying the same clip and affine
    /// stacks as painting. Later visible regions replace earlier regions.
    pub fn hit_test(&self, p: Point) -> Option<NodeKey> {
        let mut current = Affine::IDENTITY;
        let mut transforms = Vec::new();
        let mut clips = Vec::new();
        let mut hit = None;
        let point = kurbo::Point::new(p.x, p.y);
        for item in &self.items {
            match item {
                DisplayItem::PushTransform { transform } => {
                    transforms.push(current);
                    current *= *transform;
                }
                DisplayItem::PopTransform => current = transforms.pop().unwrap_or(Affine::IDENTITY),
                DisplayItem::PushClip {
                    rect,
                    border_radius,
                } => clips.push((
                    kurbo::RoundedRect::from_rect(kr(*rect), *border_radius).to_path(0.1),
                    current,
                )),
                DisplayItem::PushPathClip { path } => clips.push((path.clone(), current)),
                DisplayItem::PopClip => {
                    clips.pop();
                }
                DisplayItem::HitRegion {
                    enabled,
                    node,
                    rect,
                    border_radius,
                } => {
                    if !enabled {
                        continue;
                    }
                    if current.determinant().abs() < 1e-12 {
                        continue;
                    }
                    if clips.iter().all(|(clip, t)| {
                        t.determinant().abs() > 1e-12 && clip.contains(t.inverse() * point)
                    }) && kurbo::RoundedRect::from_rect(kr(*rect), *border_radius)
                        .contains(current.inverse() * point)
                    {
                        hit = Some(*node);
                    }
                }
                _ => {}
            }
        }
        hit
    }
}
fn context(b: &LayoutBox, parent_flex: bool) -> bool {
    b.style.clip_path.is_some()
        || b.style.transform.is_some()
        || b.style.opacity < 1.0
        || (b.style.z_index.is_some() && (b.style.position != Position::Static || parent_flex))
        || b.style.position == Position::Fixed
}
fn clipped(b: &LayoutBox) -> bool {
    b.style.overflow_x != Overflow::Visible || b.style.overflow_y != Overflow::Visible
}
fn push_contents(b: &LayoutBox, list: &mut DisplayList, s: f64) {
    if clipped(b) {
        list.items.push(DisplayItem::PushClip {
            rect: scaled(b.padding_box(), s),
            border_radius: (b.radius() - b.border.left.max(b.border.top)).max(0.0) * s,
        });
    }
    list.items.push(DisplayItem::PushTransform {
        transform: Affine::translate((-b.scroll_offset.x * s, -b.scroll_offset.y * s)),
    });
}
fn pop_contents(b: &LayoutBox, list: &mut DisplayList) {
    list.items.push(DisplayItem::PopTransform);
    if clipped(b) {
        list.items.push(DisplayItem::PopClip);
    }
}
fn decorations(b: &LayoutBox, list: &mut DisplayList, s: f64) {
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
        let radius = b.radius() * s;
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
fn content(
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
                border_radius: b.radius() * s,
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
                border_radius: 0.0,
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
                    border_radius: 0.0,
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
                border_radius: 0.0,
            });
        }
        if matches!(form.kind, Some(super::layout::FormControlKind::Checkbox)) {
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
                    border_radius: 2.0 * s,
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
        } else if matches!(form.kind, Some(super::layout::FormControlKind::Radio)) {
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
        } else if matches!(form.kind, Some(super::layout::FormControlKind::Select)) {
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
struct Entry<'a> {
    b: &'a LayoutBox,
    path: Vec<&'a LayoutBox>,
    z: i32,
    is_context: bool,
}
fn collect<'a>(b: &'a LayoutBox, path: &mut Vec<&'a LayoutBox>, entries: &mut Vec<Entry<'a>>) {
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
fn flow(
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
fn paint_entry(
    entry: &Entry<'_>,
    list: &mut DisplayList,
    embedded: &[(NodeKey, anyrender::Scene)],
    s: f64,
    mut scroll: Point,
) {
    for ancestor in &entry.path {
        push_contents(ancestor, list, s);
        scroll.x += ancestor.scroll_offset.x;
        scroll.y += ancestor.scroll_offset.y;
    }
    if entry.is_context {
        paint_context(entry.b, list, embedded, s, scroll);
    } else {
        flow(entry.b, list, embedded, s, scroll);
    }
    for ancestor in entry.path.iter().rev() {
        pop_contents(ancestor, list);
    }
}
fn paint_context(
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
            path: Affine::scale(s) * clip.path(b.border_box, b.style.font_size),
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

fn background(b: &LayoutBox, list: &mut DisplayList, s: f64) {
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
