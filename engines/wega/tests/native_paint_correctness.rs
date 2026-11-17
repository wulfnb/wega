use anyrender::{ImageRenderer, PaintScene};
use std::{cell::RefCell, rc::Rc};
use wega_adapter_blitz::{BlitzDocument, DocumentSetup};
use wega_dom::{
    DocumentView, NodeView,
    layout::{Point, ViewportConfig},
};
use wega_engine::native_engine::*;
fn document(html: &str) -> NativeDocument {
    let mut d = NativeDocument::new(Rc::new(RefCell::new(BlitzDocument::from_html(
        html,
        DocumentSetup::default(),
    ))));
    d.flush_layout();
    d
}
fn render(d: &mut NativeDocument) -> Vec<u8> {
    let scene = d.record_scene(&ViewportConfig {
        width_px: 200,
        height_px: 200,
        scale: 1.0,
        zoom: 1.0,
        dark: false,
    });
    let mut renderer = anyrender_skia::SkiaImageRenderer::new(200, 200);
    let mut p = Vec::new();
    renderer.render_to_vec(
        |s| s.append_scene(scene.clone(), kurbo::Affine::IDENTITY),
        &mut p,
    );
    p
}
fn pixel(p: &[u8], x: usize, y: usize) -> [u8; 4] {
    p[(y * 200 + x) * 4..(y * 200 + x) * 4 + 4]
        .try_into()
        .unwrap()
}
fn hit(d: &NativeDocument, x: f64, y: f64) -> String {
    let Some(k) = d.element_at(Point { x, y }) else {
        return String::new();
    };
    d.doc
        .borrow()
        .node(k)
        .unwrap()
        .attr("id")
        .unwrap_or("")
        .to_string()
}
#[test]
fn stacking_contexts_and_clips_share_paint_and_hit_order() {
    let mut d = document(
        "<body style='margin:0'><div><div id=high style='position:absolute;left:0;top:0;z-index:10;background:red;width:80px;height:80px'></div></div><div id=low style='position:absolute;left:0;top:0;z-index:1;background:blue;width:80px;height:80px'></div><div style='position:absolute;left:100px;top:0;width:40px;height:40px;overflow:hidden'><div id=clipped style='position:absolute;left:30px;top:0;width:50px;height:50px;z-index:50;background:green'></div></div></body>",
    );
    let p = render(&mut d);
    assert_eq!(pixel(&p, 20, 20), [255, 0, 0, 255]);
    assert_eq!(hit(&d, 20.0, 20.0), "high");
    assert_eq!(pixel(&p, 135, 20), [0, 128, 0, 255]);
    assert_eq!(hit(&d, 135.0, 20.0), "clipped");
    assert_eq!(pixel(&p, 150, 20), [255, 255, 255, 255]);
    assert_ne!(hit(&d, 150.0, 20.0), "clipped");
}
#[test]
fn transform_origin_and_opacity_raster_match_picking() {
    let mut d = document(
        "<body style='margin:0'><div id=scaled style='position:absolute;left:20px;top:20px;width:40px;height:40px;transform:scale(2);background:red'></div><div style='position:absolute;left:100px;top:0;width:60px;height:60px;background:blue;opacity:0.5'><div style='width:40px;height:40px;background:red'></div></div></body>",
    );
    let p = render(&mut d);
    assert_eq!(pixel(&p, 5, 5), [255, 0, 0, 255]);
    assert_eq!(hit(&d, 5.0, 5.0), "scaled");
    let a = pixel(&p, 110, 10);
    assert!(
        a[0] > 240 && (a[1] as i32 - 127).abs() < 3 && (a[2] as i32 - 127).abs() < 3,
        "opacity must composite once: {a:?}"
    );
}
#[test]
fn independent_border_colors_and_shadow_blur() {
    let mut d = document(
        "<body style='margin:0'><div style='position:absolute;left:30px;top:30px;width:60px;height:60px;background:white;border-top:6px solid red;border-left:8px solid blue;border-right:4px solid green;box-shadow:0px 0px 12px black'></div></body>",
    );
    let p = render(&mut d);
    assert_eq!(pixel(&p, 50, 32), [255, 0, 0, 255]);
    assert_eq!(pixel(&p, 32, 50), [0, 0, 255, 255]);
    assert_eq!(pixel(&p, 100, 50), [0, 128, 0, 255]);
    let near = pixel(&p, 26, 60)[0];
    let far = pixel(&p, 15, 60)[0];
    assert!(
        near > 0 && near < 250 && far > near,
        "blur must have a gradient outside the box: {near}/{far}"
    );
}

#[test]
fn hidden_boxes_keep_geometry_and_pointer_events_none_passes_through() {
    let mut d = document(
        "<body style='margin:0'><div id=under style='position:absolute;left:0;top:0;width:80px;height:80px;background:blue'></div><div id=pass style='position:absolute;left:0;top:0;width:80px;height:80px;background:red;pointer-events:none;z-index:2'></div><div id=hidden style='position:absolute;left:100px;top:0;width:80px;height:80px;background:green;visibility:hidden'><div id=visible style='height:20px;background:blue;visibility:visible'></div></div><div id=attribute hidden style='width:100px;height:100px;background:red'></div></body>",
    );
    let p = render(&mut d);
    assert_eq!(pixel(&p, 20, 20), [255, 0, 0, 255]);
    assert_eq!(hit(&d, 20.0, 20.0), "under");
    assert_eq!(pixel(&p, 110, 10), [0, 0, 255, 255]);
    assert_eq!(pixel(&p, 110, 40), [255, 255, 255, 255]);
    let hidden = d.doc.borrow().element_by_id("hidden").unwrap();
    assert_eq!(d.bounding_rect(hidden).unwrap().width, 80.0);
    let attribute = d.doc.borrow().element_by_id("attribute").unwrap();
    assert_eq!(d.bounding_rect(attribute).unwrap().width, 0.0);
}
