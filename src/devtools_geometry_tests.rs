use super::*;
use wega::engine_protocol::{
    DocumentId, DocumentRevision,
    inspector::{
        InspectorArgs,
        geometry::{BoxSet, Edges, ElementLabel, GeomRect, PaintMapping, ViewportReport},
    },
};

fn doc(n: u64) -> DocumentContext {
    DocumentContext {
        document: DocumentId(n),
        revision: DocumentRevision(1),
    }
}
fn node(d: DocumentContext, key: u64) -> InspectorNodeId {
    InspectorNodeId { document: d, key }
}
fn report(d: DocumentContext, key: u64, epoch: u64) -> GeometryReport {
    let rect = GeomRect {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
    };
    GeometryReport {
        document: d,
        epoch,
        node: node(d, key),
        state: GeometryState::Current,
        boxes: Some(BoxSet {
            border_box: rect,
            border: Edges::default(),
            padding: Edges::default(),
            margin: Edges::default(),
        }),
        viewport: ViewportReport {
            width: 800.0,
            height: 600.0,
            device_pixel_ratio: 1.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
        },
        paint: PaintMapping::Exact { clip: None },
        label: ElementLabel::default(),
    }
}
fn picked(d: DocumentContext, key: u64) -> Result<InspectorResponse, InspectorError> {
    Ok(InspectorResponse::Picked(Box::new(PickReport {
        target: report(d, key, 1),
        ancestors: vec![node(d, 1)],
    })))
}
fn geometry(d: DocumentContext, key: u64, epoch: u64) -> Result<InspectorResponse, InspectorError> {
    Ok(InspectorResponse::Geometry(Box::new(report(d, key, epoch))))
}
fn view(d: DocumentContext) -> GeometryView {
    let mut v = GeometryView::default();
    v.document(d);
    v
}
fn t0() -> Instant {
    Instant::now()
}

#[test]
fn pointer_movement_is_coalesced_into_one_outstanding_pick_with_the_newest_position() {
    let d = doc(1);
    let mut v = view(d);
    // Not in picker mode: movement is ignored.
    v.hover_at(1.0, 1.0);
    assert_eq!(v.wanted(), None);
    v.set_picker(true);
    let now = t0();
    for i in 0..1000 {
        v.hover_at(i as f32, 5.0);
    }
    let (command, args) = v.next(now, None, None).unwrap();
    assert_eq!(command, InspectorCommand::PickNode);
    assert_eq!(
        args,
        InspectorArgs::Pick {
            x_milli: to_milli(999.0),
            y_milli: to_milli(5.0)
        },
        "only the newest position is asked about"
    );
    // One at a time, however much the pointer moves meanwhile.
    v.hover_at(3.0, 3.0);
    assert!(v.next(now, None, None).is_none());
    v.complete(InspectorCommand::PickNode, picked(d, 9));
    assert_eq!(v.hover.as_ref().unwrap().node.key, 9);
    // And not faster than the hover interval.
    assert!(v.next(now, None, None).is_none());
    assert!(v.next(now + HOVER_INTERVAL, None, None).is_some());
}

#[test]
fn a_click_selects_ends_picker_mode_and_preempts_hover() {
    let d = doc(1);
    let mut v = view(d);
    v.set_picker(true);
    v.hover_at(1.0, 1.0);
    v.click_at(7.0, 8.0);
    let (_, args) = v.next(t0(), None, None).unwrap();
    assert_eq!(
        args,
        InspectorArgs::Pick {
            x_milli: to_milli(7.0),
            y_milli: to_milli(8.0)
        }
    );
    v.complete(InspectorCommand::PickNode, picked(d, 5));
    assert!(!v.picker, "page input is forwarded again");
    assert!(v.hover.is_none());
    let chosen = v.take_chosen().unwrap();
    assert_eq!(chosen.target.node.key, 5);
    assert!(v.take_chosen().is_none(), "taken once");
}

#[test]
fn cancelling_while_a_pick_is_in_flight_drops_its_reply() {
    let d = doc(1);
    let mut v = view(d);
    v.set_picker(true);
    v.click_at(1.0, 1.0);
    v.next(t0(), None, None).unwrap();
    v.set_picker(false);
    v.complete(InspectorCommand::PickNode, picked(d, 5));
    assert!(v.take_chosen().is_none());
    assert!(v.hover.is_none());
    assert!(!v.is_pending(), "the correlation is released");
    // Escape during hover likewise.
    v.set_picker(true);
    v.hover_at(1.0, 1.0);
    v.next(t0(), None, None).unwrap();
    v.set_picker(false);
    v.complete(InspectorCommand::PickNode, picked(d, 6));
    assert!(v.hover.is_none());
}

#[test]
fn replies_for_another_document_or_node_are_dropped_and_a_new_document_ends_picking() {
    let d = doc(1);
    let mut v = view(d);
    v.set_picker(true);
    v.hover_at(1.0, 1.0);
    v.next(t0(), None, None).unwrap();
    v.complete(InspectorCommand::PickNode, picked(doc(2), 4));
    assert!(v.hover.is_none(), "foreign document");
    // Geometry for the wrong node.
    let mut v = view(d);
    let n = node(d, 3);
    v.next(t0(), Some(n), Some(1)).unwrap();
    v.complete(InspectorCommand::GetGeometry, geometry(d, 99, 1));
    assert!(v.selected.is_none());
    // A document change resets everything, including the picker.
    let mut v = view(d);
    v.set_picker(true);
    v.hover_at(1.0, 1.0);
    v.document(doc(2));
    assert!(!v.picker);
    assert_eq!(v.wanted(), None);
}

#[test]
fn selected_geometry_is_read_once_per_node_and_epoch() {
    let d = doc(1);
    let mut v = view(d);
    let n = node(d, 3);
    let now = t0();
    assert!(v.next(now, None, Some(1)).is_none());
    assert!(v.next(now, Some(n), None).is_none());
    let (command, _) = v.next(now, Some(n), Some(1)).unwrap();
    assert_eq!(command, InspectorCommand::GetGeometry);
    v.complete(InspectorCommand::GetGeometry, geometry(d, 3, 1));
    assert!(v.selected.is_some());
    assert!(v.next(now, Some(n), Some(1)).is_none(), "unchanged");
    assert!(v.next(now, Some(n), Some(2)).is_some(), "epoch moved");
    // The overlay only offers reports of the current document with boxes.
    v.complete(InspectorCommand::GetGeometry, geometry(d, 3, 2));
    assert!(v.overlay(d).0.is_some());
    assert!(v.overlay(doc(2)).0.is_none());
    // A request that never answers is released by the timeout.
    let mut v = view(d);
    v.next(now, Some(n), Some(1)).unwrap();
    v.expire(now + Duration::from_millis(INSPECTOR_TIMEOUT_MS + 200));
    assert!(!v.is_pending());
}

fn edits(d: DocumentContext) -> EditView {
    let mut e = EditView::default();
    e.document(d);
    e.observe_grant(Some(d), d);
    e
}
fn mutated(
    d: DocumentContext,
    key: u64,
    epoch: u64,
    changed: bool,
) -> Result<InspectorResponse, InspectorError> {
    Ok(InspectorResponse::Mutated(Box::new(MutationOutcome {
        document: d,
        node: node(d, key),
        epoch,
        changed,
    })))
}
fn set(name: &str) -> MutationOp {
    MutationOp::SetAttribute {
        name: name.into(),
        value: "v".into(),
    }
}

#[test]
fn edits_are_refused_without_authority_and_never_leave_the_browser_unvalidated() {
    let d = doc(1);
    let mut e = EditView::default();
    e.document(d);
    assert!(!e.submit(node(d, 1), set("a")), "not enabled");
    assert!(!e.notice.is_empty());
    e.observe_grant(Some(doc(2)), d);
    assert!(
        !e.is_enabled(),
        "a grant for another document is not a grant"
    );
    e.observe_grant(Some(d), d);
    assert!(e.is_enabled());
    assert!(
        !e.submit(node(doc(2), 1), set("a")),
        "node of another document"
    );
    assert!(
        !e.submit(node(d, 1), set("bad name")),
        "validated before sending"
    );
    assert!(e.submit(node(d, 1), set("a")));
}

#[test]
fn the_edit_queue_is_bounded_serial_and_dropped_when_authority_is_lost() {
    let d = doc(1);
    let mut e = edits(d);
    for i in 0..MAX_QUEUED_EDITS {
        assert!(e.submit(node(d, 1), set(&format!("a{i}"))), "{i}");
    }
    assert!(!e.submit(node(d, 1), set("over")), "queue full");
    let now = t0();
    assert!(e.next(now).is_some());
    assert!(e.next(now).is_none(), "one in flight");
    assert!(!e.submit(node(d, 1), set("still-full")), "in flight counts");
    // Revocation (or navigation) clears everything.
    e.observe_grant(None, d);
    assert!(!e.busy());
    assert!(e.notice.contains("disabled"));
    assert!(e.next(now).is_none());
}

#[test]
fn a_stale_epoch_is_retried_once_then_reported_and_success_adopts_the_new_epoch() {
    let d = doc(1);
    let mut e = edits(d);
    e.submit(node(d, 1), set("a"));
    let now = t0();
    e.next(now).unwrap();
    let stale = || {
        Err(InspectorError::new(
            InspectorErrorCode::StaleMutation,
            "moved",
        ))
    };
    assert!(e.complete(stale()), "asks for a status refresh");
    let again = e.next(now).unwrap();
    assert!(!e.complete(stale()), "only one retry");
    assert!(e.notice.contains("keeps changing"), "{}", e.notice);
    assert!(!e.busy());
    let _ = again;
    // Success.
    e.submit(node(d, 1), set("b"));
    e.next(now).unwrap();
    e.complete(mutated(d, 1, 42, true));
    assert_eq!(e.take_adopted_epoch(), Some(42));
    assert_eq!(e.take_adopted_epoch(), None);
    assert_eq!(e.last.as_deref(), Some("Set attribute b"));
    // An ignored CSS write is reported as such.
    e.submit(
        node(d, 1),
        MutationOp::SetInlineProperty {
            property: "nope".into(),
            value: "1".into(),
            important: false,
        },
    );
    e.next(now).unwrap();
    e.complete(mutated(d, 1, 43, false));
    assert!(e.last.as_deref().unwrap().contains("nothing changed"));
}

#[test]
fn foreign_replies_removal_and_errors_are_handled_without_applying_anything() {
    let d = doc(1);
    let mut e = edits(d);
    let now = t0();
    e.submit(node(d, 7), MutationOp::RemoveNode);
    e.next(now).unwrap();
    e.complete(mutated(doc(2), 7, 5, true));
    assert!(e.take_adopted_epoch().is_none() && e.take_removed().is_none());
    e.submit(node(d, 7), MutationOp::RemoveNode);
    e.next(now).unwrap();
    e.complete(mutated(d, 7, 5, true));
    assert_eq!(e.take_removed(), Some(node(d, 7)));
    for (code, text) in [
        (InspectorErrorCode::Unauthorized, "not authorised"),
        (InspectorErrorCode::StaleDocument, "navigated"),
        (InspectorErrorCode::StaleNode, "gone"),
        (InspectorErrorCode::BudgetExceeded, "budget"),
        (InspectorErrorCode::PolicyDenied, "Not permitted"),
    ] {
        e.submit(node(d, 1), set("a"));
        e.next(now).unwrap();
        e.complete(Err(InspectorError::new(code, "x")));
        assert!(e.notice.contains(text), "{code:?}: {}", e.notice);
    }
    // A busy worker re-queues; a timeout releases the slot.
    e.submit(node(d, 1), set("a"));
    e.next(now).unwrap();
    e.unsent(InspectorError::new(InspectorErrorCode::Busy, "busy"));
    assert!(e.next(now).is_some(), "re-queued");
    e.expire(now + Duration::from_millis(INSPECTOR_TIMEOUT_MS + 200));
    assert!(!e.awaiting());
}
