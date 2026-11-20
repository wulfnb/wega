//! Checkpoint D: Dynamic Browser Integration Tests.
mod support;

use wega_dom::DocumentView;
use wega_engine::{PagePort, Pointer, PointerKind, RenderingBackend};
use wega_protocol::inspector::dom::InspectorNodeId;
use wega_protocol::{DocumentContext, DocumentId, DocumentRevision};

#[test]
fn test_view_dynamic_dom_mutation_and_invalidation() {
    let server = support::Server::new(|seen| match seen.path.as_str() {
        "/page" => support::Reply::html(
            r#"
                <!doctype html>
                <body style="margin: 0;">
                    <div id="target" style="width: 120px; height: 80px; background-color: #ff0000;">Initial</div>
                </body>
            "#,
        ),
        _ => support::Reply::html("404"),
    });

    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.paint(800, 600, 1.0, 1.0, false).unwrap();

    let doc_ctx = DocumentContext {
        document: DocumentId(1),
        revision: DocumentRevision(1),
    };

    // Get key of #target
    let target_key = {
        let native_doc = view.native_document.as_ref().unwrap().borrow();
        let d = native_doc.doc.borrow();
        d.element_by_id("target").unwrap()
    };

    // 1. Inspect geometry initially
    let node_id = InspectorNodeId {
        document: doc_ctx,
        key: target_key.as_u64(),
    };
    let report = view.inspect_geometry(doc_ctx, node_id).unwrap();
    let boxes = report.boxes.unwrap();
    assert_eq!(boxes.border_box.width, 120.0);
    assert_eq!(boxes.border_box.height, 80.0);

    // 2. Perform dynamic DOM mutation via script
    view.evaluate("document.getElementById('target').style.width = '420px';")
        .unwrap();

    // Invalidate and repaint
    view.paint(800, 600, 1.0, 1.0, false).unwrap();

    // 3. Inspect geometry after mutation: width must be 420.0
    let report_after = view.inspect_geometry(doc_ctx, node_id).unwrap();
    let boxes_after = report_after.boxes.unwrap();
    assert_eq!(boxes_after.border_box.width, 420.0);
}

#[test]
fn test_view_devtools_picking_at_point() {
    let server = support::Server::new(|seen| match seen.path.as_str() {
        "/pick" => support::Reply::html(
            r#"
                <!doctype html>
                <body style="margin: 0;">
                    <div id="first" style="width: 200px; height: 100px; background-color: #00ff00;">First</div>
                    <div id="second" style="width: 300px; height: 150px; background-color: #0000ff;">Second</div>
                </body>
            "#,
        ),
        _ => support::Reply::html("404"),
    });

    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/pick"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.paint(800, 600, 1.0, 1.0, false).unwrap();

    let doc_ctx = DocumentContext {
        document: DocumentId(1),
        revision: DocumentRevision(1),
    };

    // Pick at (50, 50) -> should be #first
    let pick_first = view.pick_node(doc_ctx, 50.0, 50.0).unwrap();
    assert_eq!(pick_first.target.label.id, "first");

    // Pick at (50, 150) -> should be #second (since first is 100px high)
    let pick_second = view.pick_node(doc_ctx, 50.0, 150.0).unwrap();
    assert_eq!(pick_second.target.label.id, "second");
}

#[test]
fn test_view_pointer_event_routing() {
    let server = support::Server::new(|seen| match seen.path.as_str() {
        "/btn" => support::Reply::html(
            r#"
                <!doctype html>
                <body style="margin: 0;">
                    <button id="btn" style="width: 150px; height: 50px;">Click Me</button>
                </body>
            "#,
        ),
        _ => support::Reply::html("404"),
    });

    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/btn"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.paint(800, 600, 1.0, 1.0, false).unwrap();

    view.evaluate(
        "globalThis.clicks=0;document.getElementById('btn').addEventListener('click',()=>clicks++)",
    )
    .unwrap();
    // Move pointer over button at (75, 25)
    view.pointer(Pointer {
        kind: PointerKind::Move,
        x: 75.0,
        y: 25.0,
        button: 0,
        buttons: 0,
        modifiers: 0,
        delta_x: 0.0,
        delta_y: 0.0,
    });

    // Pointer down over button
    view.pointer(Pointer {
        kind: PointerKind::Down,
        x: 75.0,
        y: 25.0,
        button: 0,
        buttons: 1,
        modifiers: 0,
        delta_x: 0.0,
        delta_y: 0.0,
    });

    // Pointer up over button
    view.pointer(Pointer {
        kind: PointerKind::Up,
        x: 75.0,
        y: 25.0,
        button: 0,
        buttons: 0,
        modifiers: 0,
        delta_x: 0.0,
        delta_y: 0.0,
    });
    assert_eq!(view.evaluate_json("clicks").unwrap(), "1");
}
