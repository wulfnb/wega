//! The layout contract (`wega_dom::layout`) against a real Blitz document.
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use wega_adapter_blitz::BlitzDocument;
use wega_dom::input::{FocusState, InputControl};
use wega_dom::layout::{
    GeometryAccess, HitTest, LayoutControl, LayoutError, Point, Rect, ViewportConfig,
};
use wega_dom::{DocumentView, NodeKey, NodeView};

const HTML: &str = "<!doctype html><style>body{margin:0}\
    #outer{width:100px;height:80px;padding:5px;border:2px solid;margin:10px;position:relative}\
    #inner{width:40px;height:20px;margin:3px}#frac{width:33.5px;height:10.25px;margin-left:.5px}\
    #ov{width:50px;height:30px;overflow:scroll}#big{width:200px;height:100px}\
    #none{display:none}#sp{height:20px}#pad{height:400px}\
    #scroller{width:100px;height:60px;overflow:auto}#content{height:300px}\
    #end{height:20px}</style>\
    <div id=outer><div id=inner></div></div><div id=frac></div>\
    <div id=ov><div id=big></div></div><div id=none><b id=nb></b></div>\
    <div id=pad>pad</div><div id=scroller><div id=content>c</div></div><div id=end>end</div>";

fn viewport(width: u32, height: u32) -> ViewportConfig {
    ViewportConfig {
        width_px: width,
        height_px: height,
        scale: 1.0,
        zoom: 1.0,
        dark: false,
    }
}
fn doc() -> BlitzDocument {
    let mut doc =
        BlitzDocument::new(HtmlDocument::from_html(HTML, DocumentConfig::default()).into_inner());
    doc.set_viewport(viewport(300, 200));
    doc.flush_layout(0.0);
    doc
}
fn id(doc: &BlitzDocument, id: &str) -> NodeKey {
    doc.element_by_id(id).unwrap()
}
fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn boxes_report_border_padding_margin_and_fractions() {
    let doc = doc();
    let outer = id(&doc, "outer");
    assert_eq!(
        doc.bounding_rect(outer).unwrap(),
        rect(10.0, 10.0, 114.0, 94.0)
    );
    let client = doc.client_box(outer).unwrap();
    assert_eq!(
        (client.left, client.top, client.width, client.height),
        (2.0, 2.0, 110.0, 90.0)
    );
    let inner = doc.offset_box(id(&doc, "inner")).unwrap();
    assert_eq!(
        inner.rect,
        rect(8.0, 8.0, 40.0, 20.0),
        "from the padding edge of the parent"
    );
    assert_eq!(inner.parent, Some(outer));
    assert_eq!(
        doc.bounding_rect(id(&doc, "inner")).unwrap(),
        rect(20.0, 20.0, 40.0, 20.0)
    );
    // Fractions survive (snapped to 1/64 px by the document engine).
    assert_eq!(
        doc.bounding_rect(id(&doc, "frac")).unwrap(),
        rect(0.5, 114.0, 33.5, 10.25)
    );
}

#[test]
fn boxless_nodes_have_no_rects_and_empty_geometry() {
    let doc = doc();
    let (none, inside) = (id(&doc, "none"), id(&doc, "nb"));
    let mut rects = Vec::new();
    for key in [none, inside] {
        assert!(
            !doc.has_boxes(key).unwrap(),
            "display:none and its descendants"
        );
        doc.client_rects(key, &mut rects).unwrap();
        assert_eq!(doc.bounding_rect(key).unwrap(), Rect::ZERO);
    }
    assert!(rects.is_empty());
    for key in [none, inside] {
        assert_eq!(doc.client_box(key).unwrap(), Default::default());
        assert_eq!(doc.offset_box(key).unwrap().rect, Rect::ZERO);
        assert_eq!(doc.offset_box(key).unwrap().parent, None);
        assert_eq!(doc.scroll_size(key).unwrap(), Default::default());
        assert_eq!(doc.content_box(key).unwrap(), Default::default());
    }
    // Text nodes have no bounding rectangle of their own.
    let text = doc
        .node(id(&doc, "pad"))
        .unwrap()
        .children()
        .next()
        .unwrap();
    assert_eq!(doc.bounding_rect(text).unwrap(), Rect::ZERO);
    doc.client_rects(id(&doc, "outer"), &mut rects).unwrap();
    assert_eq!(
        rects,
        vec![rect(10.0, 10.0, 114.0, 94.0)],
        "appends to the caller's buffer"
    );
}

#[test]
fn scroll_state_and_overflow() {
    let mut doc = doc();
    let ov = id(&doc, "ov");
    let size = doc.scroll_size(ov).unwrap();
    assert_eq!((size.width, size.height), (200.0, 100.0));
    assert_eq!(doc.scroll_offset(ov).unwrap(), Point::default());
    doc.scroll_node_to(ov, Point { x: 30.0, y: 20.0 }).unwrap();
    assert_eq!(doc.scroll_offset(ov).unwrap(), Point { x: 30.0, y: 20.0 });
    // Scrolling moves the content but not the scroller.
    assert_eq!(doc.bounding_rect(id(&doc, "big")).unwrap().x, -30.0);
    assert_eq!(doc.bounding_rect(ov).unwrap().x, 0.0);

    let before = doc.bounding_rect(id(&doc, "end")).unwrap().y;
    doc.set_viewport_scroll(Point { x: 0.0, y: 150.0 });
    assert_eq!(doc.viewport().scroll, Point { x: 0.0, y: 150.0 });
    assert_eq!(
        doc.bounding_rect(id(&doc, "end")).unwrap().y,
        before - 150.0
    );
}

#[test]
fn viewport_geometry_is_in_css_pixels() {
    let mut doc = doc();
    let v = doc.viewport();
    assert_eq!(
        (v.size.width, v.size.height, v.device_pixel_ratio),
        (300.0, 200.0, 1.0)
    );
    doc.set_viewport(ViewportConfig {
        width_px: 600,
        height_px: 400,
        scale: 2.0,
        zoom: 1.0,
        dark: false,
    });
    let v = doc.viewport();
    assert_eq!(
        (v.size.width, v.size.height, v.device_pixel_ratio),
        (300.0, 200.0, 2.0)
    );
    doc.set_viewport(ViewportConfig {
        width_px: 300,
        height_px: 200,
        scale: 1.0,
        zoom: 2.0,
        dark: false,
    });
    let v = doc.viewport();
    assert_eq!(
        (v.size.width, v.size.height, v.device_pixel_ratio),
        (150.0, 100.0, 2.0),
        "zoom shrinks the CSS viewport"
    );
}

#[test]
fn reads_report_the_last_flush_and_never_relayout() {
    let mut doc = doc();
    let frac = id(&doc, "frac");
    assert_eq!(doc.bounding_rect(frac).unwrap().height, 10.25);
    doc.set_viewport(viewport(300, 200)); // unchanged
    // A style write does not change what a read reports until the flush.
    wega_dom::style::StyleControl::set_inline_property(&mut doc, frac, "height", "50px", false)
        .unwrap();
    assert_eq!(
        doc.bounding_rect(frac).unwrap().height,
        10.25,
        "stale until flushed"
    );
    doc.flush_layout(0.0);
    assert_eq!(doc.bounding_rect(frac).unwrap().height, 50.0);
}

#[test]
fn hit_testing_uses_viewport_coordinates_and_follows_scroll() {
    let mut doc = doc();
    assert_eq!(
        doc.element_at(Point { x: 20.0, y: 20.0 }),
        Some(id(&doc, "inner"))
    );
    assert_eq!(
        doc.element_at(Point { x: 120.0, y: 20.0 }),
        Some(id(&doc, "outer"))
    );
    assert_eq!(
        doc.element_at(Point { x: -1.0, y: 5.0 }),
        None,
        "outside the viewport"
    );
    assert_eq!(doc.element_at(Point { x: 5.0, y: 500.0 }), None);
    // Over no box: an element (the root or a block that fills the viewport), never None.
    assert!(doc.element_at(Point { x: 290.0, y: 190.0 }).is_some());
    // After scrolling, a viewport point maps to a different element.
    let end_y = doc.bounding_rect(id(&doc, "end")).unwrap().y;
    doc.set_viewport_scroll(Point {
        x: 0.0,
        y: end_y - 5.0,
    });
    assert_eq!(
        doc.element_at(Point { x: 10.0, y: 10.0 }),
        Some(id(&doc, "end"))
    );
    assert_eq!(
        doc.element_at(Point { x: 10.0, y: 2.0 }),
        Some(id(&doc, "content")),
        "inside the scroller"
    );
}

#[test]
fn hover_takes_viewport_coordinates_too() {
    let mut doc = doc();
    let end_y = doc.bounding_rect(id(&doc, "end")).unwrap().y;
    doc.set_viewport_scroll(Point {
        x: 0.0,
        y: end_y - 5.0,
    });
    let change = doc.pointer_moved(10.0, 10.0);
    assert_eq!(change.current, Some(id(&doc, "end")));
    assert_eq!(doc.hovered(), Some(id(&doc, "end")));
}

#[test]
fn stale_keys_are_rejected_without_touching_the_document() {
    let mut doc = doc();
    let stale = NodeKey::from_u64(u64::MAX - 1);
    let mut rects = Vec::new();
    assert_eq!(doc.has_boxes(stale), Err(LayoutError::Stale(stale)));
    assert_eq!(doc.bounding_rect(stale), Err(LayoutError::Stale(stale)));
    assert_eq!(
        doc.client_rects(stale, &mut rects),
        Err(LayoutError::Stale(stale))
    );
    assert_eq!(doc.client_box(stale), Err(LayoutError::Stale(stale)));
    assert_eq!(doc.offset_box(stale), Err(LayoutError::Stale(stale)));
    assert_eq!(doc.scroll_size(stale), Err(LayoutError::Stale(stale)));
    assert_eq!(doc.scroll_offset(stale), Err(LayoutError::Stale(stale)));
    assert_eq!(doc.content_box(stale), Err(LayoutError::Stale(stale)));
    assert_eq!(
        doc.scroll_node_to(stale, Point::default()),
        Err(LayoutError::Stale(stale))
    );
    assert_eq!(
        doc.scroll_node_into_view(stale),
        Err(LayoutError::Stale(stale))
    );
    assert!(rects.is_empty());
    assert_eq!(doc.viewport().scroll, Point::default());
}
