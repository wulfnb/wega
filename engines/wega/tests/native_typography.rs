use anyrender::{ImageRenderer, PaintScene};
use std::{cell::RefCell, rc::Rc};
use wega_adapter_blitz::{BlitzDocument, DocumentSetup};
use wega_dom::DocumentView;
use wega_engine::native_engine::*;

fn document(html: &str) -> NativeDocument {
    let mut doc = NativeDocument::new(Rc::new(RefCell::new(BlitzDocument::from_html(
        html,
        DocumentSetup::default(),
    ))));
    doc.flush_layout();
    doc
}
fn widths(html: &str) -> Vec<f64> {
    let doc = document(html);
    let key = doc.doc.borrow().element_by_id("p").unwrap();
    doc.find_box(key)
        .unwrap()
        .lines
        .iter()
        .map(|l| l.rect.width)
        .collect()
}
#[test]
fn whitespace_across_elements_and_unicode() {
    let plain = widths("<p id=p>Hello world</p>");
    assert_eq!(plain, widths("<p id=p>  Hello <span>  world </span> </p>"));
    assert_eq!(
        widths("<p id=p>Hello<span>world</span></p>"),
        widths("<p id=p>Helloworld</p>")
    );
    assert!(widths("<p id=p>Hello&nbsp;world</p>")[0] > widths("<p id=p>Helloworld</p>")[0]);
    let composed = widths("<p id=p>café</p>");
    let combining = widths("<p id=p>cafe\u{301}</p>");
    assert!((composed[0] - combining[0]).abs() < 0.1);
    assert_eq!(widths("<p id=p>A<br>B</p>").len(), 2);
    assert_eq!(widths("<pre id=p>A  B\nC</pre>").len(), 2);
    assert!(widths("<pre id=p>A  B</pre>")[0] > widths("<p id=p>A  B</p>")[0]);
    assert_eq!(
        widths("<p id=p style='width:30px;white-space:nowrap'>Hello world</p>").len(),
        1
    );
}
#[test]
fn font_metrics_and_style_runs() {
    assert!(widths("<p id=p>WWW</p>")[0] > 2.0 * widths("<p id=p>iii</p>")[0]);
    let doc = document(
        "<p id=p style='width:200px'>English <b>bold</b> <i>italic</i> <span style='font-size:32px'>large</span> café العربية שלום 你好</p>",
    );
    let key = doc.doc.borrow().element_by_id("p").unwrap();
    let b = doc.find_box(key).unwrap();
    assert!(b.lines.len() > 1);
    let runs: Vec<_> = b.lines.iter().flat_map(|l| &l.text_runs).collect();
    assert!(runs.iter().any(|r| r.font_size == 32.0));
    assert!(runs.iter().all(|r| !r.glyphs.is_empty()));
    assert!(runs.iter().flat_map(|r| &r.glyphs).any(|g| g.id != 0));
    assert!(
        runs.iter()
            .all(|r| r.glyphs.iter().all(|g| g.x.is_finite() && g.y.is_finite()))
    );
}
#[test]
fn readable_glyph_raster_and_visual_evidence() {
    let html = include_str!("fixtures/native-typography.html");
    let mut doc = document(html);
    let viewport = wega_dom::layout::ViewportConfig {
        width_px: 720,
        height_px: 540,
        scale: 1.0,
        zoom: 1.0,
        dark: false,
    };
    let scene = doc.record_scene(&viewport);
    let mut renderer = anyrender_skia::SkiaImageRenderer::new(720, 540);
    let mut pixels = Vec::new();
    renderer.render_to_vec(
        |p| p.append_scene(scene.clone(), kurbo::Affine::IDENTITY),
        &mut pixels,
    );
    // Readable text must contain both ink and internal gaps on a scanline through
    // a glyph row; the old solid rectangle cannot pass this assertion.
    let mut transitions = 0;
    for y in 25..50 {
        let mut last = false;
        for x in 24..400 {
            let i = (y * 720 + x) * 4;
            let ink = pixels[i] < 100
                && pixels[i + 1] < 100
                && pixels[i + 2] < 100
                && pixels[i + 3] > 100;
            if ink != last {
                transitions += 1;
            }
            last = ink;
        }
    }
    assert!(transitions > 100, "glyph contours missing: {transitions}");
    if let Ok(path) = std::env::var("WEGA_NATIVE_EVIDENCE_DIR") {
        std::fs::create_dir_all(&path).unwrap();
        let info = skia_safe::ImageInfo::new(
            (720, 540),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let image =
            skia_safe::images::raster_from_data(&info, skia_safe::Data::new_copy(&pixels), 720 * 4)
                .unwrap();
        let png = image
            .encode(None, skia_safe::EncodedImageFormat::PNG, None)
            .unwrap();
        std::fs::write(
            std::path::Path::new(&path).join("typography-native.png"),
            png.as_bytes(),
        )
        .unwrap();
    }
}
