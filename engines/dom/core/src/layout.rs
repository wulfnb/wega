//! Layout geometry: element boxes, scroll state, the viewport and hit-testing.
//!
//! The document engine owns the layout tree and decides when it is current. These traits only
//! read its results (or forward a scroll/viewport write); they keep no cache and no copy of the
//! layout, so there is no second source of truth.
//!
//! # Coordinate spaces
//!
//! Everything is in **CSS pixels** as `f64`, never device pixels. The device pixel ratio is
//! reported by [`ViewportGeometry::device_pixel_ratio`] (page zoom included) and is only needed
//! to convert to device pixels, which is a rendering concern outside this contract.
//!
//! - **Viewport-relative (client)**: origin at the top-left of the visible viewport. Used by
//!   [`GeometryAccess::bounding_rect`], [`GeometryAccess::client_rects`] and [`HitTest`]
//!   (`getBoundingClientRect`, pointer positions).
//! - **Document-relative (page)**: origin at the top-left of the initial containing block, i.e.
//!   viewport-relative plus [`ViewportGeometry::scroll`]. Pointer coordinates are converted at
//!   this boundary, so callers never add the scroll offset themselves.
//! - **Offset-parent-relative**: [`OffsetBox::rect`] is measured from the padding edge of the
//!   offset parent (CSSOM View `offsetLeft`/`offsetTop`).
//! - **Element-relative**: [`ClientBox`] and [`ContentBox`] describe the element's own box.
//!
//! Values keep fractional precision (the document engine snaps rectangles to 1/64 px). The
//! JavaScript getters that the CSSOM View spec defines as integers round at the JS layer.
//!
//! # Flush semantics
//!
//! Reads take `&self` and **never recalculate** style or layout; they report the layout as of
//! the last flush. The caller decides when a flush is required (the engine flushes if anything
//! changed since the last one) and performs it with [`LayoutControl::flush_layout`]. This keeps
//! repeated reads free of accidental synchronous relayout and leaves a single authority for
//! "layout is dirty".
//!
//! Not covered: transforms (the document engine reports untransformed boxes), text geometry
//! and selection rectangles (no script API exposes them), stacking-order hit-testing beyond the
//! document engine's own.
use crate::{DocumentView, NodeKey};

/// Why a layout query was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutError {
    /// The key does not (or no longer) refer to a node of this document.
    Stale(NodeKey),
}
impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stale(key) => write!(f, "node {key} no longer exists"),
        }
    }
}
impl std::error::Error for LayoutError {}

/// A point in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
/// A size in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}
/// A rectangle in CSS pixels; the space is stated by the method that returns it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };
    pub fn right(&self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
}

/// The layout viewport: its size, the page zoom and the scroll position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportGeometry {
    /// Visible size in CSS pixels (`window.innerWidth`/`innerHeight`).
    pub size: Size,
    /// Device pixels per CSS pixel, page zoom included (`window.devicePixelRatio`).
    pub device_pixel_ratio: f64,
    /// Document-relative position of the viewport's top-left corner (`scrollX`/`scrollY`).
    pub scroll: Point,
}

/// The size a viewport is configured with (device pixels), as the embedder reports it. Also the
/// render viewport: the painter scale is `scale * zoom` (see `wega_dom::render`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportConfig {
    pub width_px: u32,
    pub height_px: u32,
    /// Device pixels per CSS pixel before zoom.
    pub scale: f32,
    pub zoom: f32,
    /// `prefers-color-scheme: dark`.
    pub dark: bool,
}

/// The padding box measured from the border box: `clientLeft`/`clientTop` are the border widths,
/// `width`/`height` are `clientWidth`/`clientHeight` (without borders and scrollbars).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClientBox {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}
/// `offsetLeft`/`offsetTop`/`offsetWidth`/`offsetHeight` as one rectangle (the border box,
/// positioned relative to the padding edge of the offset parent) and the offset parent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffsetBox {
    pub rect: Rect,
    pub parent: Option<NodeKey>,
}
/// The content box size and the padding offsets of its origin (ResizeObserver's `contentRect`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ContentBox {
    pub size: Size,
    pub padding_left: f64,
    pub padding_top: f64,
}

/// The deepest element nesting the engine will lay out. Style and layout recurse once per nesting
/// level, so unbounded depth is unbounded native stack: with the engine thread's 8 MiB stack
/// the measured overflow cliff of a debug build is between 520 and 600 levels (a 2 MiB thread
/// overflows at 122, which is the historical "depth-122 hang"). Past this limit
/// [`LayoutControl::flush_layout`] does nothing and [`GeometryAccess::layout_refused`] is true.
/// The limit leaves more than twice the measured headroom. See `devtools-d3-c-0-report.md`.
pub const MAX_LAYOUT_DEPTH: usize = 256;

/// Read access to layout results.
pub trait GeometryAccess: DocumentView {
    /// Whether the document nests deeper than [`MAX_LAYOUT_DEPTH`], so that it is never laid out
    /// and every geometry reader keeps reporting "no boxes". Unlike `layout_pending` this does
    /// not clear by waiting. Walks the tree: call it from diagnostics, not per frame.
    fn layout_refused(&self) -> bool {
        false
    }
    /// Whether the element generates boxes: false for `display: none`, `display: contents` and
    /// nodes outside the document. Such nodes report empty geometry.
    fn has_boxes(&self, key: NodeKey) -> Result<bool, LayoutError>;
    /// `getBoundingClientRect`, viewport-relative. All zero for non-elements, for nodes
    /// without boxes and for nodes outside the document.
    fn bounding_rect(&self, key: NodeKey) -> Result<Rect, LayoutError>;
    /// `getClientRects`, viewport-relative: appends one rectangle per box (per line box for
    /// inline elements); appends nothing for a node without boxes.
    fn client_rects(&self, key: NodeKey, out: &mut Vec<Rect>) -> Result<(), LayoutError>;
    /// `clientLeft`/`clientTop`/`clientWidth`/`clientHeight`.
    fn client_box(&self, key: NodeKey) -> Result<ClientBox, LayoutError>;
    /// `offsetLeft`/`offsetTop`/`offsetWidth`/`offsetHeight`/`offsetParent`.
    fn offset_box(&self, key: NodeKey) -> Result<OffsetBox, LayoutError>;
    /// `scrollWidth`/`scrollHeight`: the content size including overflow.
    fn scroll_size(&self, key: NodeKey) -> Result<Size, LayoutError>;
    /// `scrollLeft`/`scrollTop` of an element (not the viewport; see [`Self::viewport`]).
    fn scroll_offset(&self, key: NodeKey) -> Result<Point, LayoutError>;
    /// The content box (for ResizeObserver).
    fn content_box(&self, key: NodeKey) -> Result<ContentBox, LayoutError>;
    /// The viewport size, zoom and scroll position.
    fn viewport(&self) -> ViewportGeometry;
    /// Whether style or layout work is pending, so that the readers above report an *older*
    /// layout (or none at all, before the first one). The readers never flush; a caller that must
    /// not trigger layout (an inspector) reads this instead and reports "layout pending" rather
    /// than asking for a flush, which on a pathological document may never finish.
    fn layout_pending(&self) -> bool;
}

/// Hit-testing against the current layout.
pub trait HitTest: GeometryAccess {
    /// The topmost element at the **viewport-relative** point, as `document.elementFromPoint`:
    /// anonymous boxes and text resolve to their nearest element, a point over no box resolves
    /// to the root element, and a point outside the viewport has no target. The caller flushes
    /// layout first.
    fn element_at(&self, point: Point) -> Option<NodeKey>;
}

/// Layout flushes, viewport changes and scroll writes.
pub trait LayoutControl: GeometryAccess {
    /// Brings style and layout up to date. This is the only operation that recalculates;
    /// `animation_time` is seconds since the document started (animations and transitions).
    fn flush_layout(&mut self, animation_time: f64);
    /// Sets the viewport size, zoom and colour scheme; the next flush lays out against it.
    fn set_viewport(&mut self, viewport: ViewportConfig);
    /// Sets the viewport scroll position (document-relative); clamping is the engine's.
    fn set_viewport_scroll(&mut self, scroll: Point);
    /// Sets an element's `scrollLeft`/`scrollTop` (instant, clamped by the engine).
    fn scroll_node_to(&mut self, key: NodeKey, scroll: Point) -> Result<(), LayoutError>;
    /// `scrollIntoView()` with the default alignment (block start, inline nearest).
    fn scroll_node_into_view(&mut self, key: NodeKey) -> Result<(), LayoutError>;
    /// Scrolls to the element a URL fragment names; false when there is none.
    fn scroll_to_fragment(&mut self, fragment: &str) -> bool;
}
