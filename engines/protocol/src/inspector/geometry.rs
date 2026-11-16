//! Geometry and picking values. All coordinates are CSS pixels in the **viewport** space of the
//! document the report is stamped with (scroll already applied); the browser multiplies by its
//! own page zoom. Nothing here names an engine type.
use super::dom::InspectorNodeId;
use crate::DocumentContext;
use serde::{Deserialize, Serialize};

pub const MAX_ANCESTORS: usize = 128;
pub const MAX_LABEL_TAG: usize = 64;
pub const MAX_LABEL_ID: usize = 128;
pub const MAX_LABEL_CLASSES: usize = 256;
pub const MAX_COORDINATE: f64 = 1.0e7;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeomRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl GeomRect {
    fn bounded(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= MAX_COORDINATE)
            && self.width >= 0.0
            && self.height >= 0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Edges {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}
impl Edges {
    fn bounded(&self) -> bool {
        [self.top, self.right, self.bottom, self.left]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= MAX_COORDINATE)
    }
}

/// The four CSS boxes: `border_box` is the element's bounding rectangle; the edge widths derive
/// the padding, content and margin boxes (see [`BoxSet::padding_box`] and friends).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoxSet {
    pub border_box: GeomRect,
    pub border: Edges,
    pub padding: Edges,
    pub margin: Edges,
}
impl BoxSet {
    pub fn padding_box(&self) -> GeomRect {
        shrink(self.border_box, self.border)
    }
    pub fn content_box(&self) -> GeomRect {
        shrink(self.padding_box(), self.padding)
    }
    pub fn margin_box(&self) -> GeomRect {
        GeomRect {
            x: self.border_box.x - self.margin.left,
            y: self.border_box.y - self.margin.top,
            width: self.border_box.width + self.margin.left + self.margin.right,
            height: self.border_box.height + self.margin.top + self.margin.bottom,
        }
    }
    pub fn is_bounded(&self) -> bool {
        self.border_box.bounded()
            && self.border.bounded()
            && self.padding.bounded()
            && self.margin.bounded()
    }
}
fn shrink(r: GeomRect, e: Edges) -> GeomRect {
    GeomRect {
        x: r.x + e.left,
        y: r.y + e.top,
        width: (r.width - e.left - e.right).max(0.0),
        height: (r.height - e.top - e.bottom).max(0.0),
    }
}

/// How much to trust the boxes. The backend never flushes layout to improve on this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeometryState {
    /// Layout is current.
    Current,
    /// Style or layout work is pending: the boxes describe an older document.
    Stale,
    /// No layout exists yet.
    Pending,
    /// The node generates no boxes.
    NoBoxes,
    /// The document nests past the layout depth limit and is never laid out.
    Refused,
    /// The node is gone or never existed in this document.
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnsupportedPaint {
    /// The node or an ancestor has a transform; layout boxes are not its painted geometry.
    Transform,
    /// A positioned node under a clipping ancestor: which clips apply is not modelled.
    ClipPositioned,
    Unknown,
}

/// Whether the boxes equal what is painted. `Unsupported` must suppress any overlay.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PaintMapping {
    /// Boxes are the painted geometry, visible only inside `clip` when present.
    Exact {
        clip: Option<GeomRect>,
    },
    Unsupported {
        reason: UnsupportedPaint,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewportReport {
    pub width: f64,
    pub height: f64,
    pub device_pixel_ratio: f64,
    pub scroll_x: f64,
    pub scroll_y: f64,
}

/// Tag, id and classes for the overlay label, bounded.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementLabel {
    pub tag: String,
    pub id: String,
    pub classes: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeometryReport {
    pub document: DocumentContext,
    /// The mutation epoch the backend saw; compare for inequality with the browser's own scope.
    pub epoch: u64,
    pub node: InspectorNodeId,
    pub state: GeometryState,
    pub boxes: Option<BoxSet>,
    pub viewport: ViewportReport,
    pub paint: PaintMapping,
    pub label: ElementLabel,
}
impl GeometryReport {
    pub fn is_bounded(&self) -> bool {
        let boxes_ok = match (self.state, &self.boxes) {
            (GeometryState::Current | GeometryState::Stale, Some(b)) => b.is_bounded(),
            (_, None) => true,
            _ => false,
        };
        let v = &self.viewport;
        let clip_ok = match self.paint {
            PaintMapping::Exact { clip: Some(c) } => c.bounded(),
            _ => true,
        };
        self.node.document == self.document
            && boxes_ok
            && clip_ok
            && [
                v.width,
                v.height,
                v.device_pixel_ratio,
                v.scroll_x,
                v.scroll_y,
            ]
            .iter()
            .all(|n| n.is_finite() && n.abs() <= MAX_COORDINATE)
            && self.label.tag.len() <= MAX_LABEL_TAG
            && self.label.id.len() <= MAX_LABEL_ID
            && self.label.classes.len() <= MAX_LABEL_CLASSES
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PickReport {
    /// The topmost element at the point, with its geometry.
    pub target: GeometryReport,
    /// The target's ancestors from the root element down to its parent, at most
    /// [`MAX_ANCESTORS`]; empty when the chain is longer (the browser then asks the DOM).
    pub ancestors: Vec<InspectorNodeId>,
}
impl PickReport {
    pub fn is_bounded(&self) -> bool {
        self.target.is_bounded()
            && self.ancestors.len() <= MAX_ANCESTORS
            && self
                .ancestors
                .iter()
                .all(|a| a.document == self.target.document)
    }
}

/// CSS pixels to the fixed-point wire form of a pick position. Non-finite input maps to 0 and
/// out-of-range input saturates, so a conversion never fails.
pub fn to_milli(css: f32) -> i32 {
    if css.is_finite() {
        (css as f64 * 1000.0).clamp(-1.0e9, 1.0e9) as i32
    } else {
        0
    }
}
pub fn from_milli(milli: i32) -> f32 {
    milli as f32 / 1000.0
}
