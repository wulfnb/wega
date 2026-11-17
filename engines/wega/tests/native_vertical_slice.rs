//! Checkpoint A: Native Vertical Slice Integration Tests.
mod support;
use std::cell::RefCell;
use std::rc::Rc;
use url::Url;
use wega_adapter_blitz::{BlitzDocument, DocumentSetup};
use wega_dom::DocumentView;
use wega_dom::layout::ViewportConfig;
use wega_engine::RenderingBackend;
use wega_engine::native_engine::*;

#[test]
fn test_css_selectors_and_specificity() {
    let css = r#"
        * { box-sizing: border-box; }
        div { display: block; color: black; }
        .box { padding: 10px; width: 200px; }
        #target { background-color: #ff0000; width: 300px !important; }
        [data-role="banner"] { height: 50px; }
        header > .nav { display: flex; }
    "#;
    let sheet = StyleSheet::parse(css);
    assert_eq!(sheet.rules.len(), 6);

    // Verify specificity
    let universal = &sheet.rules[0].selectors[0];
    assert_eq!(universal.specificity(), (0, 0, 0));

    let tag = &sheet.rules[1].selectors[0];
    assert_eq!(tag.specificity(), (0, 0, 1));

    let class = &sheet.rules[2].selectors[0];
    assert_eq!(class.specificity(), (0, 1, 0));

    let id = &sheet.rules[3].selectors[0];
    assert_eq!(id.specificity(), (1, 0, 0));

    let attr = &sheet.rules[4].selectors[0];
    assert_eq!(attr.specificity(), (0, 1, 0));

    let child = &sheet.rules[5].selectors[0];
    assert_eq!(child.specificity(), (0, 1, 1));
}

#[test]
fn test_css_cascade_and_inheritance() {
    let html = r#"
        <!doctype html>
        <html>
        <head>
            <style>
                body { color: blue; font-size: 20px; }
                .card { background-color: #00ff00; width: 400px; }
                #special { width: 500px; color: red; }
            </style>
        </head>
        <body>
            <div id="special" class="card" style="padding: 15px;">
                <p>Nested text</p>
            </div>
        </body>
        </html>
    "#;
    let doc = BlitzDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.sync_author_stylesheets();
    assert_eq!(native_doc.author_sheets.len(), 1);

    let d = native_doc.doc.borrow();
    let sheets_ref: Vec<&StyleSheet> = native_doc.author_sheets.iter().collect();
    let special_key = d.element_by_id("special").unwrap();
    let style = resolve_computed_style(&*d, special_key, None, &sheets_ref);

    // ID selector beats class selector for width
    assert_eq!(style.width, Length::Px(500.0));
    // Color resolved from #special
    assert_eq!(
        style.color,
        Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255
        }
    );
    // Inline padding resolved
    assert_eq!(style.padding.top, Length::Px(15.0));
    // Background color from .card
    assert_eq!(
        style.background_color,
        Color {
            r: 0,
            g: 255,
            b: 0,
            a: 255
        }
    );
}

#[test]
fn test_block_and_inline_layout() {
    let html = r#"
        <!doctype html>
        <body>
            <div id="container" style="width: 600px; padding: 20px; border: 5px solid black;">
                <p id="para" style="margin: 10px; font-size: 16px;">Hello Wega Native Layout</p>
            </div>
        </body>
    "#;
    let doc = BlitzDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.flush_layout();

    let container_key = native_doc.doc.borrow().element_by_id("container").unwrap();
    let rect = native_doc.bounding_rect(container_key).unwrap();

    // 600px content + 40px padding + 10px borders = 650px border_box width
    assert_eq!(rect.width, 650.0);
    assert!(rect.height > 20.0);

    let client = native_doc.client_box(container_key).unwrap();
    assert_eq!(client.width, 640.0); // client width = content + padding (600 + 40)
    assert_eq!(client.left, 5.0); // left border width
}

#[test]
fn test_display_list_and_skia_scene() {
    let html = r#"
        <!doctype html>
        <body style="margin: 0; background-color: #ffffff;">
            <div id="box" style="width: 200px; height: 100px; background-color: #ff0000; border: 2px solid #0000ff; border-radius: 8px;">
                <span style="color: #000000;">Sample Text</span>
            </div>
        </body>
    "#;
    let doc = BlitzDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));

    let viewport = ViewportConfig {
        width_px: 800,
        height_px: 600,
        scale: 1.0,
        zoom: 1.0,
        dark: false,
    };
    let scene = native_doc.record_scene(&viewport);

    // Verify scene is non-empty and rasterizable
    let mut renderer = anyrender_skia::SkiaImageRenderer::new(800, 600);
    use anyrender::ImageRenderer;
    let mut pixmap = Vec::new();
    renderer.render_to_vec(
        |painter| {
            use anyrender::PaintScene;
            painter.append_scene(scene.clone(), kurbo::Affine::IDENTITY);
        },
        &mut pixmap,
    );
    assert_eq!(pixmap.len(), 800 * 600 * 4);

    // Check pixel at (50, 50) inside the red box: R=255, G=0, B=0, A=255
    let offset = (50 * 800 + 50) * 4;
    assert_eq!(pixmap[offset], 255); // Red
    assert_eq!(pixmap[offset + 1], 0); // Green
    assert_eq!(pixmap[offset + 2], 0); // Blue
    assert_eq!(pixmap[offset + 3], 255); // Alpha
}

#[test]
fn test_view_backend_selector() {
    let engine = support::engine();
    let mut view = engine
        .create_view(Url::parse("about:blank").unwrap())
        .unwrap();

    // Default backend is Blitz
    assert_eq!(view.rendering_backend(), RenderingBackend::Blitz);

    // Switch to Native Wega engine
    view.set_rendering_backend(RenderingBackend::WegaNative);
    assert_eq!(view.rendering_backend(), RenderingBackend::WegaNative);

    // Paint using WegaNative backend
    let painted = view.paint(800, 600, 1.0, 1.0, false);
    assert!(
        painted.is_ok(),
        "Painting with native Wega backend succeeded: {:?}",
        painted.err()
    );

    // Switch back to Blitz
    view.set_rendering_backend(RenderingBackend::Blitz);
    assert_eq!(view.rendering_backend(), RenderingBackend::Blitz);
    let painted_blitz = view.paint(800, 600, 1.0, 1.0, false);
    assert!(
        painted_blitz.is_ok(),
        "Painting with fallback Blitz backend succeeded"
    );
}
