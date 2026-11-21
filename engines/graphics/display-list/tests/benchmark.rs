//! Release observations on the independent owner/pipeline, without speed claims.
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, time::Instant};
use wega_css_parser::StyleSheet;
use wega_display_list::DisplayList;
use wega_dom::{Document, DocumentMutation, DocumentView, Mutator};
use wega_html::{HtmlDiagnostic, ParseReport, Parser, Step};
use wega_layout::{LayoutContext, Point, build_box_tree, perform_layout};
struct Quiet;
impl ParseReport for Quiet {
    fn warning(&self, _: HtmlDiagnostic<'_>) {}
}
fn parse(html: &str) -> Document {
    let d = Rc::new(RefCell::new(Document::new(
        url::Url::parse("https://wega.test/").unwrap(),
    )));
    let mut p = Parser::new(d.clone(), html, 30_000, Quiet);
    assert!(matches!(p.step(), Step::Finished));
    drop(p);
    Rc::try_unwrap(d).ok().unwrap().into_inner()
}
fn timed<T>(
    samples: &mut BTreeMap<&str, Vec<f64>>,
    name: &'static str,
    f: impl FnOnce() -> T,
) -> T {
    let start = Instant::now();
    let result = f();
    samples
        .entry(name)
        .or_default()
        .push(start.elapsed().as_secs_f64() * 1000.0);
    result
}
#[test]
fn independent_owner_and_native_stages_release_observations() {
    use anyrender::{ImageRenderer, PaintScene};
    let count = if cfg!(debug_assertions) { 5 } else { 50 };
    let css = format!(
        "body{{margin:0;font:16px/20px Arial}}{}",
        (0..200)
            .map(|i| format!(".r{i}{{width:300px;height:20px;background:red}} "))
            .collect::<String>()
    );
    let html = format!(
        "<!doctype html><body>{}",
        (0..200)
            .map(|i| format!("<div id=r{i} class=r{i}>Native owner text {i}</div>"))
            .collect::<String>()
    );
    let ctx = LayoutContext {
        viewport_width: 600.0,
        viewport_height: 420.0,
        font_size_root: 16.0,
    };
    let mut samples = BTreeMap::new();
    let mut actual_nodes = 0;
    let mut actual_rules = 0;
    let mut renderer = anyrender_skia::SkiaImageRenderer::new(600, 420);
    for sample in 0..count + 5 {
        let mut row = BTreeMap::new();
        let mut d = timed(&mut row, "html_dom", || parse(&html));
        let sheet = timed(&mut row, "css_parse", || StyleSheet::parse(&css));
        let mut tree = timed(&mut row, "style_box_tree", || {
            build_box_tree(&d, d.root(), None, &[&sheet], None).unwrap()
        });
        let shape_start = wega_layout::text::shaping_nanos();
        timed(&mut row, "layout_including_shaping", || {
            perform_layout(&mut tree, &ctx)
        });
        row.insert(
            "shaping_subset",
            vec![(wega_layout::text::shaping_nanos() - shape_start) as f64 / 1e6],
        );
        let target = d.element_by_id("r0").unwrap();
        actual_nodes = d.node_count();
        actual_rules = sheet.rules.len();
        let name = html5ever::QualName::new(None, html5ever::ns!(), "style".into());
        timed(&mut row, "mutation_restyle_relayout", || {
            d.mutate()
                .set_attribute(target, name, "width:320px")
                .unwrap();
            tree = build_box_tree(&d, d.root(), None, &[&sheet], None).unwrap();
            perform_layout(&mut tree, &ctx);
        });
        let list = timed(&mut row, "display_list", || {
            DisplayList::build(&tree, &[], 1.0)
        });
        assert_eq!(list.node_rects(target)[0].width, 320.0);
        let scene = timed(&mut row, "scene_record", || list.to_anyrender_scene());
        let glyphs = scene
            .commands
            .iter()
            .map(|c| {
                if let anyrender::recording::RenderCommand::GlyphRun(g) = c {
                    g.glyphs.len()
                } else {
                    0
                }
            })
            .sum::<usize>();
        assert!(glyphs > 1000, "real text must be shaped and recorded");
        let mut pixels = Vec::new();
        timed(&mut row, "raster", || {
            renderer.render_to_vec(
                |p| p.append_scene(scene.clone(), kurbo::Affine::IDENTITY),
                &mut pixels,
            )
        });
        assert_eq!(
            &pixels[(18 * 600 + 310) * 4..(18 * 600 + 310) * 4 + 4],
            &[255, 0, 0, 255]
        );
        timed(&mut row, "scroll_record", || {
            std::hint::black_box(
                DisplayList::build_scrolled(&tree, &[], 1.0, Point { x: 0.0, y: 100.0 })
                    .to_anyrender_scene(),
            )
        });
        if sample >= 5 {
            for (name, values) in row {
                samples.entry(name).or_insert_with(Vec::new).extend(values);
            }
        }
    }
    let stats: BTreeMap<_, _> = samples
        .into_iter()
        .map(|(name, mut v)| {
            v.sort_by(f64::total_cmp);
            let p = |q: f64| v[((v.len() as f64 * q).ceil() as usize).saturating_sub(1)];
            (name, serde_json_value(v.len(), p(0.5), p(0.95), p(0.99)))
        })
        .collect();
    let report = format!(
        "{{\"scope\":\"independent owner and algorithm fixture; no V8 or resource loading\",\"elements\":200,\"nodes\":{actual_nodes},\"rules\":{actual_rules},\"warmups\":5,\"stages\":{{{}}}}}\n",
        stats
            .into_iter()
            .map(|(n, v)| format!("\"{n}\":{v}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let dir = std::path::PathBuf::from(std::env::var("WEGA_NATIVE_EVIDENCE_DIR").unwrap_or_else(
        |_| {
            std::env::temp_dir()
                .join("wega-native-independent-benchmark")
                .to_string_lossy()
                .into_owned()
        },
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("independent-benchmark.json"), &report).unwrap();
    println!("{report}");
}
fn serde_json_value(n: usize, p50: f64, p95: f64, p99: f64) -> String {
    format!("{{\"n\":{n},\"p50_ms\":{p50},\"p95_ms\":{p95},\"p99_ms\":{p99}}}")
}
