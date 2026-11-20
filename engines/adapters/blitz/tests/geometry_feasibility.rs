//! D3-C.0 geometry feasibility: can an inspector get the four CSS boxes, in viewport CSS pixels,
//! from *cached* layout only (no flush), with scroll, zoom and device scale handled, and with
//! honest freshness? Each test prints the measured values (`--nocapture`) that the D3-C.0 report
//! quotes, and asserts the properties the design relies on.
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use wega_adapter_blitz::{BlitzDocument, ToKey};
use wega_dom::{
    DocumentView, Mutator, NodeKey,
    layout::{BoxModel, Geometry, GeometryAccess, HitTest, LayoutControl, Point, ViewportConfig},
};

fn doc_with(html: &str, scale: f32, zoom: f32) -> BlitzDocument {
    let mut doc =
        BlitzDocument::new(HtmlDocument::from_html(html, DocumentConfig::default()).into_inner());
    doc.set_viewport(ViewportConfig {
        width_px: 800,
        height_px: 600,
        scale,
        zoom,
        dark: false,
    });
    doc.flush_layout(0.0);
    doc
}
fn by_id(doc: &BlitzDocument, id: &str) -> NodeKey {
    doc.get_element_by_id(id).unwrap().key()
}
fn current(doc: &BlitzDocument, id: &str) -> BoxModel {
    match doc.inspect_geometry(by_id(doc, id)) {
        Geometry::Current(b) => b,
        other => panic!("{id}: {other:?}"),
    }
}
const PAGE: &str = "<!doctype html><style>body{margin:0}\
#a{margin:10px 20px 30px 40px;padding:5px 6px 7px 8px;border:2px solid;width:100px;height:50px}\
#sp{height:2000px}#none{display:none}#clip{width:50px;height:20px;overflow:hidden}\
#wide{width:200px;height:10px}#tr{transform:translate(30px,40px);width:10px;height:10px}\
#sc{width:100px;height:60px;overflow:scroll}#in{height:300px;margin-top:100px}</style>\
<div id=a>x</div><div id=none></div><div id=clip><div id=wide></div></div><div id=tr></div>\
<div id=sc><div id=in></div></div><div id=sp></div>";

#[test]
fn the_four_boxes_come_from_cached_layout_in_css_pixels() {
    let doc = doc_with(PAGE, 1.0, 1.0);
    let b = current(&doc, "a");
    println!("scale1 border_box={:?}", b.border_box);
    println!("scale1 padding_box={:?}", b.padding_box());
    println!("scale1 content_box={:?}", b.content_box());
    println!("scale1 margin_box={:?}", b.margin_box());
    assert_eq!(b.border_box.x, 40.0);
    assert_eq!(b.border_box.y, 10.0);
    assert_eq!(b.border_box.width, 100.0 + 14.0 + 4.0);
    assert_eq!(b.border_box.height, 50.0 + 12.0 + 4.0);
    assert_eq!(b.content_box().width, 100.0);
    assert_eq!(b.content_box().height, 50.0);
    assert_eq!(b.content_box().x, 40.0 + 2.0 + 8.0);
    assert_eq!(b.margin_box().x, 0.0);
    assert_eq!(b.margin_box().width, 178.0);
}

#[test]
fn device_scale_and_zoom_do_not_change_css_pixel_geometry_except_zoom() {
    for (scale, zoom) in [(1.0, 1.0), (2.0, 1.0), (1.0, 1.5), (2.0, 1.5)] {
        let doc = doc_with(PAGE, scale, zoom);
        let b = current(&doc, "a");
        let v = doc.viewport();
        println!(
            "scale={scale} zoom={zoom} border_box={:?} margin={:?} viewport_css={:?} dpr={}",
            b.border_box, b.margin, v.size, v.device_pixel_ratio
        );
    }
    // Device scale alone is invisible in CSS pixels.
    assert_eq!(
        current(&doc_with(PAGE, 2.0, 1.0), "a"),
        current(&doc_with(PAGE, 1.0, 1.0), "a")
    );
}

#[test]
fn viewport_scroll_moves_viewport_relative_boxes_and_hit_testing_agrees() {
    let mut doc = doc_with(PAGE, 1.0, 1.0);
    let before = current(&doc, "a").border_box;
    doc.set_viewport_scroll(Point { x: 0.0, y: 5.0 });
    doc.flush_layout(0.0);
    let after = current(&doc, "a").border_box;
    println!("scroll y=5: border_box.y {} -> {}", before.y, after.y);
    assert_eq!(after.y, before.y - 5.0);
    // The centre of the reported box hits the element: picker and highlight share one space.
    let c = Point {
        x: after.x + after.width / 2.0,
        y: after.y + after.height / 2.0,
    };
    assert_eq!(doc.element_at(c), Some(by_id(&doc, "a")));
}

#[test]
fn nested_scroll_offsets_are_reflected_and_transforms_and_clips_are_not() {
    let mut doc = doc_with(PAGE, 1.0, 1.0);
    let inner = current(&doc, "in").border_box;
    doc.scroll_node_to(by_id(&doc, "sc"), Point { x: 0.0, y: 40.0 })
        .unwrap();
    doc.flush_layout(0.0);
    let scrolled = current(&doc, "in").border_box;
    println!("inner scroll 40: y {} -> {}", inner.y, scrolled.y);
    assert_eq!(scrolled.y, inner.y - 40.0);
    // Clipping is NOT in the box: #wide is 200px wide inside a 50px overflow:hidden parent.
    let wide = current(&doc, "wide").border_box;
    let clip = current(&doc, "clip").border_box;
    println!(
        "clipped child width={} clip parent width={}",
        wide.width, clip.width
    );
    assert!(
        wide.width > clip.width,
        "overlay must intersect ancestor clips itself"
    );
    // Transforms: measured, recorded; the unit test pins the current behaviour.
    let tr = current(&doc, "tr").border_box;
    println!("transform translate(30,40) border_box={tr:?}");
    // Pinned limitation: the box ignores the transform, so a highlight over a transformed element
    // (or its descendants) would be misplaced. D3-C must detect transforms and say so.
    assert_eq!(
        tr.x, 0.0,
        "if this fails the engine now applies transforms; update the design"
    );
}

#[test]
fn freshness_states_are_distinguished_without_flushing() {
    // Pending: never flushed.
    let unflushed =
        BlitzDocument::new(HtmlDocument::from_html(PAGE, DocumentConfig::default()).into_inner());
    let key = by_id(&unflushed, "a");
    assert_eq!(unflushed.inspect_geometry(key), Geometry::Pending);
    // NoBoxes.
    let mut doc = doc_with(PAGE, 1.0, 1.0);
    assert_eq!(doc.inspect_geometry(by_id(&doc, "none")), Geometry::NoBoxes);
    // Invalid: a key that does not exist.
    assert_eq!(
        doc.inspect_geometry(NodeKey::from_u64(u64::MAX)),
        Geometry::Invalid
    );
    // Stale: after a mutation, still the old boxes, labelled.
    let a = by_id(&doc, "a");
    let Geometry::Current(old) = doc.inspect_geometry(a) else {
        panic!()
    };
    wega_dom::mutate(&mut doc)
        .set_attribute(
            a,
            html5ever::QualName::new(None, html5ever::ns!(), html5ever::LocalName::from("style")),
            "width:300px",
        )
        .unwrap();
    assert_eq!(doc.inspect_geometry(a), Geometry::Stale(old));
    doc.flush_layout(0.0);
    assert!(
        matches!(doc.inspect_geometry(a), Geometry::Current(b) if b.border_box.width > old.border_box.width)
    );
}

#[test]
fn a_document_past_the_depth_limit_reports_refused_not_a_hang() {
    let html = format!(
        "<!doctype html><body>{}x{}",
        "<div>".repeat(400),
        "</div>".repeat(400)
    );
    let mut doc =
        BlitzDocument::new(HtmlDocument::from_html(&html, DocumentConfig::default()).into_inner());
    doc.flush_layout(0.0);
    assert_eq!(doc.inspect_geometry(doc.root()), Geometry::Refused);
}

#[test]
fn reads_are_cheap_and_do_not_depend_on_document_size() {
    let html = format!("<!doctype html><body>{}", "<p id=p>x</p>".repeat(5000));
    let doc = doc_with(&html, 1.0, 1.0);
    let key = by_id(&doc, "p");
    let started = std::time::Instant::now();
    for _ in 0..1000 {
        std::hint::black_box(doc.inspect_geometry(key));
    }
    println!(
        "1000 inspect_geometry reads on 5000 paragraphs: {:?}",
        started.elapsed()
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

const PAINT: &str = "<!doctype html><style>body{margin:0}\
#plain{width:10px;height:10px}\
#tr{transform:translate(30px,40px);width:10px;height:10px}\
#trchild{width:5px;height:5px}\
#sc{transform:scale(2);width:10px;height:10px}\
#rot{transform:rotate(10deg);width:10px;height:10px}\
#tprop{translate:5px 5px;width:10px;height:10px}\
#clip{width:50px;height:20px;overflow:hidden;border:3px solid;margin-left:7px}\
#inner{width:200px;height:10px}\
#abs{position:absolute;width:5px;height:5px}\
#fix{position:fixed;width:5px;height:5px}\
#scroller{width:100px;height:60px;overflow:scroll}#deep{height:300px}</style>\
<div id=plain></div><div id=tr><div id=trchild></div></div><div id=sc></div><div id=rot></div>\
<div id=tprop></div><div id=clip><div id=inner></div><div id=abs></div></div><div id=fix></div>\
<div id=scroller><div id=deep></div></div>";

#[test]
fn paint_mapping_is_exact_or_explicitly_unsupported_never_misleading() {
    use wega_dom::layout::{PaintMapping, UnsupportedPaint};
    let doc = doc_with(PAINT, 1.0, 1.0);
    let map = |id: &str| doc.paint_mapping(by_id(&doc, id));
    assert_eq!(map("plain"), PaintMapping::Exact { clip: None });
    for id in ["tr", "trchild", "sc", "rot", "tprop"] {
        assert_eq!(
            map(id),
            PaintMapping::Unsupported(UnsupportedPaint::Transform),
            "{id}"
        );
    }
    // Clipped by an `overflow:hidden` ancestor: the clip is that ancestor's padding box.
    let clip_box = current(&doc, "clip");
    let PaintMapping::Exact { clip: Some(c) } = map("inner") else {
        panic!("{:?}", map("inner"))
    };
    assert_eq!(
        c,
        wega_dom::layout::Rect {
            x: clip_box.border_box.x + 3.0,
            y: clip_box.border_box.y + 3.0,
            width: 50.0,
            height: 20.0
        }
    );
    assert_eq!(c.x, 10.0, "margin 7 plus border 3");
    // An absolutely positioned node under a clipping ancestor is not modelled.
    assert_eq!(
        map("abs"),
        PaintMapping::Unsupported(UnsupportedPaint::ClipPositioned)
    );
    // A fixed node with no clipping ancestor is fine: boxes are viewport-relative already.
    assert_eq!(map("fix"), PaintMapping::Exact { clip: None });
    // The scroll container clips its scrolled content to its padding box.
    let PaintMapping::Exact { clip: Some(sc) } = map("deep") else {
        panic!()
    };
    assert_eq!((sc.width, sc.height), (100.0, 60.0));
    // Unresolved style: unknown, never a guess.
    let unresolved =
        BlitzDocument::new(HtmlDocument::from_html(PAINT, DocumentConfig::default()).into_inner());
    assert_eq!(
        unresolved.paint_mapping(by_id(&unresolved, "plain")),
        PaintMapping::Unsupported(UnsupportedPaint::Unknown)
    );
}

#[test]
fn a_clipped_child_is_reported_with_the_clip_that_hides_it() {
    use wega_dom::layout::PaintMapping;
    let doc = doc_with(PAINT, 1.0, 1.0);
    let inner = current(&doc, "inner").border_box;
    let PaintMapping::Exact { clip: Some(c) } = doc.paint_mapping(by_id(&doc, "inner")) else {
        panic!()
    };
    // 200 px wide box, 50 px visible: an overlay must be cut to the clip.
    assert!(inner.width > c.width);
    println!("clip {c:?} child {inner:?}");
}
