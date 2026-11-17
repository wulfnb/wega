//! Wega Native Display List & Skia Painting Adapter.
use super::css::Color;
use super::layout::LayoutBox;
use anyrender::PaintScene;
use wega_dom::layout::Rect;
use wega_dom::NodeKey;

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
    },
    Text {
        rect: Rect,
        text: String,
        color: Color,
        font_size: f64,
    },
    PushClip {
        rect: Rect,
    },
    PopClip,
    PushOpacity {
        opacity: f32,
    },
    PopOpacity,
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
                DisplayItem::FillRect { rect, color, border_radius } => {
                    if color.a == 0 {
                        continue;
                    }
                    let p_color = peniko::Color::from_rgba8(color.r, color.g, color.b, color.a);
                    let kurbo_rect = kurbo::Rect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
                    if *border_radius > 0.0 {
                        let rounded = kurbo::RoundedRect::from_rect(kurbo_rect, *border_radius);
                        scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, p_color, None, &rounded);
                    } else {
                        scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, p_color, None, &kurbo_rect);
                    }
                }
                DisplayItem::StrokeRect { rect, color, width } => {
                    if color.a == 0 || *width <= 0.0 {
                        continue;
                    }
                    let p_color = peniko::Color::from_rgba8(color.r, color.g, color.b, color.a);
                    let kurbo_rect = kurbo::Rect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
                    let stroke = kurbo::Stroke::new(*width);
                    scene.stroke(&stroke, kurbo::Affine::IDENTITY, p_color, None, &kurbo_rect);
                }
                DisplayItem::Text { rect, text: _, color, font_size: _ } => {
                    // Approximate text block rendering using solid color bar for basic vertical slice
                    // Full text glyph shaping maps here in Checkpoint B
                    let p_color = peniko::Color::from_rgba8(color.r, color.g, color.b, (color.a as f32 * 0.9) as u8);
                    let bar_h = (rect.height * 0.7).max(2.0);
                    let bar_y = rect.y + (rect.height - bar_h) / 2.0;
                    let kurbo_rect = kurbo::Rect::new(rect.x, bar_y, rect.x + rect.width, bar_y + bar_h);
                    scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, p_color, None, &kurbo_rect);
                }
                DisplayItem::PushClip { rect } => {
                    let kurbo_rect = kurbo::Rect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
                    scene.push_clip_layer(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, &kurbo_rect);
                }
                DisplayItem::PopClip => {
                    scene.pop_layer();
                }
                DisplayItem::PushOpacity { opacity } => {
                    scene.push_layer(peniko::Fill::NonZero, peniko::Mix::Normal, *opacity, kurbo::Affine::IDENTITY, &kurbo::Rect::new(-1e9, -1e9, 1e9, 1e9), None, None);
                }
                DisplayItem::PopOpacity => {
                    scene.pop_layer();
                }
                DisplayItem::EmbeddedScene { rect, scene: child_scene } => {
                    let kurbo_rect = kurbo::Rect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
                    scene.push_clip_layer(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, &kurbo_rect);
                    scene.append_scene(child_scene.clone(), kurbo::Affine::translate((rect.x, rect.y)));
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

    let has_opacity = layout_box.style.opacity < 1.0;
    if has_opacity {
        list.items.push(DisplayItem::PushOpacity { opacity: layout_box.style.opacity });
    }

    // 1. Paint background
    if layout_box.style.background_color != Color::TRANSPARENT {
        list.items.push(DisplayItem::FillRect {
            rect,
            color: layout_box.style.background_color,
            border_radius: layout_box.style.border_radius * scale,
        });
    }

    // 2. Paint borders
    if layout_box.style.borders.top.width > 0.0 && layout_box.style.borders.top.color.a > 0 {
        list.items.push(DisplayItem::StrokeRect {
            rect,
            color: layout_box.style.borders.top.color,
            width: layout_box.style.borders.top.width * scale,
        });
    }

    // 3. Paint embedded iframe if matched
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

    // 4. Paint lines for inline formatting context
    let pad_border_x = (layout_box.border.left + layout_box.padding.left) * scale;
    let pad_border_y = (layout_box.border.top + layout_box.padding.top) * scale;

    for line in &layout_box.lines {
        for (run_rect, text) in &line.text_runs {
            let mut r = *run_rect;
            r.x = rect.x + pad_border_x + r.x * scale;
            r.y = rect.y + pad_border_y + r.y * scale;
            r.width *= scale;
            r.height *= scale;
            list.items.push(DisplayItem::Text {
                rect: r,
                text: text.clone(),
                color: layout_box.style.color,
                font_size: layout_box.style.font_size * scale,
            });
        }
    }

    // 5. Paint child boxes
    for child in &layout_box.children {
        paint_box(child, list, embedded, scale);
    }

    if has_opacity {
        list.items.push(DisplayItem::PopOpacity);
    }
}
