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
use super::layout::{LayoutBox, LayoutContext, build_box_tree, hit_test_node, perform_layout};
use std::cell::RefCell;
use std::rc::Rc;
use wega_adapter_blitz::BlitzDocument;
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
                if node.kind() == NodeKind::Element {
                    if let Some(elem) = node.element_name() {
                        if elem.local.eq_ignore_ascii_case("style") {
                            // Extract text from children
                            let mut css_text = String::new();
                            for child_k in node.children() {
                                if let Some(child_node) = d.node(child_k) {
                                    if let Some(t) = child_node.text() {
                                        css_text.push_str(t);
                                    }
                                }
                            }
                            if !css_text.is_empty() {
                                sheets.push(StyleSheet::parse(&css_text));
                            }
                        }
                    }
                }
                for c in node.children() {
                    stack.push(c);
                }
            }
        }
        self.author_sheets = sheets;
    }

    pub fn flush_layout(&mut self) {
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
            self.layout_tree = Some(box_tree);
        } else {
            self.layout_tree = None;
        }
        self.layout_dirty = false;
    }

    pub fn find_box(&self, key: NodeKey) -> Option<&LayoutBox> {
        self.layout_tree
            .as_ref()
            .and_then(|tree| find_box_in_tree(tree, key))
    }

    pub fn bounding_rect(&self, key: NodeKey) -> Result<Rect, LayoutError> {
        let b = self.find_box(key).ok_or(LayoutError::Stale(key))?;
        let mut r = b.border_box;
        r.x -= self.scroll.x;
        r.y -= self.scroll.y;
        Ok(r)
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
            if self.layout_dirty {
                Geometry::Stale(bm)
            } else {
                Geometry::Current(bm)
            }
        } else {
            Geometry::NoBoxes
        }
    }

    pub fn element_at(&self, point: Point) -> Option<NodeKey> {
        let doc_point = Point {
            x: point.x + self.scroll.x,
            y: point.y + self.scroll.y,
        };
        self.layout_tree
            .as_ref()
            .and_then(|tree| hit_test_node(tree, doc_point))
    }

    pub fn record_scene(&mut self, viewport: &ViewportConfig) -> anyrender::Scene {
        self.record_scene_with_children(viewport, Vec::new())
    }

    pub fn record_scene_with_children(
        &mut self,
        viewport: &ViewportConfig,
        children: Vec<(NodeKey, anyrender::Scene)>,
    ) -> anyrender::Scene {
        if self.layout_dirty || self.layout_tree.is_none() {
            self.flush_layout();
        }
        let scale = (viewport.scale * viewport.zoom) as f64;
        let Some(tree) = &self.layout_tree else {
            return anyrender::Scene::new();
        };
        let dl = DisplayList::build(tree, &children, scale);
        dl.to_anyrender_scene()
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
        let computed = resolve_computed_style(&*d, key, None, &sheets_ref);
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
        GeometryReport {
            document,
            epoch,
            node: node_id,
            state,
            boxes: set,
            viewport,
            paint: wega_protocol::inspector::geometry::PaintMapping::Exact { clip: None },
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
