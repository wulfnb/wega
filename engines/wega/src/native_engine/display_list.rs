//! Wega-owned Display List and Painting Adapter.
//!
//! Transforms computed `LayoutBox` trees into deterministic 2D drawing
//! commands (`DisplayItem`) and converts them into Skia / `anyrender::Scene` primitives.
//! Supports stacking contexts, z-index ordering, transforms, box-shadows,
//! rounded borders, and iframe scene composition.

use crate::native_engine::css::Color;
use crate::native_engine::layout::LayoutBox;
use anyrender::PaintScene;
use wega_dom::NodeKey;
use wega_dom::layout::Rect;

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
        transform: kurbo::Affine,
    },
    PushClip {
        rect: Rect,
    },
    PopClip,
    PushOpacity {
        opacity: f32,
    },
    PopOpacity,
    PushTransform {
        transform: kurbo::Affine,
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

impl DisplayList {
    pub fn build(
        root: &LayoutBox,
        embedded_children: &[(NodeKey, anyrender::Scene)],
        scale: f64,
    ) -> Self {
        let mut list = Self::default();
        paint_box(root, &mut list, embedded_children, scale);
        list
    }

    pub fn to_anyrender_scene(&self) -> anyrender::Scene {
        let mut scene = anyrender::Scene::new();
        self.render_into(&mut scene);
        scene
    }

    pub fn render_into(&self, scene: &mut anyrender::Scene) {
        for item in &self.items {
            match item {
                DisplayItem::BoxShadow {
                    rect,
                    offset_x,
                    offset_y,
                    blur_radius: _,
                    color,
                    border_radius,
                } => {
                    if color.a == 0 {
                        continue;
                    }
                    let p_color = peniko::Color::from_rgba8(color.r, color.g, color.b, color.a);
                    let shadow_rect = kurbo::Rect::new(
                        rect.x + offset_x,
                        rect.y + offset_y,
                        rect.x + rect.width + offset_x,
                        rect.y + rect.height + offset_y,
                    );
                    if *border_radius > 0.0 {
                        let rounded = kurbo::RoundedRect::from_rect(shadow_rect, *border_radius);
                        scene.fill(
                            peniko::Fill::NonZero,
                            kurbo::Affine::IDENTITY,
                            p_color,
                            None,
                            &rounded,
                        );
                    } else {
                        scene.fill(
                            peniko::Fill::NonZero,
                            kurbo::Affine::IDENTITY,
                            p_color,
                            None,
                            &shadow_rect,
                        );
                    }
                }
                DisplayItem::FillRect {
                    rect,
                    color,
                    border_radius,
                } => {
                    if color.a == 0 {
                        continue;
                    }
                    let p_color = peniko::Color::from_rgba8(color.r, color.g, color.b, color.a);
                    let kurbo_rect =
                        kurbo::Rect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
                    if *border_radius > 0.0 {
                        let rounded = kurbo::RoundedRect::from_rect(kurbo_rect, *border_radius);
                        scene.fill(
                            peniko::Fill::NonZero,
                            kurbo::Affine::IDENTITY,
                            p_color,
                            None,
                            &rounded,
                        );
                    } else {
                        scene.fill(
                            peniko::Fill::NonZero,
                            kurbo::Affine::IDENTITY,
                            p_color,
                            None,
                            &kurbo_rect,
                        );
                    }
                }
                DisplayItem::StrokeRect {
                    rect,
                    color,
                    width,
                    border_radius,
                } => {
                    if color.a == 0 || *width <= 0.0 {
                        continue;
                    }
                    let p_color = peniko::Color::from_rgba8(color.r, color.g, color.b, color.a);
                    let kurbo_rect =
                        kurbo::Rect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
                    let stroke = kurbo::Stroke::new(*width);
                    if *border_radius > 0.0 {
                        let rounded = kurbo::RoundedRect::from_rect(kurbo_rect, *border_radius);
                        scene.stroke(&stroke, kurbo::Affine::IDENTITY, p_color, None, &rounded);
                    } else {
                        scene.stroke(&stroke, kurbo::Affine::IDENTITY, p_color, None, &kurbo_rect);
                    }
                }
                DisplayItem::Text { run, transform } => {
                    let color = run.color;
                    scene.draw_glyphs(
                        &run.font,
                        run.font_size,
                        true,
                        &run.coords,
                        run.embolden,
                        peniko::Fill::NonZero,
                        &anyrender::Paint::from(peniko::Color::from_rgba8(
                            color.r, color.g, color.b, color.a,
                        )),
                        1.0,
                        *transform,
                        run.skew,
                        run.glyphs.iter().copied(),
                    );
                }
                DisplayItem::PushClip { rect } => {
                    let kurbo_rect =
                        kurbo::Rect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
                    scene.push_clip_layer(
                        peniko::Fill::NonZero,
                        kurbo::Affine::IDENTITY,
                        &kurbo_rect,
                    );
                }
                DisplayItem::PopClip => {
                    scene.pop_layer();
                }
                DisplayItem::PushOpacity { opacity } => {
                    scene.push_layer(
                        peniko::Fill::NonZero,
                        peniko::Mix::Normal,
                        *opacity,
                        kurbo::Affine::IDENTITY,
                        &kurbo::Rect::new(-1e9, -1e9, 1e9, 1e9),
                        None,
                        None,
                    );
                }
                DisplayItem::PopOpacity => {
                    scene.pop_layer();
                }
                DisplayItem::PushTransform { transform } => {
                    scene.push_layer(
                        peniko::Fill::NonZero,
                        peniko::Mix::Normal,
                        1.0,
                        *transform,
                        &kurbo::Rect::new(-1e9, -1e9, 1e9, 1e9),
                        None,
                        None,
                    );
                }
                DisplayItem::PopTransform => {
                    scene.pop_layer();
                }
                DisplayItem::EmbeddedScene {
                    rect,
                    scene: child_scene,
                } => {
                    let kurbo_rect =
                        kurbo::Rect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
                    scene.push_clip_layer(
                        peniko::Fill::NonZero,
                        kurbo::Affine::IDENTITY,
                        &kurbo_rect,
                    );
                    scene.append_scene(
                        child_scene.clone(),
                        kurbo::Affine::translate((rect.x, rect.y)),
                    );
                    scene.pop_layer();
                }
            }
        }
    }
}

fn paint_box(
    layout_box: &LayoutBox,
    list: &mut DisplayList,
    embedded: &[(NodeKey, anyrender::Scene)],
    scale: f64,
) {
    let mut rect = layout_box.border_box;
    rect.x *= scale;
    rect.y *= scale;
    rect.width *= scale;
    rect.height *= scale;

    let has_transform = layout_box.style.transform.is_some();
    if let Some(t) = layout_box.style.transform {
        list.items.push(DisplayItem::PushTransform {
            transform: t.to_kurbo(),
        });
    }

    let has_opacity = layout_box.style.opacity < 1.0;
    if has_opacity {
        list.items.push(DisplayItem::PushOpacity {
            opacity: layout_box.style.opacity,
        });
    }

    // 1. Paint box shadow if configured
    if let Some(shadow) = layout_box.style.box_shadow {
        list.items.push(DisplayItem::BoxShadow {
            rect,
            offset_x: shadow.offset_x * scale,
            offset_y: shadow.offset_y * scale,
            blur_radius: shadow.blur_radius * scale,
            color: shadow.color,
            border_radius: layout_box.style.border_radius * scale,
        });
    }

    // 2. Paint background
    if layout_box.style.background_color != Color::TRANSPARENT {
        list.items.push(DisplayItem::FillRect {
            rect,
            color: layout_box.style.background_color,
            border_radius: layout_box.style.border_radius * scale,
        });
    }

    // 3. Paint borders
    if layout_box.style.borders.top.width > 0.0 && layout_box.style.borders.top.color.a > 0 {
        list.items.push(DisplayItem::StrokeRect {
            rect,
            color: layout_box.style.borders.top.color,
            width: layout_box.style.borders.top.width * scale,
            border_radius: layout_box.style.border_radius * scale,
        });
    }

    // Classify children into Stacking Context groups
    let mut neg_z = Vec::new();
    let mut normal_flow = Vec::new();
    let mut pos_z = Vec::new();

    for child in &layout_box.children {
        match child.style.z_index {
            Some(z) if z < 0 => neg_z.push((z, child)),
            Some(z) if z > 0 => pos_z.push((z, child)),
            _ => normal_flow.push(child),
        }
    }

    neg_z.sort_by_key(|(z, _)| *z);
    pos_z.sort_by_key(|(z, _)| *z);

    // 4. Paint negative z-index children
    for (_, child) in neg_z {
        paint_box(child, list, embedded, scale);
    }

    // 5. Paint embedded iframe if matched
    if let Some(key) = layout_box.node {
        if let Some((_, child_scene)) = embedded.iter().find(|(k, _)| *k == key) {
            let mut content = layout_box.content_box();
            content.x *= scale;
            content.y *= scale;
            content.width *= scale;
            content.height *= scale;
            list.items.push(DisplayItem::EmbeddedScene {
                rect: content,
                scene: child_scene.clone(),
            });
        }
    }

    // 6. Paint lines for inline formatting context
    let pad_border_x = (layout_box.border.left + layout_box.padding.left) * scale;
    let pad_border_y = (layout_box.border.top + layout_box.padding.top) * scale;

    for line in &layout_box.lines {
        for run in &line.text_runs {
            list.items.push(DisplayItem::Text {
                run: run.clone(),
                transform: kurbo::Affine::translate((rect.x + pad_border_x, rect.y + pad_border_y))
                    * kurbo::Affine::scale(scale),
            });
        }
    }

    // 7. Overflow clipping if overflow != visible
    let has_clip = layout_box.style.overflow_x != crate::native_engine::css::Overflow::Visible
        || layout_box.style.overflow_y != crate::native_engine::css::Overflow::Visible;

    if has_clip {
        let mut pbox = layout_box.padding_box();
        pbox.x *= scale;
        pbox.y *= scale;
        pbox.width *= scale;
        pbox.height *= scale;
        list.items.push(DisplayItem::PushClip { rect: pbox });
    }

    // 8. Paint normal in-flow child boxes
    for child in normal_flow {
        paint_box(child, list, embedded, scale);
    }

    if has_clip {
        list.items.push(DisplayItem::PopClip);
    }

    // 9. Paint positive z-index children
    for (_, child) in pos_z {
        paint_box(child, list, embedded, scale);
    }

    if has_opacity {
        list.items.push(DisplayItem::PopOpacity);
    }

    if has_transform {
        list.items.push(DisplayItem::PopTransform);
    }
}
