use super::*;
use wega::engine_protocol::{
    DocumentContext, DocumentId, DocumentRevision,
    inspector::{
        dom::InspectorNodeId,
        geometry::{ElementLabel, GeomRect, ViewportReport},
    },
};

fn report(state: GeometryState, paint: PaintMapping) -> GeometryReport {
    let d = DocumentContext {
        document: DocumentId(1),
        revision: DocumentRevision(1),
    };
    GeometryReport {
        document: d,
        epoch: 3,
        node: InspectorNodeId {
            document: d,
            key: 1,
        },
        state,
        boxes: matches!(state, GeometryState::Current | GeometryState::Stale).then_some(BoxSet {
            border_box: GeomRect {
                x: 1.0,
                y: 2.0,
                width: 3.5,
                height: 4.0,
            },
            border: Edges::default(),
            padding: Edges::default(),
            margin: Edges::default(),
        }),
        viewport: ViewportReport {
            width: 1.0,
            height: 1.0,
            device_pixel_ratio: 1.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
        },
        paint,
        label: ElementLabel::default(),
    }
}

#[test]
fn every_state_has_an_explicit_message_and_none_invents_numbers() {
    let exact = PaintMapping::Exact { clip: None };
    assert!(status(None, None).contains("never forces layout"));
    for (state, word) in [
        (GeometryState::Current, "Current"),
        (GeometryState::Stale, "Stale"),
        (GeometryState::Pending, "Pending"),
        (GeometryState::NoBoxes, "no boxes"),
        (GeometryState::Refused, "nested too deeply"),
        (GeometryState::Invalid, "no longer exists"),
    ] {
        let text = status(Some(&report(state, exact)), Some(3));
        assert!(text.contains(word), "{state:?}: {text}");
    }
    // A report older than the document says it is being refreshed.
    assert!(status(Some(&report(GeometryState::Current, exact)), Some(4)).contains("refreshing"));
    assert!(!status(Some(&report(GeometryState::Current, exact)), Some(3)).contains("refreshing"));
}

#[test]
fn paint_limitations_are_stated_next_to_the_numbers() {
    let t = status(
        Some(&report(
            GeometryState::Current,
            PaintMapping::Unsupported {
                reason: UnsupportedPaint::Transform,
            },
        )),
        None,
    );
    assert!(t.contains("transformed"), "{t}");
    let c = status(
        Some(&report(
            GeometryState::Current,
            PaintMapping::Exact {
                clip: Some(GeomRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                }),
            },
        )),
        None,
    );
    assert!(c.contains("clipped"), "{c}");
    assert_eq!(num(3.5), "3.50");
    assert_eq!(num(4.0), "4");
}
