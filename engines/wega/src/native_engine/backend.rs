use wega_protocol::DocumentContext;
use wega_protocol::inspector::dom::InspectorNodeId;
use wega_protocol::inspector::geometry::*;

fn to_geom_rect(r: Rect) -> GeomRect {
    GeomRect {
        x: r.x,
        y: r.y,
        width: r.width,
        height: r.height,
    }
}
fn to_edges(i: Insets) -> Edges {
    Edges {
        top: i.top,
        right: i.right,
        bottom: i.bottom,
        left: i.left,
    }
}
fn to_box_set(b: BoxModel) -> BoxSet {
    BoxSet {
        border_box: to_geom_rect(b.border_box),
        border: to_edges(b.border),
        padding: to_edges(b.padding),
        margin: to_edges(b.margin),
    }
}

/// Native Engine Backend: Backend selector, NativeDocument, and Wega DOM integration.
use super::css::{StyleSheet, resolve_computed_style};
use super::display_list::DisplayList;
use super::layout::{LayoutBox, LayoutContext, build_box_tree, perform_layout};
use std::cell::RefCell;
use std::rc::Rc;
use wega_adapter_blitz::{BlitzDocument, ToNodeId};
use wega_dom::layout::*;
use wega_dom::style::StyleError;
use wega_dom::{DocumentView, NodeKey, NodeKind, NodeView};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderingBackend {
    #[default]
    Blitz,
    WegaNative,
}

impl RenderingBackend {
    pub fn from_env() -> Self {
        match std::env::var("WEGA_RENDER_BACKEND").as_deref() {
            Ok("native") | Ok("WegaNative") | Ok("wega") => Self::WegaNative,
            _ => Self::Blitz,
        }
    }
}

pub struct NativeDocument {
    pub doc: Rc<RefCell<BlitzDocument>>,
    pub layout_tree: Option<LayoutBox>,
    pub author_sheets: Vec<StyleSheet>,
    pub viewport: ViewportConfig,
    pub scroll: Point,
    pub layout_dirty: bool,
    seen_epoch: Option<u64>,
    seen_viewport: Option<(u32, u32, u32, u32, bool)>,
    pub layout_generation: u64,
}

impl NativeDocument {
    pub fn new(doc: Rc<RefCell<BlitzDocument>>) -> Self {
        let initial_viewport = ViewportConfig {
            width_px: 1280,
            height_px: 800,
            scale: 1.0,
            zoom: 1.0,
            dark: false,
        };
        Self {
            doc,
            layout_tree: None,
            author_sheets: Vec::new(),
            viewport: initial_viewport,
            scroll: Point::default(),
            layout_dirty: true,
            seen_epoch: None,
            seen_viewport: None,
            layout_generation: 0,
        }
    }

    pub fn invalidate(&mut self) {
        self.layout_dirty = true;
    }

    pub fn sync_author_stylesheets(&mut self) {
        let d = self.doc.borrow();
        let mut sheets = Vec::new();
        // Traverse DOM looking for <style> elements
        let root = d.root();
        let mut stack = vec![root];
        while let Some(k) = stack.pop() {
            if let Some(node) = d.node(k) {
                if node.kind() == NodeKind::Element && !d.stylesheet_disabled(k.node_id()) {
                    if let Some(elem) = node.element_name() {
                        if elem.local.eq_ignore_ascii_case("style") {
                            let mut css_text = String::new();
                            for child_k in node.children() {
                                if let Some(text) = d.node(child_k).and_then(NodeView::text) {
                                    css_text.push_str(text);
                                }
                            }
                            sheets.push(StyleSheet::parse(&css_text));
                        } else if elem.local.eq_ignore_ascii_case("link")
                            && node.attr("rel").is_some_and(|r| {
                                r.split_ascii_whitespace()
                                    .any(|r| r.eq_ignore_ascii_case("stylesheet"))
                            })
                        {
                            if let Some((source, requested_url)) = d.stylesheet_source(k.node_id())
                            {
                                if node
                                    .attr("href")
                                    .and_then(|href| d.url().join(href).ok())
                                    .is_some_and(|u| u.as_str() == requested_url)
                                {
                                    sheets.push(StyleSheet::parse(source));
                                }
                            }
                        }
                    }
                }
                for c in node.children().rev() {
                    stack.push(c);
                }
            }
        }
        self.author_sheets = sheets;
    }

    fn viewport_key(&self) -> (u32, u32, u32, u32, bool) {
        (
            self.viewport.width_px,
            self.viewport.height_px,
            self.viewport.scale.to_bits(),
            self.viewport.zoom.to_bits(),
            self.viewport.dark,
        )
    }
    pub fn is_dirty(&self) -> bool {
        self.layout_dirty
            || self.seen_epoch
                != Some(wega_dom::MutationClock::mutation_epoch(&*self.doc.borrow()).as_u64())
            || self.seen_viewport != Some(self.viewport_key())
    }
    pub fn ensure_layout(&mut self) {
        if self.is_dirty() {
            self.flush_layout();
        }
    }
    pub fn flush_layout(&mut self) {
        let mut offsets = std::collections::HashMap::new();
        fn save(b: &LayoutBox, out: &mut std::collections::HashMap<NodeKey, Point>) {
            if let Some(k) = b.node {
                out.insert(k, b.scroll_offset);
            }
            for c in &b.children {
                save(c, out);
            }
        }
        if let Some(tree) = &self.layout_tree {
            save(tree, &mut offsets);
        }
        self.sync_author_stylesheets();
        let d = self.doc.borrow();
        let root_key = d.root();
        let sheets_ref: Vec<&StyleSheet> = self.author_sheets.iter().collect();

        if let Some(mut box_tree) = build_box_tree(&*d, root_key, None, &sheets_ref) {
            let ctx = LayoutContext {
                viewport_width: self.viewport.width_px as f64
                    / (self.viewport.scale * self.viewport.zoom) as f64,
                viewport_height: self.viewport.height_px as f64
                    / (self.viewport.scale * self.viewport.zoom) as f64,
                font_size_root: 16.0,
            };
            perform_layout(&mut box_tree, &ctx);
            fn restore(b: &mut LayoutBox, offsets: &std::collections::HashMap<NodeKey, Point>) {
                if let Some(p) = b.node.and_then(|k| offsets.get(&k)) {
                    let client = b.padding_box();
                    b.scroll_offset = Point {
                        x: p.x.clamp(0.0, (b.scroll_width - client.width).max(0.0)),
                        y: p.y.clamp(0.0, (b.scroll_height - client.height).max(0.0)),
                    };
                }
                for c in &mut b.children {
                    restore(c, offsets);
                }
            }
            restore(&mut box_tree, &offsets);
            self.layout_tree = Some(box_tree);
        } else {
            self.layout_tree = None;
        }
        self.seen_epoch = Some(wega_dom::MutationClock::mutation_epoch(&*d).as_u64());
        self.seen_viewport = Some(self.viewport_key());
        self.layout_generation = self.layout_generation.wrapping_add(1);
        self.layout_dirty = false;
    }

    pub fn find_box(&self, key: NodeKey) -> Option<&LayoutBox> {
        self.layout_tree
            .as_ref()
            .and_then(|tree| find_box_in_tree(tree, key))
    }

    pub fn bounding_rect(&self, key: NodeKey) -> Result<Rect, LayoutError> {
        let rects = self.client_rects(key)?;
        Ok(rects
            .into_iter()
            .reduce(|a, b| {
                let x = a.x.min(b.x);
                let y = a.y.min(b.y);
                Rect {
                    x,
                    y,
                    width: a.right().max(b.right()) - x,
                    height: a.bottom().max(b.bottom()) - y,
                }
            })
            .unwrap_or(Rect::ZERO))
    }

    pub fn point_in_content(&self, key: NodeKey, p: Point) -> Option<Point> {
        let tree = self.layout_tree.as_ref()?;
        let b = self.find_box(key)?;
        let t = DisplayList::build_scrolled(tree, &[], 1.0, self.scroll).node_transform(key)?;
        if t.determinant().abs() < 1e-12 {
            return None;
        }
        let local = t.inverse() * kurbo::Point::new(p.x, p.y);
        let content = b.content_box();
        Some(Point {
            x: local.x - content.x,
            y: local.y - content.y,
        })
    }
    pub fn client_rects(&self, key: NodeKey) -> Result<Vec<Rect>, LayoutError> {
        if self.doc.borrow().node(key).is_none() {
            return Err(LayoutError::Stale(key));
        }
        Ok(self
            .layout_tree
            .as_ref()
            .map(|tree| DisplayList::build_scrolled(tree, &[], 1.0, self.scroll).node_rects(key))
            .unwrap_or_default())
    }
    pub fn client_box(&self, key: NodeKey) -> Result<ClientBox, LayoutError> {
        let b = self.find_box(key).ok_or(LayoutError::Stale(key))?;
        Ok(b.client_box())
    }

    pub fn content_box(&self, key: NodeKey) -> Result<ContentBox, LayoutError> {
        let b = self.find_box(key).ok_or(LayoutError::Stale(key))?;
        Ok(b.content_box_metrics())
    }

    pub fn offset_box(&self, key: NodeKey) -> Result<OffsetBox, LayoutError> {
        let b = self.find_box(key).ok_or(LayoutError::Stale(key))?;
        if b.style.position == super::css::Position::Fixed {
            return Ok(OffsetBox {
                rect: b.border_box,
                parent: None,
            });
        }
        let d = self.doc.borrow();
        let mut at = d.node(key).and_then(NodeView::parent);
        while let Some(parent) = at {
            if let Some(p) = self.find_box(parent) {
                if p.style.position != super::css::Position::Static
                    || d.node(parent)
                        .and_then(NodeView::element_name)
                        .is_some_and(|n| n.local == "body")
                {
                    return Ok(OffsetBox {
                        rect: Rect {
                            x: b.border_box.x - p.border_box.x - p.border.left,
                            y: b.border_box.y - p.border_box.y - p.border.top,
                            ..b.border_box
                        },
                        parent: Some(parent),
                    });
                }
            }
            at = d.node(parent).and_then(NodeView::parent);
        }
        Ok(OffsetBox {
            rect: b.border_box,
            parent: None,
        })
    }

    pub fn scroll_size(&self, key: NodeKey) -> Result<Size, LayoutError> {
        let b = self.find_box(key).ok_or(LayoutError::Stale(key))?;
        Ok(Size {
            width: b.scroll_width,
            height: b.scroll_height,
        })
    }

    pub fn scroll_offset(&self, key: NodeKey) -> Result<Point, LayoutError> {
        let b = self.find_box(key).ok_or(LayoutError::Stale(key))?;
        Ok(b.scroll_offset)
    }

    pub fn inspect_geometry(&self, key: NodeKey) -> Geometry {
        if let Some(b) = self.find_box(key) {
            let mut bm = b.box_model();
            bm.border_box.x -= self.scroll.x;
            bm.border_box.y -= self.scroll.y;
            if self.is_dirty() {
                Geometry::Stale(bm)
            } else {
                Geometry::Current(bm)
            }
        } else {
            Geometry::NoBoxes
        }
    }

    pub fn element_at(&self, point: Point) -> Option<NodeKey> {
        let key = self.layout_tree.as_ref().and_then(|tree| {
            DisplayList::build_scrolled(tree, &[], 1.0, self.scroll).hit_test(point)
        })?;
        let d = self.doc.borrow();
        let mut key = key;
        loop {
            let node = d.node(key)?;
            if node.kind() == NodeKind::Element {
                return Some(key);
            }
            key = node.parent()?;
        }
    }

    pub fn record_scene(&mut self, viewport: &ViewportConfig) -> anyrender::Scene {
        self.record_scene_with_children(viewport, Vec::new())
    }

    pub fn record_scene_with_children(
        &mut self,
        viewport: &ViewportConfig,
        children: Vec<(NodeKey, anyrender::Scene)>,
    ) -> anyrender::Scene {
        self.viewport = *viewport;
        self.ensure_layout();
        let scale = (viewport.scale * viewport.zoom) as f64;
        let Some(tree) = &self.layout_tree else {
            return anyrender::Scene::new();
        };
        let dl = DisplayList::build_scrolled(tree, &children, scale, self.scroll);
        let mut scene = anyrender::Scene::new();
        use anyrender::PaintScene;
        scene.fill(
            peniko::Fill::NonZero,
            kurbo::Affine::IDENTITY,
            peniko::Color::WHITE,
            None,
            &kurbo::Rect::new(
                0.0,
                0.0,
                viewport.width_px as f64,
                viewport.height_px as f64,
            ),
        );
        dl.render_into(&mut scene);
        scene
    }

    pub fn resolved_value(
        &self,
        key: NodeKey,
        property: &str,
        out: &mut String,
    ) -> Result<(), StyleError> {
        let d = self.doc.borrow();
        if d.node(key).is_none() {
            return Err(StyleError::Stale(key));
        }
        let sheets_ref: Vec<&StyleSheet> = self.author_sheets.iter().collect();
        let computed = if let Some(b) = self.find_box(key) {
            b.style.clone()
        } else {
            let mut ancestors = Vec::new();
            let mut at = Some(key);
            while let Some(k) = at {
                ancestors.push(k);
                at = d.node(k).and_then(NodeView::parent);
            }
            let mut parent = None;
            for k in ancestors.into_iter().rev() {
                parent = Some(resolve_computed_style(&*d, k, parent.as_ref(), &sheets_ref));
            }
            parent.unwrap_or_default()
        };
        match property {
            "display" => out.push_str(&format!("{:?}", computed.display).to_lowercase()),
            "position" => out.push_str(&format!("{:?}", computed.position).to_lowercase()),
            "width" => out.push_str(&format!(
                "{:.1}px",
                self.find_box(key).map_or(0.0, |b| b.border_box.width)
            )),
            "height" => out.push_str(&format!(
                "{:.1}px",
                self.find_box(key).map_or(0.0, |b| b.border_box.height)
            )),
            "color" => out.push_str(&format!(
                "rgb({}, {}, {})",
                computed.color.r, computed.color.g, computed.color.b
            )),
            "background-color" => out.push_str(&format!(
                "rgb({}, {}, {})",
                computed.background_color.r,
                computed.background_color.g,
                computed.background_color.b
            )),
            "font-size" => out.push_str(&format!("{:.0}px", computed.font_size)),
            "line-height" => out.push_str(&format!("{:.0}px", computed.line_height)),
            _ => {}
        }
        Ok(())
    }
    pub fn inspect_geometry_report(
        &self,
        document: DocumentContext,
        key: NodeKey,
    ) -> wega_protocol::inspector::geometry::GeometryReport {
        let (state, set) = match self.inspect_geometry(key) {
            Geometry::Current(b) => (GeometryState::Current, Some(to_box_set(b))),
            Geometry::Stale(b) => (GeometryState::Stale, Some(to_box_set(b))),
            Geometry::Pending => (GeometryState::Pending, None),
            Geometry::NoBoxes => (GeometryState::NoBoxes, None),
            Geometry::Refused => (GeometryState::Refused, None),
            Geometry::Invalid => (GeometryState::Invalid, None),
        };
        let d = self.doc.borrow();
        let epoch = wega_dom::MutationClock::mutation_epoch(&*d).as_u64();
        let node_id = InspectorNodeId {
            document,
            key: key.as_u64(),
        };
        let viewport = ViewportReport {
            width: self.viewport.width_px as f64,
            height: self.viewport.height_px as f64,
            device_pixel_ratio: self.viewport.scale as f64,
            scroll_x: self.scroll.x,
            scroll_y: self.scroll.y,
        };
        let label = if let Some(n) = d.node(key) {
            let tag = n
                .element_name()
                .map(|e| e.local.to_string())
                .unwrap_or_default();
            let id = n.attr("id").unwrap_or_default().to_string();
            let classes = n.attr("class").unwrap_or_default().to_string();
            ElementLabel { tag, id, classes }
        } else {
            ElementLabel::default()
        };
        let mut paint = wega_protocol::inspector::geometry::PaintMapping::Exact { clip: None };
        let mut at = Some(key);
        while let Some(k) = at {
            if let Some(b) = self.find_box(k) {
                use wega_protocol::inspector::geometry::{PaintMapping, UnsupportedPaint};
                if b.style.transform.is_some() {
                    paint = PaintMapping::Unsupported {
                        reason: UnsupportedPaint::Transform,
                    };
                    break;
                }
                if b.scroll_offset != Point::default() {
                    paint = PaintMapping::Unsupported {
                        reason: UnsupportedPaint::Unknown,
                    };
                    break;
                }
                if b.style.overflow_x != super::css::Overflow::Visible
                    || b.style.overflow_y != super::css::Overflow::Visible
                {
                    if k != key {
                        let r = b.padding_box();
                        let r = GeomRect {
                            x: r.x - self.scroll.x,
                            y: r.y - self.scroll.y,
                            width: r.width,
                            height: r.height,
                        };
                        if let PaintMapping::Exact { clip } = &mut paint {
                            *clip = Some(if let Some(c) = *clip {
                                let x = r.x.max(c.x);
                                let y = r.y.max(c.y);
                                GeomRect {
                                    x,
                                    y,
                                    width: (r.x + r.width).min(c.x + c.width).max(x) - x,
                                    height: (r.y + r.height).min(c.y + c.height).max(y) - y,
                                }
                            } else {
                                r
                            });
                        }
                    }
                }
            }
            at = d.node(k).and_then(NodeView::parent);
        }
        GeometryReport {
            document,
            epoch,
            node: node_id,
            state,
            boxes: set,
            viewport,
            paint,
            label,
        }
    }

    pub fn pick_node_report(
        &self,
        document: DocumentContext,
        point: Point,
    ) -> Result<PickReport, wega_protocol::inspector::InspectorError> {
        let hit_key = self.element_at(point).ok_or_else(|| {
            wega_protocol::inspector::InspectorError::new(
                wega_protocol::inspector::InspectorErrorCode::Unavailable,
                "Nothing at that point",
            )
        })?;
        let d = self.doc.borrow();
        let Some(node) = d.node(hit_key) else {
            return Err(wega_protocol::inspector::InspectorError::new(
                wega_protocol::inspector::InspectorErrorCode::StaleNode,
                "Hit node was dropped",
            ));
        };
        let mut chain = Vec::new();
        let mut at = node.parent();
        while let Some(parent) = at {
            if chain.len() > 32 {
                chain.clear();
                break;
            }
            chain.push(InspectorNodeId {
                document,
                key: parent.as_u64(),
            });
            at = d.node(parent).and_then(|n| n.parent());
        }
        chain.reverse();
        drop(d);
        Ok(PickReport {
            target: self.inspect_geometry_report(document, hit_key),
            ancestors: chain,
        })
    }
}

fn find_box_in_tree(box_node: &LayoutBox, key: NodeKey) -> Option<&LayoutBox> {
    if box_node.node == Some(key) {
        return Some(box_node);
    }
    for child in &box_node.children {
        if let Some(found) = find_box_in_tree(child, key) {
            return Some(found);
        }
    }
    None
}

impl NativeDocument {
    pub fn scroll_node_to(&mut self, key: NodeKey, point: Point) -> Result<(), LayoutError> {
        self.ensure_layout();
        fn find(b: &mut LayoutBox, k: NodeKey) -> Option<&mut LayoutBox> {
            if b.node == Some(k) {
                return Some(b);
            }
            for c in &mut b.children {
                if let Some(b) = find(c, k) {
                    return Some(b);
                }
            }
            None
        }
        let b = self
            .layout_tree
            .as_mut()
            .and_then(|b| find(b, key))
            .ok_or(LayoutError::Stale(key))?;
        let client = b.padding_box();
        b.scroll_offset = Point {
            x: point.x.clamp(0.0, (b.scroll_width - client.width).max(0.0)),
            y: point
                .y
                .clamp(0.0, (b.scroll_height - client.height).max(0.0)),
        };
        Ok(())
    }
    pub fn scroll_viewport_to(&mut self, point: Point) {
        self.ensure_layout();
        let width =
            self.viewport.width_px as f64 / (self.viewport.scale * self.viewport.zoom) as f64;
        let height =
            self.viewport.height_px as f64 / (self.viewport.scale * self.viewport.zoom) as f64;
        let size = self
            .layout_tree
            .as_ref()
            .map(|b| (b.scroll_width, b.scroll_height))
            .unwrap_or_default();
        self.scroll = Point {
            x: point.x.clamp(0.0, (size.0 - width).max(0.0)),
            y: point.y.clamp(0.0, (size.1 - height).max(0.0)),
        };
    }
    pub fn scroll_by_at(&mut self, key: NodeKey, dx: f64, dy: f64) {
        let mut at = Some(key);
        while let Some(k) = at {
            if let Some(b) = self.find_box(k) {
                let client = b.padding_box();
                let movable_x = matches!(
                    b.style.overflow_x,
                    super::css::Overflow::Auto | super::css::Overflow::Scroll
                ) && b.scroll_width > client.width;
                let movable_y = matches!(
                    b.style.overflow_y,
                    super::css::Overflow::Auto | super::css::Overflow::Scroll
                ) && b.scroll_height > client.height;
                if movable_x || movable_y {
                    let point = Point {
                        x: b.scroll_offset.x + if movable_x { dx } else { 0.0 },
                        y: b.scroll_offset.y + if movable_y { dy } else { 0.0 },
                    };
                    let before = b.scroll_offset;
                    let _ = self.scroll_node_to(k, point);
                    if self.find_box(k).is_some_and(|b| b.scroll_offset != before) {
                        return;
                    }
                }
            }
            at = self.doc.borrow().node(k).and_then(NodeView::parent);
        }
        self.scroll_viewport_to(Point {
            x: self.scroll.x + dx,
            y: self.scroll.y + dy,
        });
    }
}
