//! Display items and the display list.
use crate::*;

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
        run: wega_layout::text::GlyphRun,
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
        tree: SvgGraphic,
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

pub(crate) fn kr(r: Rect) -> kurbo::Rect {
    kurbo::Rect::new(r.x, r.y, r.right(), r.bottom())
}

pub(crate) fn color(c: Color) -> peniko::Color {
    peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

pub(crate) fn scaled(r: Rect, s: f64) -> Rect {
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

/// The topmost node painted at `point`.
pub fn hit_test_node(box_node: &LayoutBox, point: Point) -> Option<NodeKey> {
    DisplayList::build(box_node, &[], 1.0).hit_test(point)
}
