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
use super::css::{MediaContext, StyleSheet, resolve_computed_style};
use super::display_list::DisplayList;
use super::layout::{LayoutBox, LayoutContext, build_box_tree, perform_layout};
use crate::document::WegaDocument;
use std::cell::RefCell;
use std::rc::Rc;
use wega_dom::layout::*;
use wega_dom::style::StyleError;
use wega_dom::{NodeKey, NodeKind, NodeView};
use wega_native::resources::NativeResources;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderingBackend {
    #[default]
    WegaNative,
}

impl RenderingBackend {
    pub fn from_env() -> Self {
        Self::WegaNative
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeTimings {
    pub stylesheet_parse_ms: f64,
    pub style_and_box_tree_ms: f64,
    pub layout_ms: f64,
    pub shaping_ms: f64,
    pub display_list_ms: f64,
    pub scene_ms: f64,
}
#[derive(PartialEq)]
struct SheetCacheKey {
    generation: u64,
    resources: u64,
    policy: usize,
    owners: Vec<(NodeKey, String, String, String, String)>,
}
struct GeometryCache {
    generation: u64,
    scroll: (u64, u64),
    list: DisplayList,
    rects: std::collections::HashMap<NodeKey, Vec<Rect>>,
    transforms: std::collections::HashMap<NodeKey, kurbo::Affine>,
}
pub struct NativeDocument<D: NativeResources = WegaDocument> {
    pub doc: Rc<RefCell<D>>,
    pub layout_tree: Option<LayoutBox>,
    pub author_sheets: Vec<StyleSheet>,
    pub viewport: ViewportConfig,
    pub scroll: Point,
    pub layout_dirty: bool,
    pub layout_refused: bool,
    seen_epoch: Option<u64>,
    seen_resources: u64,
    seen_state: u64,
    seen_viewport: Option<(u32, u32, u32, u32, bool)>,
    pub layout_generation: u64,
    pub timings: NativeTimings,
    pub font_source: Option<std::sync::Arc<Vec<Vec<u8>>>>,
    pub csp: Option<std::sync::Arc<std::sync::RwLock<wega_security::csp::Csp>>>,
    seen_policy_generation: usize,
    sheet_cache_key: Option<SheetCacheKey>,
    geometry_cache: RefCell<Option<GeometryCache>>,
    seen_focus: Option<NodeKey>,
}

impl<D: NativeResources> NativeDocument<D> {
    pub fn media_context(&self) -> MediaContext {
        let v = self.viewport;
        let scale = (v.scale * v.zoom) as f64;
        let scale = if scale > 0.0 { scale } else { 1.0 };
        MediaContext {
            viewport_width: v.width_px as f64 / scale,
            viewport_height: v.height_px as f64 / scale,
            device_pixel_ratio: scale,
            dark_mode: v.dark,
        }
    }

    pub fn register_font_bytes(
        &self,
        family_name: &str,
        bytes: &[u8],
        weight: Option<u16>,
        style: Option<parley::fontique::FontStyle>,
    ) {
        use parley::fontique::{Blob, FontInfoOverride, FontWeight};
        let font = Blob::new(std::sync::Arc::new(bytes.to_vec()) as _);
        let info = FontInfoOverride {
            family_name: Some(family_name),
            weight: weight.map(|w| FontWeight::new(w as f32)),
            style,
            ..Default::default()
        };
        self.doc
            .borrow()
            .native_fonts()
            .lock()
            .unwrap()
            .collection
            .register_fonts(font, Some(info));
        super::text::register_font_bytes(family_name, bytes, weight, style);
    }

    pub fn new(doc: Rc<RefCell<D>>) -> Self {
        Self::with_font_source(doc, None)
    }
    pub fn with_font_source(
        doc: Rc<RefCell<D>>,
        font_source: Option<std::sync::Arc<Vec<Vec<u8>>>>,
    ) -> Self {
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
            layout_refused: false,
            seen_epoch: None,
            seen_resources: 0,
            seen_state: 0,
            seen_viewport: None,
            layout_generation: 0,
            timings: NativeTimings::default(),
            font_source,
            csp: None,
            seen_policy_generation: 0,
            sheet_cache_key: None,
            geometry_cache: RefCell::new(None),
            seen_focus: None,
        }
    }

    pub fn invalidate(&mut self) {
        self.layout_dirty = true;
    }

    pub fn sync_author_stylesheets(&mut self) {
        let d = self.doc.borrow();
        let mut owners = Vec::new();
        let mut stack = vec![d.root()];
        while let Some(k) = stack.pop() {
            if let Some(n) = d.node(k) {
                if n.element_name()
                    .is_some_and(|n| matches!(n.local, "style" | "link"))
                {
                    owners.push((
                        k,
                        n.attr("media").unwrap_or("").to_owned(),
                        n.attr("nonce").unwrap_or("").to_owned(),
                        n.attr("href").unwrap_or("").to_owned(),
                        n.attr("rel").unwrap_or("").to_owned(),
                    ));
                }
                stack.extend(n.children().rev());
            }
        }
        let key = SheetCacheKey {
            generation: d.native_generations().0,
            resources: d.native_generations().1,
            policy: self.policy_generation(),
            owners,
        };
        if self.sheet_cache_key.as_ref() == Some(&key) {
            return;
        }
        self.sheet_cache_key = Some(key);
        self.author_sheets = d.native_stylesheets(&|nonce, css| {
            self.csp.as_ref().is_none_or(|csp| {
                csp.read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .allows_inline("style-src-elem", nonce, css)
            })
        });
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
    fn policy_generation(&self) -> usize {
        self.csp
            .as_ref()
            .map(|c| {
                c.read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .generation()
            })
            .unwrap_or(0)
    }
    pub fn is_dirty(&self) -> bool {
        self.layout_dirty
            || self.seen_state != self.doc.borrow().native_state_generation()
            || self.seen_focus != wega_dom::input::FocusState::focused(&*self.doc.borrow())
            || self.seen_resources != self.doc.borrow().native_generations().1
            || self.seen_policy_generation != self.policy_generation()
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
        self.geometry_cache.borrow_mut().take();
        super::text::configure_fonts(self.font_source.clone());
        super::text::sync_font_context(self.doc.borrow().native_fonts());
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
        self.doc.borrow_mut().prepare_controls();
        let start = std::time::Instant::now();
        self.sync_author_stylesheets();
        self.timings.stylesheet_parse_ms = start.elapsed().as_secs_f64() * 1000.0;
        let style_start = std::time::Instant::now();
        let mut d = self.doc.borrow_mut();
        let root_key = d.root();
        let sheets_ref: Vec<&StyleSheet> = self.author_sheets.iter().collect();

        let media_ctx = self.media_context();
        if let Some(mut box_tree) =
            build_box_tree(&*d, root_key, None, &sheets_ref, Some(&media_ctx))
        {
            d.native_images(&mut box_tree);
            d.sync_controls(&mut box_tree);
            self.timings.style_and_box_tree_ms = style_start.elapsed().as_secs_f64() * 1000.0;
            let layout_start = std::time::Instant::now();
            let shaping_start = super::text::shaping_nanos();
            let ctx = LayoutContext {
                viewport_width: self.viewport.width_px as f64
                    / (self.viewport.scale * self.viewport.zoom) as f64,
                viewport_height: self.viewport.height_px as f64
                    / (self.viewport.scale * self.viewport.zoom) as f64,
                font_size_root: box_tree
                    .children
                    .iter()
                    .find(|b| b.is_root)
                    .map_or(16.0, |b| b.style.font_size),
            };
            perform_layout(&mut box_tree, &ctx);
            d.decorate_controls(&mut box_tree);
            self.timings.layout_ms = layout_start.elapsed().as_secs_f64() * 1000.0;
            self.timings.shaping_ms = (super::text::shaping_nanos() - shaping_start) as f64 / 1e6;
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
            self.layout_refused = false;
        } else {
            self.layout_tree = None;
            self.layout_refused = true;
        }
        self.seen_focus = wega_dom::input::FocusState::focused(&*d);
        self.seen_resources = d.native_generations().1;
        self.seen_state = d.native_state_generation();
        self.seen_epoch = Some(wega_dom::MutationClock::mutation_epoch(&*d).as_u64());
        self.seen_viewport = Some(self.viewport_key());
        self.layout_generation = self.layout_generation.wrapping_add(1);
        self.seen_policy_generation = self.policy_generation();
        self.layout_dirty = false;
    }

    pub fn find_box(&self, key: NodeKey) -> Option<&LayoutBox> {
        self.layout_tree
            .as_ref()
            .and_then(|tree| find_box_in_tree(tree, key))
    }

    fn geometry(&self) -> std::cell::Ref<'_, GeometryCache> {
        let scroll = (self.scroll.x.to_bits(), self.scroll.y.to_bits());
        let rebuild = self
            .geometry_cache
            .borrow()
            .as_ref()
            .is_none_or(|c| c.generation != self.layout_generation || c.scroll != scroll);
        if rebuild {
            let list = self
                .layout_tree
                .as_ref()
                .map(|tree| DisplayList::build_scrolled(tree, &[], 1.0, self.scroll))
                .unwrap_or_default();
            let (rects, transforms) = list.geometry_snapshot();
            *self.geometry_cache.borrow_mut() = Some(GeometryCache {
                generation: self.layout_generation,
                scroll,
                list,
                rects,
                transforms,
            });
        }
        std::cell::Ref::map(self.geometry_cache.borrow(), |c| c.as_ref().unwrap())
    }
    pub fn bounding_rect(&self, key: NodeKey) -> Result<Rect, LayoutError> {
        let rects = self.client_rects(key)?;
        if !rects.is_empty() {
            return Ok(rects
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
                .unwrap_or(Rect::ZERO));
        }
        if let Some(b) = self.find_box(key) {
            return Ok(b.border_box);
        }
        Ok(Rect::ZERO)
    }

    pub fn point_in_content(&self, key: NodeKey, p: Point) -> Option<Point> {
        self.layout_tree.as_ref()?;
        let b = self.find_box(key)?;
        let t = *self.geometry().transforms.get(&key)?;
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
        Ok(self.geometry().rects.get(&key).cloned().unwrap_or_default())
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
        } else if self.layout_refused {
            Geometry::Refused
        } else {
            Geometry::NoBoxes
        }
    }

    pub fn element_at(&self, point: Point) -> Option<NodeKey> {
        let key = self.geometry().list.hit_test(point)?;
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
        let start = std::time::Instant::now();
        let dl = DisplayList::build_scrolled(tree, &children, scale, self.scroll);
        self.timings.display_list_ms = start.elapsed().as_secs_f64() * 1000.0;
        let start = std::time::Instant::now();
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
        // CSS canvas background propagates from the root, or the body when
        // the root is transparent, including otherwise empty child documents.
        let backdrop = {
            let doc = self.doc.borrow();
            let mut html = None;
            let mut body = None;
            let mut stack = vec![tree];
            while let Some(b) = stack.pop() {
                if let Some(name) = b
                    .node
                    .and_then(|k| doc.node(k))
                    .and_then(NodeView::element_name)
                {
                    if name.local == "html" {
                        html = Some(b.style.background_color);
                    }
                    if name.local == "body" {
                        body = Some(b.style.background_color);
                    }
                }
                stack.extend(&b.children);
            }
            html.filter(|c| c.a > 0).or(body)
        };
        if let Some(c) = backdrop.filter(|c| c.a > 0) {
            scene.fill(
                peniko::Fill::NonZero,
                kurbo::Affine::IDENTITY,
                peniko::Color::from_rgba8(c.r, c.g, c.b, c.a),
                None,
                &kurbo::Rect::new(
                    0.0,
                    0.0,
                    viewport.width_px as f64,
                    viewport.height_px as f64,
                ),
            );
        }
        dl.render_into(&mut scene);
        self.timings.scene_ms = start.elapsed().as_secs_f64() * 1000.0;
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
                parent = Some(resolve_computed_style(
                    &*d,
                    k,
                    parent.as_ref(),
                    &sheets_ref,
                    Some(&self.media_context()),
                ));
            }
            parent.unwrap_or_default()
        };
        let used = self.find_box(key);
        let value = match (property, used) {
            ("width", Some(b)) => Some(format!(
                "{}px",
                if computed.box_sizing == super::css::BoxSizing::BorderBox {
                    b.border_box.width
                } else {
                    b.content_box().width
                }
            )),
            ("height", Some(b)) => Some(format!(
                "{}px",
                if computed.box_sizing == super::css::BoxSizing::BorderBox {
                    b.border_box.height
                } else {
                    b.content_box().height
                }
            )),
            ("padding-top", Some(b)) => Some(format!("{}px", b.padding.top)),
            ("padding-right", Some(b)) => Some(format!("{}px", b.padding.right)),
            ("padding-bottom", Some(b)) => Some(format!("{}px", b.padding.bottom)),
            ("padding-left", Some(b)) => Some(format!("{}px", b.padding.left)),
            ("margin-top", Some(b)) => Some(format!("{}px", b.margin.top)),
            ("margin-right", Some(b)) => Some(format!("{}px", b.margin.right)),
            ("margin-bottom", Some(b)) => Some(format!("{}px", b.margin.bottom)),
            ("margin-left", Some(b)) => Some(format!("{}px", b.margin.left)),
            _ => wega_native::computed::scalar(&computed, property),
        };
        if let Some(value) = value {
            out.push_str(&value);
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
        fn bounded(value: &str, limit: usize) -> String {
            let mut end = value.len().min(limit);
            while !value.is_char_boundary(end) {
                end -= 1;
            }
            value[..end].to_owned()
        }
        let label = if let Some(n) = d.node(key) {
            let tag = n
                .element_name()
                .map(|e| bounded(e.local, MAX_LABEL_TAG))
                .unwrap_or_default();
            let id = bounded(n.attr("id").unwrap_or_default(), MAX_LABEL_ID);
            let classes = bounded(n.attr("class").unwrap_or_default(), MAX_LABEL_CLASSES);
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
            if chain.len() >= MAX_ANCESTORS {
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

impl<D: NativeResources> NativeDocument<D> {
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
            x: if b.style.overflow_x == super::css::Overflow::Clip {
                0.0
            } else {
                point.x.clamp(0.0, (b.scroll_width - client.width).max(0.0))
            },
            y: if b.style.overflow_y == super::css::Overflow::Clip {
                0.0
            } else {
                point
                    .y
                    .clamp(0.0, (b.scroll_height - client.height).max(0.0))
            },
        };
        self.geometry_cache.borrow_mut().take();
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
