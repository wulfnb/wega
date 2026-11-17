//! Wega display commands carry the geometry used by both rasterization and
//! picking. Stacking contexts are collected across non-context ancestors.
use super::css::{Color, Overflow, Position};
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
    BoxShadow {
        rect: Rect,
        offset_x: f64,
        offset_y: f64,
        blur_radius: f64,
        color: Color,
        border_radius: f64,
    },
    Text {
        run: super::text::GlyphRun,
        transform: Affine,
    },
    HitRegion {
        node: NodeKey,
        rect: Rect,
        border_radius: f64,
    },
    PushClip {
        rect: Rect,
        border_radius: f64,
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
                DisplayItem::BoxShadow {
                    rect,
                    offset_x,
                    offset_y,
                    blur_radius,
                    color: c,
                    border_radius,
                } => {
                    // CSS blur radius is twice the Gaussian standard deviation.
                    scene.draw_box_shadow(
                        current * Affine::translate((*offset_x, *offset_y)),
                        kr(*rect),
                        color(*c),
                        *border_radius,
                        blur_radius.max(0.0) / 2.0,
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
                DisplayItem::HitRegion { .. } => {}
            }
        }
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
                    kurbo::RoundedRect::from_rect(kr(*rect), *border_radius),
                    current,
                )),
                DisplayItem::PopClip => {
                    clips.pop();
                }
                DisplayItem::HitRegion {
                    node,
                    rect,
                    border_radius,
                } => {
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
    b.style.transform.is_some()
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
            border_radius: (b.style.border_radius - b.border.left.max(b.border.top)).max(0.0) * s,
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
        let radius = b.style.border_radius * s;
        if let Some(k) = b.node {
            list.items.push(DisplayItem::HitRegion {
                node: k,
                rect,
                border_radius: radius,
            });
        }
        if let Some(shadow) = b.style.box_shadow {
            list.items.push(DisplayItem::BoxShadow {
                rect,
                offset_x: shadow.offset_x * s,
                offset_y: shadow.offset_y * s,
                blur_radius: shadow.blur_radius * s,
                color: shadow.color,
                border_radius: radius,
            });
        }
        if b.style.background_color.a > 0 {
            list.items.push(DisplayItem::FillRect {
                rect,
                color: b.style.background_color,
                border_radius: radius,
            });
        }
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
    if let Some((_, scene)) = embedded.iter().find(|(k, _)| Some(*k) == b.node) {
        list.items.push(DisplayItem::EmbeddedScene {
            rect: scaled(b.content_box(), s),
            scene: scene.clone(),
        });
    }
    let c = b.content_box();
    for line in &b.lines {
        for run in &line.text_runs {
            if let Some(node) = run.node {
                list.items.push(DisplayItem::HitRegion {
                    node,
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
    if transform.is_some() {
        list.items.push(DisplayItem::PopTransform);
    }
    if fixed {
        list.items.push(DisplayItem::PopTransform);
    }
}
