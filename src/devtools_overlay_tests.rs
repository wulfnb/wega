use super::*;
use wega::engine_protocol::{
    DocumentContext, DocumentId, DocumentRevision,
    inspector::{
        dom::InspectorNodeId,
        geometry::{BoxSet, Edges, ElementLabel, ViewportReport},
    },
};

fn doc() -> DocumentContext {
    DocumentContext {
        document: DocumentId(1),
        revision: DocumentRevision(1),
    }
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> GeomRect {
    GeomRect {
        x,
        y,
        width: w,
        height: h,
    }
}
fn all(v: f64) -> Edges {
    Edges {
        top: v,
        right: v,
        bottom: v,
        left: v,
    }
}
fn report() -> GeometryReport {
    GeometryReport {
        document: doc(),
        epoch: 4,
        node: InspectorNodeId {
            document: doc(),
            key: 2,
        },
        state: GeometryState::Current,
        // The fixture of the D3-C.0 measurement: margin 10, border 2, padding 5, content 20x10.
        boxes: Some(BoxSet {
            border_box: rect(50.0, 40.0, 38.0, 28.0),
            border: all(2.0),
            padding: all(5.0),
            margin: all(10.0),
        }),
        viewport: ViewportReport {
            width: 800.0,
            height: 600.0,
            device_pixel_ratio: 2.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
        },
        paint: PaintMapping::Exact { clip: None },
        label: ElementLabel {
            tag: "div".into(),
            id: "box".into(),
            classes: "a b c d".into(),
        },
    }
}
fn area(rects: impl Iterator<Item = GeomRect>) -> f64 {
    rects.map(|r| r.width * r.height).sum()
}

#[test]
fn bands_are_exclusive_and_tile_the_margin_box_exactly() {
    let Plan::Draw {
        bands,
        outline,
        label,
        stale,
    } = plan(&report(), Some(4))
    else {
        panic!()
    };
    assert!(!stale);
    assert_eq!(outline, rect(50.0, 40.0, 38.0, 28.0));
    assert_eq!(label, "div#box.a.b.c  38 × 28", "three classes at most");
    let total = area(bands.iter().map(|(_, r)| *r));
    assert_eq!(total, 58.0 * 48.0, "no overlap and no gap: the margin box");
    let content: Vec<_> = bands.iter().filter(|(l, _)| *l == Layer::Content).collect();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0].1, rect(57.0, 47.0, 24.0, 14.0));
    for layer in [Layer::Margin, Layer::Border, Layer::Padding] {
        assert!(bands.iter().any(|(l, _)| *l == layer), "{layer:?}");
    }
}

#[test]
fn transforms_and_unmodelled_clipping_draw_nothing_and_say_why() {
    for (reason, word) in [
        (UnsupportedPaint::Transform, "transformed"),
        (UnsupportedPaint::ClipPositioned, "positioned"),
        (UnsupportedPaint::Unknown, "not known"),
    ] {
        let mut r = report();
        r.paint = PaintMapping::Unsupported { reason };
        let Plan::Suppressed(message) = plan(&r, Some(4)) else {
            panic!("{reason:?} must not draw")
        };
        assert!(message.contains(word), "{message}");
    }
}

#[test]
fn unavailable_geometry_never_draws() {
    for (state, word) in [
        (GeometryState::Pending, "not run"),
        (GeometryState::NoBoxes, "no boxes"),
        (GeometryState::Refused, "nested too deeply"),
        (GeometryState::Invalid, "no longer exists"),
    ] {
        let mut r = report();
        r.state = state;
        r.boxes = None;
        let Plan::Suppressed(message) = plan(&r, Some(4)) else {
            panic!("{state:?}")
        };
        assert!(message.contains(word), "{message}");
    }
}

#[test]
fn clips_and_the_viewport_cut_the_overlay_and_a_fully_hidden_element_is_not_drawn() {
    // Clipped to the left half of the element's border box.
    let mut r = report();
    r.paint = PaintMapping::Exact {
        clip: Some(rect(0.0, 0.0, 60.0, 600.0)),
    };
    let Plan::Draw { bands, outline, .. } = plan(&r, Some(4)) else {
        panic!()
    };
    assert_eq!(outline, rect(50.0, 40.0, 10.0, 28.0));
    assert!(bands.iter().all(|(_, b)| b.x + b.width <= 60.0 + 1e-9));
    // Scrolled partly above the viewport: cut at 0.
    let mut r = report();
    r.boxes.as_mut().unwrap().border_box.y = -10.0;
    let Plan::Draw { outline, .. } = plan(&r, None) else {
        panic!()
    };
    assert_eq!(outline.y, 0.0);
    assert_eq!(outline.height, 18.0);
    // Entirely clipped away, or entirely off screen.
    let mut r = report();
    r.paint = PaintMapping::Exact {
        clip: Some(rect(300.0, 300.0, 10.0, 10.0)),
    };
    assert!(matches!(plan(&r, None), Plan::Suppressed(_)));
    let mut r = report();
    r.boxes.as_mut().unwrap().border_box.x = 5000.0;
    assert!(matches!(plan(&r, None), Plan::Suppressed(_)));
}

#[test]
fn stale_reports_and_epoch_changes_are_drawn_marked_never_as_current() {
    let mut r = report();
    r.state = GeometryState::Stale;
    assert!(matches!(plan(&r, Some(4)), Plan::Draw { stale: true, .. }));
    let r = report();
    assert!(
        matches!(plan(&r, Some(5)), Plan::Draw { stale: true, .. }),
        "the document moved on"
    );
    assert!(matches!(plan(&r, Some(4)), Plan::Draw { stale: false, .. }));
}

#[test]
fn css_pixels_map_to_screen_points_with_browser_zoom_and_no_device_pixel_factor() {
    let page = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(800.0, 600.0));
    let s = to_screen(page, 1.0, rect(10.0, 20.0, 30.0, 40.0));
    assert_eq!(
        s,
        Rect::from_min_size(Pos2::new(110.0, 70.0), Vec2::new(30.0, 40.0))
    );
    let s = to_screen(page, 1.5, rect(10.0, 20.0, 30.0, 40.0));
    assert_eq!(
        s,
        Rect::from_min_size(Pos2::new(115.0, 80.0), Vec2::new(45.0, 60.0))
    );
    // A zero-size element yields no bands rather than a degenerate fill.
    let mut r = report();
    r.boxes = Some(BoxSet {
        border_box: rect(5.0, 5.0, 0.0, 0.0),
        border: Edges::default(),
        padding: Edges::default(),
        margin: Edges::default(),
    });
    assert!(matches!(plan(&r, None), Plan::Suppressed(_)));
}

#[test]
#[ignore = "BENCH: run optimized"]
fn plan_and_mapping_cost_per_frame() {
    let r = report();
    let page = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(800.0, 600.0));
    let mut samples = Vec::new();
    for _ in 0..200_000 {
        let t = std::time::Instant::now();
        let p = plan(&r, Some(4));
        if let Plan::Draw { bands, .. } = &p {
            for (_, b) in bands {
                std::hint::black_box(to_screen(page, 1.25, *b));
            }
        }
        std::hint::black_box(&p);
        samples.push(t.elapsed().as_secs_f64() * 1e9);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "BENCH overlay plan+map per element: p50={:.0} ns p95={:.0} ns max={:.0} ns",
        samples[samples.len() / 2],
        samples[(samples.len() as f64 * 0.95) as usize],
        samples[samples.len() - 1]
    );
}
