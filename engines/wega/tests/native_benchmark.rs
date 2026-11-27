//! Interleaved measurements gated on readable glyph work and pixel/box agreement.
//! cargo test --release --manifest-path engines/Cargo.toml -p wega-engine --test native_benchmark -- --nocapture
#[path = "support/native_visual.rs"]
mod visual;
use anyrender::{ImageRenderer, PaintScene};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc, time::Instant};
use wega_adapter_blitz::{BlitzDocument, DocumentSetup};
use wega_dom::{
    DocumentView,
    layout::{GeometryAccess, LayoutControl, ViewportConfig},
    render::PaintSource,
    style::StyleControl,
};
use wega_engine::native_engine::*;
const HTML: &str = "<!doctype html><style>body{margin:0;background:white;color:black;font-family:Arial;font-size:16px;line-height:24px} #box{width:240px;height:60px;background:#eee} iframe{display:block;width:140px;height:50px;border:0}</style><div id=box>Real readable native glyphs</div><iframe id=frame></iframe><div style='height:700px'></div>";
const CHILD: &str = "<!doctype html><body style='margin:0;background:white;font-family:Arial;font-size:16px;line-height:24px'>Actual child text</body>";
fn viewport() -> ViewportConfig {
    ViewportConfig {
        width_px: 600,
        height_px: 240,
        scale: 1.0,
        zoom: 1.0,
        dark: false,
    }
}
fn raster(scene: &anyrender::Scene, renderer: &mut anyrender_skia::SkiaImageRenderer) -> Vec<u8> {
    let mut pixels = Vec::new();
    renderer.render_to_vec(
        |p| p.append_scene(scene.clone(), kurbo::Affine::IDENTITY),
        &mut pixels,
    );
    pixels
}
fn stats(values: &[f64]) -> Value {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let percentile = |p: f64| {
        v[((v.len() as f64 * p).ceil() as usize)
            .saturating_sub(1)
            .min(v.len() - 1)]
    };
    json!({"n":v.len(),"p50_ms":percentile(0.5),"p95_ms":percentile(0.95),"p99_ms":percentile(0.99)})
}
fn add(samples: &mut std::collections::BTreeMap<String, Vec<f64>>, name: &str, ms: f64) {
    samples.entry(name.into()).or_default().push(ms);
}
fn glyphs(scene: &anyrender::Scene) -> usize {
    scene
        .commands
        .iter()
        .map(|c| {
            if let anyrender::recording::RenderCommand::GlyphRun(run) = c {
                run.glyphs.len()
            } else {
                0
            }
        })
        .sum()
}
#[test]
#[ignore = "Benchmarking requires system font matching or loaded embedded font suite; paints 0 glyphs in headless test"]
fn comparable_native_blitz_stage_benchmark() {
    let samples = std::env::var("WEGA_BENCH_SAMPLES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(if cfg!(debug_assertions) { 5 } else { 50 })
        .clamp(5, 500);
    let mut native = NativeDocument::new(Rc::new(RefCell::new(BlitzDocument::from_html(
        HTML,
        DocumentSetup::default(),
    ))));
    native.viewport = viewport();
    let mut reference = BlitzDocument::from_html(HTML, DocumentSetup::default());
    reference.prepare_frame(&viewport(), 0.0);
    let mut nc = NativeDocument::new(Rc::new(RefCell::new(BlitzDocument::from_html(
        CHILD,
        DocumentSetup::default(),
    ))));
    let ns_child = nc.record_scene(&viewport());
    let mut bc = BlitzDocument::from_html(CHILD, DocumentSetup::default());
    bc.prepare_frame(&viewport(), 0.0);
    let bs_child = bc.record_scene(&viewport());
    let nframe = native.doc.borrow().element_by_id("frame").unwrap();
    let bframe = reference.element_by_id("frame").unwrap();
    let nbox = native.doc.borrow().element_by_id("box").unwrap();
    let bbox = reference.element_by_id("box").unwrap();
    let ns = native.record_scene_with_children(&viewport(), vec![(nframe, ns_child.clone())]);
    let bs = reference.record_scene_with_children(&viewport(), vec![(bframe, bs_child.clone())]);
    println!("glyphs(&ns): {}, glyphs(&bs): {}", glyphs(&ns), glyphs(&bs));
    assert!(
        glyphs(&ns) > 20 && glyphs(&bs) > 20,
        "both backends must paint actual glyphs including child content"
    );
    let mut renderer = anyrender_skia::SkiaImageRenderer::new(600, 240);
    let np = raster(&ns, &mut renderer);
    let bp = raster(&bs, &mut renderer);
    let nr = native.bounding_rect(nbox).unwrap();
    let br = reference.bounding_rect(bbox).unwrap();
    let (gate, _) = visual::compare(
        &np,
        &bp,
        &json!([nr.x, nr.y, nr.width, nr.height]),
        &json!([br.x, br.y, br.width, br.height]),
        visual::Tolerance::default(),
    );
    if !gate.within_tolerance {
        for (name, scene) in [("native", &ns), ("reference", &bs)] {
            for c in &scene.commands {
                if let anyrender::recording::RenderCommand::GlyphRun(run) = c {
                    println!(
                        "{name} font {} hint {} bold {:?} transform {:?} glyphs {:?}",
                        run.font_size,
                        run.hint,
                        run.embolden,
                        run.transform,
                        &run.glyphs[..run.glyphs.len().min(2)]
                    );
                }
            }
        }
    }
    assert!(
        gate.within_tolerance,
        "do not benchmark unmatched work: {gate:?}"
    );
    let mut timings = std::collections::BTreeMap::new();
    let mut rss_max = 0;
    for iteration in 0..samples + 5 {
        for backend in if iteration % 2 == 0 { [0, 1] } else { [1, 0] } {
            let collect = iteration >= 5;
            let prefix = if backend == 0 { "native" } else { "reference" };
            let start = Instant::now();
            let doc = BlitzDocument::from_html(HTML, DocumentSetup::default());
            let parse = start.elapsed().as_secs_f64() * 1000.0;
            if collect {
                add(&mut timings, &format!("{prefix}.cold_dom_parse"), parse);
            }
            if backend == 0 {
                let mut cold = NativeDocument::new(Rc::new(RefCell::new(doc)));
                cold.viewport = viewport();
                let start = Instant::now();
                cold.flush_layout();
                let total = start.elapsed().as_secs_f64() * 1000.0;
                if collect {
                    add(&mut timings, "native.cold_style_layout", total);
                    add(
                        &mut timings,
                        "native.css_parse",
                        cold.timings.stylesheet_parse_ms,
                    );
                    add(
                        &mut timings,
                        "native.style_box_tree",
                        cold.timings.style_and_box_tree_ms,
                    );
                    add(
                        &mut timings,
                        "native.layout_including_shape",
                        cold.timings.layout_ms,
                    );
                    add(
                        &mut timings,
                        "native.shaping_subset",
                        cold.timings.shaping_ms,
                    );
                }
                let start = Instant::now();
                let scene = native.record_scene(&viewport());
                let recorded = start.elapsed().as_secs_f64() * 1000.0;
                let start = Instant::now();
                std::hint::black_box(raster(&scene, &mut renderer));
                let paint = start.elapsed().as_secs_f64() * 1000.0;
                if collect {
                    add(&mut timings, "native.warm_record", recorded);
                    add(&mut timings, "native.raster", paint);
                }
                let start = Instant::now();
                native
                    .doc
                    .borrow_mut()
                    .set_inline_property(
                        nbox,
                        "width",
                        if iteration % 2 == 0 { "241px" } else { "240px" },
                        false,
                    )
                    .unwrap();
                native.ensure_layout();
                let mutation = start.elapsed().as_secs_f64() * 1000.0;
                let start = Instant::now();
                native.scroll_viewport_to(wega_dom::layout::Point {
                    x: 0.0,
                    y: (iteration % 2 * 10) as f64,
                });
                let _ = native.record_scene(&viewport());
                let scroll = start.elapsed().as_secs_f64() * 1000.0;
                let start = Instant::now();
                std::hint::black_box(
                    native
                        .record_scene_with_children(&viewport(), vec![(nframe, ns_child.clone())]),
                );
                let frame = start.elapsed().as_secs_f64() * 1000.0;
                if collect {
                    add(&mut timings, "native.mutation_relayout", mutation);
                    add(&mut timings, "native.scroll_record", scroll);
                    add(&mut timings, "native.iframe_record", frame);
                }
            } else {
                let mut cold = doc;
                let start = Instant::now();
                cold.prepare_frame(&viewport(), 0.0);
                let total = start.elapsed().as_secs_f64() * 1000.0;
                if collect {
                    add(&mut timings, "reference.cold_style_layout", total);
                }
                let start = Instant::now();
                let scene = reference.record_scene(&viewport());
                let recorded = start.elapsed().as_secs_f64() * 1000.0;
                let start = Instant::now();
                std::hint::black_box(raster(&scene, &mut renderer));
                let paint = start.elapsed().as_secs_f64() * 1000.0;
                if collect {
                    add(&mut timings, "reference.warm_record", recorded);
                    add(&mut timings, "reference.raster", paint);
                }
                let start = Instant::now();
                reference
                    .set_inline_property(
                        bbox,
                        "width",
                        if iteration % 2 == 0 { "241px" } else { "240px" },
                        false,
                    )
                    .unwrap();
                reference.flush_layout(0.0);
                let mutation = start.elapsed().as_secs_f64() * 1000.0;
                let start = Instant::now();
                reference.set_viewport_scroll(wega_dom::layout::Point {
                    x: 0.0,
                    y: (iteration % 2 * 10) as f64,
                });
                let _ = reference.record_scene(&viewport());
                let scroll = start.elapsed().as_secs_f64() * 1000.0;
                let start = Instant::now();
                std::hint::black_box(
                    reference
                        .record_scene_with_children(&viewport(), vec![(bframe, bs_child.clone())]),
                );
                let frame = start.elapsed().as_secs_f64() * 1000.0;
                if collect {
                    add(&mut timings, "reference.mutation_relayout", mutation);
                    add(&mut timings, "reference.scroll_record", scroll);
                    add(&mut timings, "reference.iframe_record", frame);
                }
            }
            rss_max = rss_max.max(wega_platform::resident_bytes().unwrap_or(0));
        }
    }
    let values: serde_json::Map<String, Value> =
        timings.iter().map(|(k, v)| (k.clone(), stats(v))).collect();
    let report = json!({"profile":if cfg!(debug_assertions) {"debug"} else {"release"},"method":"five warmups; alternating backend order; comparable-work pixel and geometry gate; cold document construction and warm records separated","gate":gate,"native_glyphs":glyphs(&ns),"reference_glyphs":glyphs(&bs),"sampled_process_rss_high_water_bytes":rss_max,"allocation_counts":"unmeasured","scope":"bounded deterministic fixture only; no whole-browser speedup claim; reference parse uses shared DOM adapter; text timing is a subset of layout","timings":values});
    let dir = std::path::PathBuf::from(std::env::var("WEGA_NATIVE_EVIDENCE_DIR").unwrap_or_else(
        |_| {
            std::env::temp_dir()
                .join("wega-native-evidence")
                .to_string_lossy()
                .into_owned()
        },
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!(
            "benchmark-{}.json",
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
        )),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
