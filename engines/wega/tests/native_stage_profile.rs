//! Bounded stage profile. Timing observations are evidence, never flaky time gates.
use std::{cell::RefCell, rc::Rc, time::Instant};
use wega_adapter_blitz::{BlitzDocument, DocumentSetup};
use wega_dom::{DocumentView, style::StyleControl};
use wega_engine::native_engine::NativeDocument;
#[test]
fn profile_native_mutation_geometry_and_scroll() {
    let css = (0..200)
        .map(|i| format!(".r{i}{{width:{}px;height:20px;background:#eee}}", 100 + i))
        .collect::<String>();
    let html = format!(
        "<style>body{{margin:0;font:16px Arial}}{css}</style>{}",
        (0..200)
            .map(|i| format!("<div id=r{i} class=r{i}>Profile text {i}</div>"))
            .collect::<String>()
    );
    let mut native = NativeDocument::new(Rc::new(RefCell::new(BlitzDocument::from_html(
        &html,
        DocumentSetup::default(),
    ))));
    let start = Instant::now();
    native.flush_layout();
    let cold = start.elapsed().as_secs_f64() * 1000.0;
    let keys: Vec<_> = (0..200)
        .map(|i| native.doc.borrow().element_by_id(&format!("r{i}")).unwrap())
        .collect();
    let mut geometry = Vec::new();
    let mut mutation = Vec::new();
    let mut css_parse = Vec::new();
    let mut scroll = Vec::new();
    for sample in 0..10 {
        let start = Instant::now();
        let sum: f64 = keys
            .iter()
            .map(|&k| native.bounding_rect(k).unwrap().width)
            .sum();
        std::hint::black_box(sum);
        geometry.push(start.elapsed().as_secs_f64() * 1000.0);
        assert!(sum > 20000.0);
        let start = Instant::now();
        native
            .doc
            .borrow_mut()
            .set_inline_property(
                keys[0],
                "width",
                if sample % 2 == 0 { "321px" } else { "123px" },
                false,
            )
            .unwrap();
        native.ensure_layout();
        mutation.push(start.elapsed().as_secs_f64() * 1000.0);
        css_parse.push(native.timings.stylesheet_parse_ms);
        let start = Instant::now();
        native.scroll_viewport_to(wega_dom::layout::Point {
            x: 0.0,
            y: sample as f64 * 10.0,
        });
        std::hint::black_box(native.record_scene(&native.viewport.clone()));
        scroll.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let stats = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        serde_json::json!({"p50_ms":v[4],"p95_ms":v[9],"n":v.len()})
    };
    let report = serde_json::json!({"profile":if cfg!(debug_assertions){"debug"}else{"release"},"fixture":{"nodes":200,"author_rules":200,"samples":10},"cold_ms":cold,"geometry_200_queries":stats(geometry),"mutation_relayout":stats(mutation),"mutation_css_parse":stats(css_parse),"scroll_record":stats(scroll),"last_stages":{"style_box_ms":native.timings.style_and_box_tree_ms,"layout_ms":native.timings.layout_ms,"shaping_ms":native.timings.shaping_ms,"scene_ms":native.timings.scene_ms}});
    println!("{report}");
    if let Ok(dir) = std::env::var("WEGA_NATIVE_EVIDENCE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("native-stage-profile.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
}
