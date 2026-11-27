//! Cached-only geometry and picking. Reads `&self` documents through Wega traits: no flush, no
//! style resolution, no script. The report says how fresh and how paint-accurate it is; it never
//! improves either by asking the engine to do work.
use wega_dom::{
    DocumentView, MutationClock, NodeKey, NodeKind, NodeView,
    layout::{
        Geometry, GeometryAccess, HitTest, PaintMapping as DomPaint, Point,
        UnsupportedPaint as DomUnsupported,
    },
};
use wega_protocol::{
    DocumentContext,
    inspector::{InspectorError, InspectorErrorCode, dom::InspectorNodeId, geometry::*},
};

fn error(code: InspectorErrorCode, message: &str) -> InspectorError {
    InspectorError::new(code, message)
}
fn bounded(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}
fn rect(r: wega_dom::layout::Rect) -> GeomRect {
    GeomRect {
        x: r.x,
        y: r.y,
        width: r.width,
        height: r.height,
    }
}
fn edges(i: wega_dom::layout::Insets) -> Edges {
    Edges {
        top: i.top,
        right: i.right,
        bottom: i.bottom,
        left: i.left,
    }
}
fn boxes(b: wega_dom::layout::BoxModel) -> BoxSet {
    BoxSet {
        border_box: rect(b.border_box),
        border: edges(b.border),
        padding: edges(b.padding),
        margin: edges(b.margin),
    }
}
fn label<D: DocumentView>(doc: &D, key: NodeKey) -> ElementLabel {
    let Some(node) = doc.node(key) else {
        return ElementLabel::default();
    };
    let Some(name) = node.element_name() else {
        return ElementLabel::default();
    };
    ElementLabel {
        tag: bounded(name.local, MAX_LABEL_TAG),
        id: bounded(node.attr("id").unwrap_or(""), MAX_LABEL_ID),
        classes: bounded(node.attr("class").unwrap_or(""), MAX_LABEL_CLASSES),
    }
}

/// The geometry of `key` as an inspector report, from cached state only.
pub fn report<D: GeometryAccess + MutationClock>(
    doc: &D,
    document: DocumentContext,
    key: NodeKey,
) -> GeometryReport {
    let viewport = doc.viewport();
    let viewport = ViewportReport {
        width: viewport.size.width,
        height: viewport.size.height,
        device_pixel_ratio: viewport.device_pixel_ratio,
        scroll_x: viewport.scroll.x,
        scroll_y: viewport.scroll.y,
    };
    let node = InspectorNodeId {
        document,
        key: key.as_u64(),
    };
    let (state, set) = match doc.inspect_geometry(key) {
        Geometry::Current(b) => (GeometryState::Current, Some(boxes(b))),
        Geometry::Stale(b) => (GeometryState::Stale, Some(boxes(b))),
        Geometry::Pending => (GeometryState::Pending, None),
        Geometry::NoBoxes => (GeometryState::NoBoxes, None),
        Geometry::Refused => (GeometryState::Refused, None),
        Geometry::Invalid => (GeometryState::Invalid, None),
    };
    // Reject non-finite engine output rather than forwarding it.
    let (state, set) = match set {
        Some(b) if !b.is_bounded() => (GeometryState::Invalid, None),
        other => (state, other),
    };
    let paint = if set.is_some() {
        match doc.paint_mapping(key) {
            DomPaint::Exact { clip } => PaintMapping::Exact {
                clip: clip.map(rect),
            },
            DomPaint::Unsupported(reason) => PaintMapping::Unsupported {
                reason: match reason {
                    DomUnsupported::Transform => UnsupportedPaint::Transform,
                    DomUnsupported::ClipPositioned => UnsupportedPaint::ClipPositioned,
                    DomUnsupported::Unknown => UnsupportedPaint::Unknown,
                },
            },
        }
    } else {
        PaintMapping::Unsupported {
            reason: UnsupportedPaint::Unknown,
        }
    };
    GeometryReport {
        document,
        epoch: doc.mutation_epoch().as_u64(),
        node,
        state,
        boxes: set,
        viewport,
        paint,
        label: label(doc, key),
    }
}

/// `GetGeometry`: the node must belong to `document`.
pub fn inspect<D: GeometryAccess + MutationClock>(
    doc: &D,
    document: DocumentContext,
    node: InspectorNodeId,
) -> Result<GeometryReport, InspectorError> {
    if node.document != document {
        return Err(error(
            InspectorErrorCode::StaleDocument,
            "Node belongs to a replaced document",
        ));
    }
    let key = NodeKey::from_u64(node.key);
    if doc.node(key).is_none() {
        return Err(error(InspectorErrorCode::StaleNode, "Node was dropped"));
    }
    Ok(report(doc, document, key))
}

/// `PickNode`: hit-test at a viewport CSS point against the cached layout.
pub fn pick<D: GeometryAccess + HitTest + MutationClock>(
    doc: &D,
    document: DocumentContext,
    x: f32,
    y: f32,
) -> Result<PickReport, InspectorError> {
    let key = doc
        .element_at(Point {
            x: x as f64,
            y: y as f64,
        })
        .ok_or_else(|| error(InspectorErrorCode::Unavailable, "Nothing at that point"))?;
    let Some(node) = doc.node(key) else {
        return Err(error(InspectorErrorCode::StaleNode, "Hit node was dropped"));
    };
    if node.kind() != NodeKind::Element {
        return Err(error(
            InspectorErrorCode::StaleNode,
            "Hit node is not an element",
        ));
    }
    // Ancestors root first; an overlong chain is reported empty (the browser asks the DOM).
    let mut chain = Vec::new();
    let mut at = node.parent();
    while let Some(parent) = at {
        if chain.len() > MAX_ANCESTORS {
            chain.clear();
            break;
        }
        chain.push(InspectorNodeId {
            document,
            key: parent.as_u64(),
        });
        at = doc.node(parent).and_then(|n| n.parent());
    }
    if chain.len() > MAX_ANCESTORS {
        chain.clear();
    }
    chain.reverse();
    Ok(PickReport {
        target: report(doc, document, key),
        ancestors: chain,
    })
}
