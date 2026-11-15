//! The only mapping from Blitz's document onto the Wega document contract (`wega-dom`).
#![forbid(unsafe_code)]
use blitz_dom::{
    BaseDocument, DocumentConfig, DocumentMutator, Node, NodeId, node::Attribute, node::NodeData,
};
use blitz_html::HtmlDocument;
use blitz_traits::{
    navigation::NavigationProvider,
    net::{AbortSignal, NetProvider},
};
use html5ever::{LocalName, QualName, ns};
use url::Url;
use wega_dom::input::{FocusState, HoverChange, InputControl};
use wega_dom::layout::{
    ClientBox, ContentBox, GeometryAccess, HitTest, LayoutControl, LayoutError, OffsetBox, Point,
    Rect, Size, ViewportConfig, ViewportGeometry,
};
use wega_dom::render::PaintSource;
use wega_dom::style::{StyleAccess, StyleControl, StyleError};
use wega_dom::{
    DocumentMutation, DocumentView, MutationClock, MutationEpoch, MutationError, Mutator,
    NewAttribute, NodeKey, NodeKind, NodeView,
};

/// `NodeKey` and Blitz's `NodeId` are both foreign to this module's eventual crate, so they
/// cannot implement `From` for each other; these explicit conversions replace that. Both are
/// the same `u64`, so they compile to nothing.
pub trait ToKey {
    fn key(self) -> NodeKey;
}
pub trait ToNodeId {
    fn node_id(self) -> NodeId;
}
impl ToKey for NodeId {
    #[inline(always)]
    fn key(self) -> NodeKey {
        NodeKey::from_u64(self.as_u64())
    }
}
impl ToNodeId for NodeKey {
    #[inline(always)]
    fn node_id(self) -> NodeId {
        NodeId::from_u64(self.as_u64())
    }
}

/// The document the engine owns: Blitz's `BaseDocument`, held by value (no second tree, no
/// extra allocation or indirection) in a local type so the Wega contract can be implemented
/// for it. It dereferences to `BaseDocument`, so the layout, paint, style and form APIs that
/// are not behind the contract yet stay reachable.
pub struct BlitzDocument(BaseDocument, u64);
impl BlitzDocument {
    pub fn new(document: BaseDocument) -> Self {
        Self(document, 0)
    }
    pub fn into_inner(self) -> BaseDocument {
        self.0
    }
    /// Record that a state-changing operation was applied to the document *outside* the Wega
    /// mutation contract (a Wega-owned wrapper over a Blitz API reached through `DerefMut`, such
    /// as a future stylesheet-rule edit). Blitz does not bump its stylesheet generation for edits
    /// inside an existing sheet, so such a wrapper must call this once per successful change.
    pub fn record_external_mutation(&mut self) {
        self.1 = self.1.wrapping_add(1);
    }
    /// Start the mutation counter at `epoch`. Only for tests of its wrap-around behaviour.
    #[doc(hidden)]
    pub fn with_mutation_epoch(mut self, epoch: u64) -> Self {
        self.1 = epoch;
        self
    }
}

impl MutationClock for BlitzDocument {
    /// The adapter's own count of successful mutations through the Wega contract, plus Blitz's
    /// stylesheet-set generation, so CSSOM changes (which also reach the document through
    /// `DerefMut`) are covered too. Both only ever step forward, so their sum changes whenever
    /// either does.
    fn mutation_epoch(&self) -> MutationEpoch {
        MutationEpoch::from_u64(self.1.wrapping_add(self.0.stylesheet_generation()))
    }
}
impl From<BaseDocument> for BlitzDocument {
    fn from(document: BaseDocument) -> Self {
        Self(document, 0)
    }
}
impl std::ops::Deref for BlitzDocument {
    type Target = BaseDocument;
    #[inline(always)]
    fn deref(&self) -> &BaseDocument {
        &self.0
    }
}
impl std::ops::DerefMut for BlitzDocument {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut BaseDocument {
        &mut self.0
    }
}

/// What the engine decides about a new document. The adapter turns it into Blitz's own
/// configuration, so the engine neither builds a `DocumentConfig` nor chooses how a
/// `BaseDocument` is constructed.
#[derive(Default)]
pub struct DocumentSetup {
    /// The URL relative references resolve against.
    pub base_url: Option<String>,
    /// Font file bytes supplied by the browser process (brokered fonts). `None` uses the
    /// system fonts.
    pub fonts: Option<std::sync::Arc<Vec<Vec<u8>>>>,
    /// Cancelled when the document is replaced, to stop its in-flight resource loads.
    pub abort_signal: Option<AbortSignal>,
    pub net_provider: Option<std::sync::Arc<dyn NetProvider>>,
    pub navigation_provider: Option<std::sync::Arc<dyn NavigationProvider>>,
}
impl DocumentSetup {
    fn config(self) -> DocumentConfig {
        DocumentConfig {
            font_ctx: self
                .fonts
                .as_ref()
                .map(|fonts| blitz_dom::build_broker_font_ctx(fonts)),
            base_url: self.base_url,
            abort_signal: self.abort_signal,
            net_provider: self.net_provider,
            navigation_provider: self.navigation_provider,
            ..Default::default()
        }
    }
}
impl BlitzDocument {
    /// A document parsed from `html` in one pass (the engine's initial blank page).
    pub fn from_html(html: &str, setup: DocumentSetup) -> Self {
        Self(
            HtmlDocument::from_html(html, setup.config()).into_inner(),
            0,
        )
    }
    /// A document parsed from XML in one pass.
    pub fn from_xml(xml: &str, setup: DocumentSetup) -> Self {
        Self(HtmlDocument::from_xml(xml, setup.config()).into_inner(), 0)
    }
    /// An empty document that the incremental HTML parser then fills.
    pub fn empty(setup: DocumentSetup) -> Self {
        Self(BaseDocument::new(setup.config()), 0)
    }
}

/// A borrowed node of a [`BlitzDocument`].
#[derive(Clone, Copy)]
pub struct BlitzNode<'a>(&'a Node);
impl<'a> BlitzNode<'a> {
    /// The underlying Blitz node, for the properties that are not behind the contract yet.
    pub fn blitz(self) -> &'a Node {
        self.0
    }
}

impl DocumentView for BlitzDocument {
    type Node<'a> = BlitzNode<'a>;
    #[inline]
    fn node(&self, key: NodeKey) -> Option<BlitzNode<'_>> {
        self.0.get_node(key.node_id()).map(BlitzNode)
    }
    fn element_by_id(&self, id: &str) -> Option<NodeKey> {
        self.0.get_element_by_id(id).map(ToKey::key)
    }
    fn root(&self) -> NodeKey {
        self.0.root_node().id.key()
    }
    fn base_url(&self) -> &Url {
        self.0.url()
    }
}

impl<'a> NodeView<'a> for BlitzNode<'a> {
    #[inline]
    fn kind(self) -> NodeKind {
        match &self.0.data {
            NodeData::Document(_) => NodeKind::Document,
            NodeData::Element(_) => NodeKind::Element,
            NodeData::AnonymousBlock(_) => NodeKind::AnonymousBlock,
            NodeData::Text(_) => NodeKind::Text,
            NodeData::Comment { .. } => NodeKind::Comment,
        }
    }
    #[inline]
    fn qual_name(self) -> Option<&'a QualName> {
        match &self.0.data {
            NodeData::Element(e) | NodeData::AnonymousBlock(e) => Some(&e.name),
            _ => None,
        }
    }
    #[inline]
    fn attr(self, name: &str) -> Option<&'a str> {
        let element = self.0.element_data()?;
        element
            .attrs
            .iter()
            .find(|attr| &*attr.name.local == name)
            .map(|attr| &*attr.value)
    }
    #[inline(always)]
    fn parent(self) -> Option<NodeKey> {
        self.0.parent.map(ToKey::key)
    }
    #[inline]
    fn children(self) -> impl ExactSizeIterator<Item = NodeKey> + DoubleEndedIterator + 'a {
        self.0.children.iter().copied().map(ToKey::key)
    }
    #[inline]
    fn child_at(self, index: usize) -> Option<NodeKey> {
        self.0.children.get(index).copied().map(ToKey::key)
    }
    fn attributes(self) -> impl Iterator<Item = (&'a QualName, &'a str)> + 'a {
        self.0
            .element_data()
            .into_iter()
            .flat_map(|element| element.attrs.iter())
            .map(|attr| (&attr.name, &*attr.value))
    }
}

impl DocumentMutation for BlitzDocument {
    type Mutator<'a> = BlitzMutator<'a>;
    fn mutate(&mut self) -> BlitzMutator<'_> {
        BlitzMutator(BaseDocument::mutate(&mut self.0), &mut self.1)
    }
    fn refresh_style_element(&mut self, key: NodeKey) {
        // A `<style>` outside the document (a fragment, a template, a shadow tree) has no
        // stylesheet; it gets one when it is inserted.
        let id = key.node_id();
        if self
            .0
            .get_node(id)
            .is_some_and(|node| node.flags.is_in_document())
        {
            self.0.process_style_element(id);
        }
    }
}

impl FocusState for BlitzDocument {
    fn focused(&self) -> Option<NodeKey> {
        // Blitz reports the root element when nothing is focused; that fallback is the
        // keyboard target, not focus.
        let focused = self.0.get_focussed_node_id()?;
        let root = self.0.try_root_element().map(|element| element.id);
        (Some(focused) != root).then(|| focused.key())
    }
    fn keyboard_target(&self) -> Option<NodeKey> {
        self.0.get_focussed_node_id().map(ToKey::key)
    }
    fn hovered(&self) -> Option<NodeKey> {
        self.0
            .get_hover_node_id()
            .filter(|id| self.0.get_node(*id).is_some())
            .map(ToKey::key)
    }
}
impl InputControl for BlitzDocument {
    fn set_focus(&mut self, key: NodeKey) -> Result<bool, MutationError> {
        if self.0.get_node(key.node_id()).is_none() {
            return Err(MutationError::Stale(key));
        }
        Ok(self.0.set_focus_to(key.node_id()))
    }
    fn clear_focus(&mut self) {
        self.0.clear_focus();
    }
    fn pointer_moved(&mut self, x: f32, y: f32) -> HoverChange {
        let previous = self.hovered();
        // Blitz hit-tests in document space; the contract takes viewport coordinates.
        let scroll = self.0.viewport_scroll();
        self.0
            .set_hover_to(x + scroll.x as f32, y + scroll.y as f32);
        HoverChange {
            previous,
            current: self.hovered(),
        }
    }
    fn pointer_left(&mut self) -> HoverChange {
        let previous = self.hovered();
        self.0.clear_hover();
        HoverChange {
            previous,
            current: self.hovered(),
        }
    }
}

fn rect(r: blitz_dom::BoundingRect) -> Rect {
    Rect {
        x: r.x,
        y: r.y,
        width: r.width,
        height: r.height,
    }
}
impl BlitzDocument {
    fn laid_out(&self, key: NodeKey) -> Result<&Node, LayoutError> {
        self.0
            .get_node(key.node_id())
            .ok_or(LayoutError::Stale(key))
    }
}
impl GeometryAccess for BlitzDocument {
    fn has_boxes(&self, key: NodeKey) -> Result<bool, LayoutError> {
        let node = self.laid_out(key)?;
        // Blitz checks only the node's own `display`; an element below a `display: none`
        // ancestor is never styled, so it has no primary style either.
        Ok(node.has_boxes() && (!node.is_element() || node.primary_styles().is_some()))
    }
    fn bounding_rect(&self, key: NodeKey) -> Result<Rect, LayoutError> {
        let node = self.laid_out(key)?;
        if node.element_data().is_none() || !self.has_boxes(key)? {
            return Ok(Rect::ZERO);
        }
        Ok(self
            .0
            .get_client_bounding_rect(key.node_id())
            .map_or(Rect::ZERO, rect))
    }
    fn client_rects(&self, key: NodeKey, out: &mut Vec<Rect>) -> Result<(), LayoutError> {
        if self.has_boxes(key)? {
            out.extend(
                self.0
                    .node_client_rects(key.node_id())
                    .into_iter()
                    .map(rect),
            );
        }
        Ok(())
    }
    fn client_box(&self, key: NodeKey) -> Result<ClientBox, LayoutError> {
        let node = self.laid_out(key)?;
        if !self.has_boxes(key)? {
            return Ok(ClientBox::default());
        }
        let layout = node.final_layout();
        Ok(ClientBox {
            left: layout.border.left as f64,
            top: layout.border.top as f64,
            width: node.client_width() as f64,
            height: node.client_height() as f64,
        })
    }
    fn offset_box(&self, key: NodeKey) -> Result<OffsetBox, LayoutError> {
        let node = self.laid_out(key)?;
        if !self.has_boxes(key)? {
            return Ok(OffsetBox {
                rect: Rect::ZERO,
                parent: None,
            });
        }
        Ok(OffsetBox {
            rect: self.0.offset_rect(key.node_id()).map_or(Rect::ZERO, rect),
            parent: node.offset_parent().map(|parent| parent.id.key()),
        })
    }
    fn scroll_size(&self, key: NodeKey) -> Result<Size, LayoutError> {
        let node = self.laid_out(key)?;
        if !self.has_boxes(key)? {
            return Ok(Size::default());
        }
        Ok(Size {
            width: node.scroll_width() as f64,
            height: node.scroll_height() as f64,
        })
    }
    fn scroll_offset(&self, key: NodeKey) -> Result<Point, LayoutError> {
        let offset = self.laid_out(key)?.scroll_offset();
        Ok(Point {
            x: offset.x,
            y: offset.y,
        })
    }
    fn content_box(&self, key: NodeKey) -> Result<ContentBox, LayoutError> {
        let node = self.laid_out(key)?;
        if !self.has_boxes(key)? {
            return Ok(ContentBox::default());
        }
        let layout = node.final_layout();
        Ok(ContentBox {
            size: Size {
                width: layout.content_box_width() as f64,
                height: layout.content_box_height() as f64,
            },
            padding_left: layout.padding.left as f64,
            padding_top: layout.padding.top as f64,
        })
    }
    fn layout_pending(&self) -> bool {
        let root = self.0.root_node();
        // Before the first resolve no element has a style; afterwards pending work shows as
        // dirty descendants or damage on the root, and a document waiting on render-blocking
        // resources is not resolved at all.
        // `root_element()` unwraps, and an empty document (before parsing) has no element child.
        let unresolved = root
            .children
            .iter()
            .filter_map(|id| self.0.get_node(*id))
            .find(|node| node.is_element())
            .is_none_or(|element| element.primary_styles().is_none());
        unresolved
            || self.0.has_pending_critical_resources()
            || root.has_dirty_descendants()
            || root.damage().is_some_and(|damage| !damage.is_empty())
    }
    fn viewport(&self) -> ViewportGeometry {
        let viewport = self.0.viewport();
        let scale = (viewport.hidpi_scale * viewport.zoom) as f64;
        let scroll = self.0.viewport_scroll();
        ViewportGeometry {
            size: Size {
                width: viewport.window_size.0 as f64 / scale,
                height: viewport.window_size.1 as f64 / scale,
            },
            device_pixel_ratio: scale,
            scroll: Point {
                x: scroll.x,
                y: scroll.y,
            },
        }
    }
}
impl HitTest for BlitzDocument {
    fn element_at(&self, point: Point) -> Option<NodeKey> {
        let viewport = self.viewport();
        if point.x < 0.0
            || point.y < 0.0
            || point.x > viewport.size.width
            || point.y > viewport.size.height
        {
            return None;
        }
        // Blitz hit-tests in document space.
        let hit = self.0.hit(
            (point.x + viewport.scroll.x) as f32,
            (point.y + viewport.scroll.y) as f32,
        );
        let Some(hit) = hit else {
            return self.0.try_root_element().map(|root| root.id.key());
        };
        let mut id = self.0.nearest_non_anonymous_ancestor(hit.node_id)?;
        loop {
            let node = self.0.get_node(id)?;
            if node.is_element() {
                return Some(id.key());
            }
            id = node.parent?;
        }
    }
}
impl LayoutControl for BlitzDocument {
    fn flush_layout(&mut self, animation_time: f64) {
        self.0.resolve(animation_time);
    }
    fn set_viewport(&mut self, viewport: ViewportConfig) {
        self.0.set_viewport(blitz_traits::shell::Viewport {
            window_size: (viewport.width_px, viewport.height_px),
            hidpi_scale: viewport.scale,
            zoom: viewport.zoom,
            color_scheme: if viewport.dark {
                blitz_traits::shell::ColorScheme::Dark
            } else {
                blitz_traits::shell::ColorScheme::Light
            },
        });
    }
    fn set_viewport_scroll(&mut self, scroll: Point) {
        self.0.set_viewport_scroll(blitz_dom::Point {
            x: scroll.x,
            y: scroll.y,
        });
    }
    fn scroll_node_to(&mut self, key: NodeKey, scroll: Point) -> Result<(), LayoutError> {
        self.laid_out(key)?;
        self.0.scroll_to(
            key.node_id(),
            scroll.x,
            scroll.y,
            blitz_dom::ScrollBehavior::Instant,
        );
        Ok(())
    }
    fn scroll_node_into_view(&mut self, key: NodeKey) -> Result<(), LayoutError> {
        self.laid_out(key)?;
        self.0.scroll_into_view(
            key.node_id(),
            blitz_dom::ScrollBehavior::Instant,
            blitz_dom::ScrollLogicalPosition::Start,
            blitz_dom::ScrollLogicalPosition::Nearest,
        );
        Ok(())
    }
    fn scroll_to_fragment(&mut self, fragment: &str) -> bool {
        self.0.scroll_to_fragment(fragment)
    }
}

impl PaintSource for BlitzDocument {
    type Scene = anyrender::Scene;
    fn prepare_frame(&mut self, viewport: &ViewportConfig, animation_time: f64) {
        self.set_viewport(*viewport);
        // Large trees restyle on Stylo's thread pool; the hint is applied for painting only.
        let large = self.0.tree().len() >= 512;
        self.0.set_style_threading(if large {
            blitz_dom::StyleThreading::Parallel
        } else {
            blitz_dom::StyleThreading::Sequential
        });
        self.flush_layout(animation_time);
    }
    fn record_scene(&mut self, viewport: &ViewportConfig) -> Self::Scene {
        let mut scene = anyrender::Scene::new();
        blitz_paint::paint_scene(
            &mut scene,
            &mut self.0,
            (viewport.scale * viewport.zoom) as f64,
            viewport.width_px,
            viewport.height_px,
            0,
            0,
        );
        scene
    }
    fn is_animating(&self) -> bool {
        self.0.is_animating()
    }
}

impl BlitzDocument {
    /// The element behind `key`, for style reads.
    fn styled(&self, key: NodeKey) -> Result<&blitz_dom::ElementData, StyleError> {
        self.0
            .get_node(key.node_id())
            .ok_or(StyleError::Stale(key))?
            .element_data()
            .ok_or(StyleError::NotElement(key))
    }
}

/// Styles are Stylo's, hosted by the Blitz document: the inline declaration block lives on
/// the element, computed values in its Stylo data, and the stylist and device in the
/// document. Nothing is cached or copied here.
impl StyleAccess for BlitzDocument {
    fn inline_style(&self, key: NodeKey, out: &mut String) -> Result<(), StyleError> {
        if let Some(block) = self.styled(key)?.style_attribute.as_ref() {
            let guard = self.0.guard().read();
            let _ = block.read_with(&guard).to_css(out);
        }
        Ok(())
    }
    fn inline_property(
        &self,
        key: NodeKey,
        property: &str,
        out: &mut String,
    ) -> Result<(), StyleError> {
        let element = self.styled(key)?;
        if let Ok(property) = style::properties::PropertyId::parse_enabled_for_all_content(property)
            && let Some(block) = element.style_attribute.as_ref()
        {
            let guard = self.0.guard().read();
            let _ = block
                .read_with(&guard)
                .property_value_to_css(&property, out);
        }
        Ok(())
    }
    fn resolved_value(
        &self,
        key: NodeKey,
        property: &str,
        out: &mut String,
    ) -> Result<(), StyleError> {
        self.styled(key)?;
        out.push_str(&self.0.resolved_style_value(key.node_id(), property));
        Ok(())
    }
    fn supports_condition(&self, condition: &str) -> bool {
        self.0.css_supports_condition(condition)
    }
    fn supports_declaration(&self, property: &str, value: &str) -> bool {
        // A declaration is supported when it survives parsing into an empty block.
        self.0
            .style_attr_set_property("", property, value, false)
            .is_some_and(|css| !css.is_empty())
    }
}

impl StyleControl for BlitzDocument {
    fn set_inline_property(
        &mut self,
        key: NodeKey,
        property: &str,
        value: &str,
        important: bool,
    ) -> Result<bool, MutationError> {
        use style::properties::declaration_block::{
            Importance, SourcePropertyDeclarationUpdate, parse_one_declaration_into,
        };
        use style::properties::{PropertyId, SourcePropertyDeclaration};
        use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
        let element = match self.styled(key) {
            Ok(element) => element,
            Err(StyleError::Stale(key)) => return Err(MutationError::Stale(key)),
            Err(StyleError::NotElement(key)) => return Err(MutationError::NotElement(key)),
        };
        // Unknown property: CSSOM ignores the write.
        let Ok(property) = PropertyId::parse_enabled_for_all_content(property) else {
            return Ok(false);
        };
        // Work on a copy of the element's block and write it back through the `style`
        // attribute, which is how Blitz keeps the attribute and the block in step and
        // invalidates the element.
        let mut block = match element.style_attribute.as_ref() {
            Some(block) => block.read_with(&self.0.guard().read()).clone(),
            None => Default::default(),
        };
        let changed = if value.trim().is_empty() {
            match block.first_declaration_to_remove(&property) {
                Some(first) => {
                    block.remove_property(&property, first);
                    true
                }
                None => false,
            }
        } else {
            let mut source = SourcePropertyDeclaration::default();
            let url_data = UrlExtraData(style::servo_arc::Arc::new(self.0.url().clone()));
            // A value that does not parse (including one carrying `!important`) is ignored.
            if parse_one_declaration_into(
                &mut source,
                property,
                value,
                Origin::Author,
                &url_data,
                None,
                style_traits::ParsingMode::DEFAULT,
                style::context::QuirksMode::NoQuirks,
                CssRuleType::Style,
            )
            .is_err()
            {
                return Ok(false);
            }
            let importance = if important {
                Importance::Important
            } else {
                Importance::Normal
            };
            // CSSOM update: an existing declaration keeps its position.
            let mut updates = SourcePropertyDeclarationUpdate::default();
            let changed = block.prepare_for_update(&source, importance, &mut updates);
            if changed {
                block.update(source.drain(), importance, &mut updates);
            }
            changed
        };
        if !changed {
            return Ok(false);
        }
        let mut css = String::new();
        let _ = block.to_css(&mut css);
        self.mutate().set_attribute(
            key,
            QualName::new(None, ns!(), LocalName::from("style")),
            &css,
        )?;
        Ok(true)
    }
    fn media_matches(&mut self, query: &str) -> bool {
        use style::context::QuirksMode;
        use style::stylesheets::{CssRuleType, CustomMediaEvaluator, Origin, UrlExtraData};
        let url_data = UrlExtraData(style::servo_arc::Arc::new(self.0.url().clone()));
        let mut context = style::parser::ParserContext::new(
            Origin::Author,
            &url_data,
            Some(CssRuleType::Media),
            style_traits::ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            std::borrow::Cow::Owned(Default::default()),
            None,
            None,
            Default::default(),
        );
        let mut input = cssparser::Parser::new(query);
        let media = style::media_queries::MediaList::parse(&mut context, &mut input);
        media.evaluate(
            self.0.stylist_device(),
            QuirksMode::NoQuirks,
            &mut CustomMediaEvaluator::none(),
        )
    }
}

/// Blitz's `DocumentMutator` behind the Wega mutation contract. It has no `Drop` of its
/// own: dropping it drops the inner mutator, which runs Blitz's deferred work.
pub struct BlitzMutator<'a>(DocumentMutator<'a>, &'a mut u64);

fn attributes(attrs: impl IntoIterator<Item = NewAttribute>) -> Vec<Attribute> {
    attrs
        .into_iter()
        .map(|attr| Attribute {
            name: attr.name,
            value: attr.value,
        })
        .collect()
}

impl BlitzMutator<'_> {
    /// One successful, state-changing operation happened (see `MutationEpoch`).
    fn stepped(&mut self) {
        *self.1 = self.1.wrapping_add(1);
    }
    /// The element's current value of attribute `name`, if it has one.
    fn attribute_value(&self, key: NodeKey, name: &QualName) -> Option<&str> {
        let node = self.0.doc.get_node(key.node_id())?;
        node.element_data()?
            .attrs
            .iter()
            .find(|attr| attr.name == *name)
            .map(|attr| &*attr.value)
    }
    fn node(&self, key: NodeKey) -> Result<&Node, MutationError> {
        self.0
            .doc
            .get_node(key.node_id())
            .ok_or(MutationError::Stale(key))
    }
    fn live(&self, key: NodeKey) -> Result<NodeId, MutationError> {
        self.node(key).map(|_| key.node_id())
    }
    fn element(&self, key: NodeKey) -> Result<NodeId, MutationError> {
        match BlitzNode(self.node(key)?).kind() {
            NodeKind::Element | NodeKind::AnonymousBlock => Ok(key.node_id()),
            _ => Err(MutationError::NotElement(key)),
        }
    }
    fn container(&self, key: NodeKey) -> Result<NodeId, MutationError> {
        match BlitzNode(self.node(key)?).kind() {
            NodeKind::Text | NodeKind::Comment => Err(MutationError::InvalidParent(key)),
            _ => Ok(key.node_id()),
        }
    }
    /// Rejects stale children and any child that is `parent` or one of its ancestors.
    /// A childless node can only be an ancestor of `parent` by being `parent` itself, so the
    /// ancestor walk runs only for children that have children.
    fn check_children(&self, parent: NodeKey, children: &[NodeKey]) -> Result<(), MutationError> {
        for &child in children {
            let node = self.node(child)?;
            let cycle =
                child == parent || (!node.children.is_empty() && self.is_ancestor(child, parent));
            if cycle {
                return Err(MutationError::HierarchyCycle { parent, child });
            }
        }
        Ok(())
    }
    /// Whether `ancestor` is a proper ancestor of `node`.
    fn is_ancestor(&self, ancestor: NodeKey, node: NodeKey) -> bool {
        let mut current = self.0.doc.get_node(node.node_id()).and_then(|n| n.parent);
        while let Some(id) = current {
            if id.key() == ancestor {
                return true;
            }
            current = self.0.doc.get_node(id).and_then(|n| n.parent);
        }
        false
    }
}

/// Converts keys for Blitz; the common single-child case does not allocate.
fn with_ids<R>(children: &[NodeKey], f: impl FnOnce(&[NodeId]) -> R) -> R {
    match children {
        [] => f(&[]),
        [one] => f(&[one.node_id()]),
        many => {
            let ids: Vec<NodeId> = many.iter().copied().map(ToNodeId::node_id).collect();
            f(&ids)
        }
    }
}

impl Mutator for BlitzMutator<'_> {
    fn parent(&self, key: NodeKey) -> Option<NodeKey> {
        self.node(key).ok()?.parent.map(ToKey::key)
    }
    fn last_child(&self, key: NodeKey) -> Option<NodeKey> {
        self.node(key)
            .ok()?
            .children
            .last()
            .copied()
            .map(ToKey::key)
    }
    fn previous_sibling(&self, key: NodeKey) -> Option<NodeKey> {
        self.node(key).ok()?;
        self.0.previous_sibling_id(key.node_id()).map(ToKey::key)
    }

    fn create_element(
        &mut self,
        name: QualName,
        attrs: impl IntoIterator<Item = NewAttribute>,
    ) -> NodeKey {
        (self.0.create_element(name, attributes(attrs))).key()
    }
    fn create_text(&mut self, text: &str) -> NodeKey {
        (self.0.create_text_node(text)).key()
    }
    fn create_comment(&mut self, text: &str) -> NodeKey {
        (self.0.create_comment_node(text)).key()
    }
    fn template_contents(&mut self, template: NodeKey) -> Result<NodeKey, MutationError> {
        let id = self.element(template)?;
        Ok((self.0.template_contents(id)).key())
    }

    fn append_children(
        &mut self,
        parent: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError> {
        let parent_id = self.container(parent)?;
        self.check_children(parent, children)?;
        with_ids(children, |ids| self.0.append_children(parent_id, ids));
        if !children.is_empty() {
            self.stepped();
        }
        Ok(())
    }
    fn insert_before(
        &mut self,
        anchor: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError> {
        let anchor_id = self.live(anchor)?;
        let parent = self.parent(anchor).ok_or(MutationError::Detached(anchor))?;
        self.check_children(parent, children)?;
        with_ids(children, |ids| self.0.insert_nodes_before(anchor_id, ids));
        if !children.is_empty() {
            self.stepped();
        }
        Ok(())
    }
    fn replace_children(
        &mut self,
        parent: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError> {
        let parent_id = self.container(parent)?;
        self.check_children(parent, children)?;
        // Replacing nothing with nothing changes nothing.
        let had_children = !self.node(parent)?.children.is_empty();
        with_ids(children, |ids| self.0.replace_children(parent_id, ids));
        if had_children || !children.is_empty() {
            self.stepped();
        }
        Ok(())
    }
    fn clone_node(&mut self, key: NodeKey, deep: bool) -> Result<NodeKey, MutationError> {
        use blitz_dom::node::NodeData;
        let id = self.live(key)?;
        if deep {
            return Ok((self.0.deep_clone_node(id)).key());
        }
        let cloned = match &self.node(key)?.data {
            NodeData::Element(e) | NodeData::AnonymousBlock(e) => {
                let (name, attrs) = (e.name.clone(), e.attrs.to_vec());
                self.0.create_element(name, attrs)
            }
            NodeData::Text(t) => {
                let text = t.content.to_string();
                self.0.create_text_node(&text)
            }
            NodeData::Comment { contents } => {
                let text = contents.clone();
                self.0.create_comment_node(&text)
            }
            _ => return Err(MutationError::NotCloneable(key)),
        };
        Ok(cloned.key())
    }
    fn remove(&mut self, key: NodeKey) -> Result<(), MutationError> {
        let id = self.live(key)?;
        self.0.remove_node(id);
        self.stepped();
        Ok(())
    }
    fn remove_and_drop(&mut self, key: NodeKey) -> Result<(), MutationError> {
        let id = self.live(key)?;
        self.0.remove_and_drop_node(id);
        self.stepped();
        Ok(())
    }
    fn reparent_children(&mut self, from: NodeKey, to: NodeKey) -> Result<(), MutationError> {
        let from_id = self.container(from)?;
        let to_id = self.container(to)?;
        if from == to || self.is_ancestor(from, to) {
            return Err(MutationError::HierarchyCycle {
                parent: to,
                child: from,
            });
        }
        let moves = !self.node(from)?.children.is_empty();
        self.0.reparent_children(from_id, to_id);
        if moves {
            self.stepped();
        }
        Ok(())
    }

    fn set_text(&mut self, node: NodeKey, text: &str) -> Result<(), MutationError> {
        let id = self.live(node)?;
        let current = self.node(node)?;
        if BlitzNode(current).kind() != NodeKind::Text {
            return Err(MutationError::NotText(node));
        }
        let unchanged =
            matches!(&current.data, blitz_dom::node::NodeData::Text(t) if &*t.content == text);
        self.0.set_node_text(id, text);
        if !unchanged {
            self.stepped();
        }
        Ok(())
    }
    fn append_text(&mut self, node: NodeKey, text: &str) -> Result<(), MutationError> {
        let id = self.live(node)?;
        if BlitzNode(self.node(node)?).kind() != NodeKind::Text {
            return Err(MutationError::NotText(node));
        }
        self.0
            .append_text_to_node(id, text)
            .map_err(|_| MutationError::NotText(node))?;
        if !text.is_empty() {
            self.stepped();
        }
        Ok(())
    }
    fn set_attribute(
        &mut self,
        node: NodeKey,
        name: QualName,
        value: &str,
    ) -> Result<(), MutationError> {
        let id = self.element(node)?;
        let unchanged = self
            .attribute_value(node, &name)
            .is_some_and(|current| current == value);
        self.0.set_attribute(id, name, value);
        if !unchanged {
            self.stepped();
        }
        Ok(())
    }
    fn clear_attribute(&mut self, node: NodeKey, name: QualName) -> Result<(), MutationError> {
        let id = self.element(node)?;
        let present = self.attribute_value(node, &name).is_some();
        self.0.clear_attribute(id, name);
        if present {
            self.stepped();
        }
        Ok(())
    }
    fn add_attributes_if_missing(
        &mut self,
        node: NodeKey,
        attrs: impl IntoIterator<Item = NewAttribute>,
    ) -> Result<(), MutationError> {
        let id = self.element(node)?;
        let attrs = attributes(attrs);
        let adds = attrs
            .iter()
            .any(|attr| self.attribute_value(node, &attr.name).is_none());
        self.0.add_attrs_if_missing(id, attrs);
        if adds {
            self.stepped();
        }
        Ok(())
    }
}
