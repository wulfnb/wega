//! Rendering through `View::paint` / `begin_paint`: what the painter and the Skia backend put
//! in the pixels, and the frame lifecycle around it. Blitz records the scene (behind
//! `wega_dom::render::PaintSource`), Skia rasterises it on a dedicated thread. Solid fills are
//! asserted exactly (Skia does not anti-alias the inside of an axis-aligned rectangle);
//! everything else is a reftest against a reference page painted by the same code path, so no
//! platform-specific screenshot tolerance hides a difference.
use std::sync::Arc;
use wega_engine::{Engine, FrameTarget, Limits, NetworkPolicy, Pixels, Url, View, ViewportConfig};

fn page(html: &str) -> View {
    let mut view = Engine::new(
        Limits::default(),
        NetworkPolicy {
            offline: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .create_view(Url::parse("about:blank").unwrap())
    .unwrap();
    view.set_html(&format!(
        "<!doctype html><style>body{{margin:0}}div{{box-sizing:border-box}}</style>{html}"
    ))
    .unwrap();
    view.set_transparent(true);
    view.poll();
    view
}
fn px(frame: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    [frame[i], frame[i + 1], frame[i + 2], frame[i + 3]]
}
fn abs(left: u32, top: u32, width: u32, height: u32, style: &str) -> String {
    format!(
        "<div style='position:absolute;left:{left}px;top:{top}px;width:{width}px;height:{height}px;{style}'></div>"
    )
}
const CLEAR: [u8; 4] = [0, 0, 0, 0];

#[test]
fn backgrounds_and_borders_fill_their_boxes() {
    let mut v = page(&abs(
        10,
        10,
        24,
        24,
        "background:#f00;border:2px solid #00f",
    ));
    let f = v.paint(100, 100, 1.0, 1.0, false).unwrap();
    assert_eq!((f.width, f.height, f.rgba.len()), (100, 100, 40_000));
    assert_eq!(
        px(&f.rgba, 100, 0, 0),
        CLEAR,
        "an unpainted page is transparent"
    );
    assert_eq!(px(&f.rgba, 100, 11, 20), [0, 0, 255, 255], "border");
    assert_eq!(px(&f.rgba, 100, 20, 20), [255, 0, 0, 255], "background");
    assert_eq!(px(&f.rgba, 100, 9, 20), CLEAR);
    assert_eq!(px(&f.rgba, 100, 34, 20), CLEAR, "the border box ends at 34");
}

#[test]
fn overflow_clips_descendants() {
    let mut v = page(
        "<div style='position:absolute;left:10px;top:10px;width:20px;height:20px;overflow:hidden'><div style='width:50px;height:50px;background:#0f0'></div></div>",
    );
    let f = v.paint(100, 100, 1.0, 1.0, false).unwrap();
    assert_eq!(px(&f.rgba, 100, 15, 15), [0, 255, 0, 255]);
    assert_eq!(
        px(&f.rgba, 100, 35, 15),
        CLEAR,
        "clipped at the padding box"
    );
    assert_eq!(px(&f.rgba, 100, 15, 35), CLEAR);
}

#[test]
fn opacity_and_transforms_apply_to_painting_but_not_to_layout() {
    let mut v = page(&format!(
        "{}{}",
        abs(10, 10, 20, 20, "background:#000;opacity:.5"),
        abs(
            50,
            50,
            20,
            20,
            "background:#f0f;transform:translate(10px,10px)"
        )
    ));
    let f = v.paint(100, 100, 1.0, 1.0, false).unwrap();
    assert_eq!(px(&f.rgba, 100, 20, 20), [0, 0, 0, 128], "50% black");
    assert_eq!(
        px(&f.rgba, 100, 55, 55),
        CLEAR,
        "the untransformed position is empty"
    );
    assert_eq!(
        px(&f.rgba, 100, 65, 65),
        [255, 0, 255, 255],
        "painted translated"
    );
}

#[test]
fn text_paints_glyphs_in_its_colour_and_is_deterministic() {
    let html = "<div style='position:absolute;left:4px;top:4px;font:20px sans-serif;color:#00f'>Hello</div>";
    let a = page(html).paint(100, 40, 1.0, 1.0, false).unwrap().rgba;
    let b = page(html).paint(100, 40, 1.0, 1.0, false).unwrap().rgba;
    assert_eq!(a, b, "the same page paints the same pixels");
    let inked = a.chunks(4).filter(|p| p[3] > 0).count();
    assert!(inked > 100, "glyph pixels were painted ({inked})");
    assert!(
        a.chunks(4)
            .filter(|p| p[3] == 255)
            .all(|p| p[0] == 0 && p[1] == 0 && p[2] == 255),
        "opaque glyph pixels are the text colour"
    );
    let none = page("<div style='position:absolute;left:4px;top:4px;font:20px sans-serif;color:#00f;visibility:hidden'>Hello</div>")
        .paint(100, 40, 1.0, 1.0, false)
        .unwrap()
        .rgba;
    assert!(
        none.chunks(4).all(|p| p[3] == 0),
        "hidden text paints nothing"
    );
}

#[test]
fn images_paint_once_decoded() {
    let mut v = page(
        "<img style='position:absolute;left:10px;top:10px' width=10 height=10 src=\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='10'%3E%3Crect width='10' height='10' fill='%23ff8000'/%3E%3C/svg%3E\">",
    );
    for _ in 0..20 {
        v.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let f = v.paint(40, 40, 1.0, 1.0, false).unwrap();
    assert_eq!(px(&f.rgba, 40, 15, 15), [255, 128, 0, 255]);
    assert_eq!(px(&f.rgba, 40, 25, 25), CLEAR);
}

#[test]
fn scrolling_moves_the_painted_page() {
    let mut v = page(&format!(
        "{}<div style='height:600px'></div>",
        abs(10, 150, 20, 20, "background:#f00")
    ));
    let before = v.paint(100, 100, 1.0, 1.0, false).unwrap().rgba;
    assert_eq!(px(&before, 100, 20, 60), CLEAR, "below the fold");
    v.evaluate("scrollTo(0,100)").unwrap();
    let after = v.paint(100, 100, 1.0, 1.0, false).unwrap().rgba;
    assert_eq!(
        px(&after, 100, 20, 60),
        [255, 0, 0, 255],
        "scrolled into view at 150-100"
    );
    assert_eq!(px(&after, 100, 20, 40), CLEAR);
}

#[test]
fn an_overflow_container_scrolls_its_own_content_and_clips_it() {
    let mut v = page(
        "<div id=s style='position:absolute;left:10px;top:10px;width:40px;height:40px;overflow:auto'><div style='height:40px;background:#f00'></div><div style='height:40px;background:#0f0'></div></div>",
    );
    let a = v.paint(80, 80, 1.0, 1.0, false).unwrap().rgba;
    assert_eq!(px(&a, 80, 20, 20), [255, 0, 0, 255]);
    v.evaluate("document.getElementById('s').scrollTop=40")
        .unwrap();
    let b = v.paint(80, 80, 1.0, 1.0, false).unwrap().rgba;
    assert_eq!(px(&b, 80, 20, 20), [0, 255, 0, 255], "scrolled content");
    assert_eq!(
        px(&b, 80, 20, 55),
        CLEAR,
        "the container clips below its box"
    );
}

#[test]
fn high_dpi_scales_painting_by_the_device_ratio() {
    let mut v = page(&abs(10, 10, 20, 20, "background:#f00"));
    let f = v.paint(200, 200, 2.0, 1.0, false).unwrap();
    assert_eq!((f.width, f.height), (200, 200));
    assert_eq!(
        px(&f.rgba, 200, 20, 20),
        [255, 0, 0, 255],
        "starts at 10 css px = 20 device px"
    );
    assert_eq!(px(&f.rgba, 200, 19, 30), CLEAR);
    assert_eq!(
        px(&f.rgba, 200, 59, 30),
        [255, 0, 0, 255],
        "ends at 30 css px = 60 device px"
    );
    assert_eq!(px(&f.rgba, 200, 60, 30), CLEAR);
    let z = v.paint(100, 100, 1.0, 2.0, false).unwrap();
    assert_eq!(
        px(&z.rgba, 100, 20, 20),
        [255, 0, 0, 255],
        "page zoom 2 scales like a 2x display"
    );
    assert_eq!(px(&z.rgba, 100, 60, 30), CLEAR);
}

#[test]
fn resizing_repaints_at_the_new_size_with_reflowed_content() {
    let mut v = page("<div style='height:10px;background:#f00'></div>");
    let small = v.paint(40, 20, 1.0, 1.0, false).unwrap();
    assert_eq!((small.width, small.height), (40, 20));
    assert_eq!(px(&small.rgba, 40, 39, 5), [255, 0, 0, 255]);
    let large = v.paint(120, 30, 1.0, 1.0, false).unwrap();
    assert_eq!(
        (large.width, large.height, large.rgba.len()),
        (120, 30, 14_400)
    );
    assert_eq!(
        px(&large.rgba, 120, 119, 5),
        [255, 0, 0, 255],
        "a block follows the viewport width"
    );
    assert_eq!(px(&large.rgba, 120, 119, 15), CLEAR);
    let back = v.paint(40, 20, 1.0, 1.0, false).unwrap();
    assert_eq!(
        back.rgba, small.rgba,
        "returning to a size reproduces its pixels"
    );
}

#[test]
fn dynamic_style_and_layout_updates_repaint() {
    let mut v = page(&abs(10, 10, 20, 20, "background:#f00").replace("<div ", "<div id=a "));
    v.paint(60, 60, 1.0, 1.0, false).unwrap();
    assert!(!v.needs_paint());
    v.evaluate("a.style.background='#00f';a.style.left='30px'")
        .unwrap();
    assert!(v.needs_paint(), "a mutation asks for a frame");
    let f = v.paint(60, 60, 1.0, 1.0, false).unwrap().rgba;
    assert_eq!(px(&f, 60, 20, 20), CLEAR, "old position cleared");
    assert_eq!(
        px(&f, 60, 40, 20),
        [0, 0, 255, 255],
        "new colour at the new position"
    );
    assert!(!v.needs_paint());
}

#[test]
fn replacing_the_document_paints_only_the_new_document() {
    let mut v = page(&abs(10, 10, 20, 20, "background:#f00"));
    v.paint(60, 60, 1.0, 1.0, false).unwrap();
    v.set_html("<!doctype html><style>body{margin:0}</style><div style='width:20px;height:20px;background:#0f0'></div>")
        .unwrap();
    v.poll();
    assert!(v.needs_paint(), "a new document needs its first frame");
    let f = v.paint(60, 60, 1.0, 1.0, false).unwrap().rgba;
    assert_eq!(px(&f, 60, 5, 5), [0, 255, 0, 255]);
    assert_eq!(
        px(&f, 60, 20, 20),
        CLEAR,
        "nothing of the old document remains"
    );
}

#[test]
fn shadow_tree_content_is_not_painted_but_light_children_are() {
    let mut v = page(
        "<div id=h style='width:30px;height:30px'><div style='width:10px;height:10px;background:#f00'></div></div><script>const r=document.getElementById('h').attachShadow({mode:'open'});r.innerHTML='<div style=\"position:absolute;left:50px;top:0;width:10px;height:10px;background:#00f\"></div>'</script>",
    );
    let f = v.paint(80, 40, 1.0, 1.0, false).unwrap().rgba;
    assert_eq!(
        px(&f, 80, 5, 5),
        [255, 0, 0, 255],
        "light children render unflattened"
    );
    assert_eq!(
        px(&f, 80, 55, 5),
        CLEAR,
        "known gap: shadow content has no layout and no paint"
    );
}

// ---- frame submission and lifecycle ---------------------------------------------------

fn viewport(width: u32, height: u32) -> ViewportConfig {
    ViewportConfig {
        width_px: width,
        height_px: height,
        scale: 1.0,
        zoom: 1.0,
        dark: false,
    }
}

#[test]
fn begin_paint_rejects_invalid_viewports_without_starting_a_frame() {
    let mut v = page("<div></div>");
    for bad in [
        viewport(0, 10),
        viewport(10, 0),
        ViewportConfig {
            scale: 0.0,
            ..viewport(10, 10)
        },
        ViewportConfig {
            scale: f32::NAN,
            ..viewport(10, 10)
        },
        ViewportConfig {
            zoom: 0.1,
            ..viewport(10, 10)
        },
        ViewportConfig {
            zoom: 9.0,
            ..viewport(10, 10)
        },
        viewport(100_000, 100_000),
    ] {
        assert!(
            v.begin_paint(bad, 1, FrameTarget::Owned).is_err(),
            "{bad:?}"
        );
        assert!(!v.paint_pending());
    }
}

#[test]
fn one_frame_is_in_flight_and_its_result_carries_the_request() {
    let mut v = page(&abs(0, 0, 10, 10, "background:#f00"));
    v.begin_paint(viewport(30, 20), 7, FrameTarget::Owned)
        .unwrap();
    assert!(v.paint_pending());
    assert!(
        v.begin_paint(viewport(30, 20), 8, FrameTarget::Owned)
            .is_err(),
        "a second frame cannot start while one is rasterising"
    );
    let painted = v.wait_painted().unwrap();
    assert!(!v.paint_pending());
    assert_eq!((painted.serial, painted.width, painted.height), (7, 30, 20));
    match painted.pixels {
        Pixels::Owned(rgba) => {
            assert_eq!(rgba.len(), 30 * 20 * 4);
            assert_eq!(px(&rgba, 30, 5, 5), [255, 0, 0, 255]);
        }
        Pixels::Shared { .. } => panic!("an Owned target returns owned pixels"),
    }
}

#[test]
fn shared_slot_frames_match_owned_frames_pixel_for_pixel() {
    let capacity = 4096 * 2160 * 4;
    let store = wega_platform::FrameStore::create(capacity).unwrap();
    let path = store.path().unwrap().to_owned();
    let mut v = page(&format!(
        "{}<div style='position:absolute;left:4px;top:30px;font:16px sans-serif;color:#222'>shared</div>",
        abs(10, 10, 30, 12, "background:#f00;border:1px solid #00f")
    ));
    v.configure_shared_frames(&path).unwrap();
    v.begin_paint(
        viewport(80, 60),
        3,
        FrameTarget::Shared {
            slot: 1,
            sequence: 9,
        },
    )
    .unwrap();
    let painted = v.wait_painted().unwrap();
    let Pixels::Shared { slot, sequence } = painted.pixels else {
        panic!("a Shared target returns a slot")
    };
    assert_eq!((slot, sequence, painted.serial), (1, 9, 3));
    let shared = store
        .into_reader()
        .read_slot(slot as usize, 80 * 60 * 4)
        .unwrap();
    let owned = v.paint(80, 60, 1.0, 1.0, false).unwrap().rgba;
    assert_eq!(shared, owned, "the transport does not change the pixels");
}

#[test]
fn frame_requests_follow_dirtiness_freezing_and_explicit_redraws() {
    let mut v = page(&abs(0, 0, 10, 10, "background:#f00"));
    assert!(v.needs_paint(), "a new view has never painted");
    v.paint(40, 40, 1.0, 1.0, false).unwrap();
    assert!(!v.needs_paint());
    v.request_redraw();
    assert!(v.needs_paint());
    v.paint(40, 40, 1.0, 1.0, false).unwrap();
    v.freeze(true);
    v.request_redraw();
    assert!(!v.needs_paint(), "a hidden view never asks for a frame");
    v.freeze(false);
    assert!(v.needs_paint(), "thawing asks for a fresh frame");
    // Hiding released the renderer and caches; painting again must still work.
    let f = v.paint(40, 40, 1.0, 1.0, false).unwrap().rgba;
    assert_eq!(px(&f, 40, 5, 5), [255, 0, 0, 255]);
}

#[test]
fn known_gap_every_poll_marks_a_settled_page_dirty_again() {
    // Pre-existing (see docs/architecture/dom-round11-report.md): `poll()` marks the view dirty
    // in its timer task even when nothing changed, so an idle page keeps repainting at the
    // frame-rate cap. Pinned so a fix is noticed; the correct value after poll is `false`.
    let mut v = page("<div style='width:10px;height:10px;background:red'></div>");
    v.paint(40, 40, 1.0, 1.0, false).unwrap();
    assert!(!v.needs_paint());
    v.poll();
    assert!(v.needs_paint());
}
