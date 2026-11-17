mod support;
#[path = "support/native_visual.rs"]
mod visual;
use wega_engine::RenderingBackend;
#[test]
fn controlled_public_native_compatibility() {
    if std::env::var("WEGA_PUBLIC_COMPAT").as_deref() != Ok("1") {
        println!("Public network probe not requested; set WEGA_PUBLIC_COMPAT=1");
        return;
    }
    let engine = support::engine();
    let dir = std::path::PathBuf::from(
        std::env::var("WEGA_NATIVE_EVIDENCE_DIR")
            .unwrap_or_else(|_| "/private/tmp/wega-native-evidence".into()),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let mut reports = Vec::new();
    for (name, url) in [
        ("example", "https://example.com/"),
        ("cern", "http://info.cern.ch/hypertext/WWW/TheProject.html"),
    ] {
        let mut view = engine
            .create_view(wega_engine::Url::parse(url).unwrap())
            .unwrap();
        view.set_rendering_backend(RenderingBackend::WegaNative);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while view.loading && std::time::Instant::now() < deadline {
            view.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if view.loading {
            view.stop();
            reports.push(serde_json::json!({"page":name,"url":url,"status":"BLOCKED","reason":"bounded navigation deadline","loads":false}));
            continue;
        }
        view.poll();
        let title = view.evaluate_json("document.title").unwrap();
        let text = view
            .evaluate_json("document.body ? document.body.textContent.slice(0,240) : ''")
            .unwrap();
        let frame = view.paint(800, 600, 1.0, 1.0, false).unwrap();
        visual::png(
            &dir.join(format!("public-{name}-native.png")),
            &frame.rgba,
            800,
            600,
        );
        let (glyphs, notdef, height, unsupported) = {
            let n = view.native_document.as_ref().unwrap().borrow();
            let mut stack = n.layout_tree.iter().collect::<Vec<_>>();
            let mut glyphs = 0;
            let mut missing = 0;
            while let Some(b) = stack.pop() {
                for line in &b.lines {
                    for run in &line.text_runs {
                        glyphs += run.glyphs.len();
                        missing += run.glyphs.iter().filter(|g| g.id == 0).count();
                    }
                }
                stack.extend(&b.children);
            }
            (
                glyphs,
                missing,
                n.layout_tree
                    .as_ref()
                    .map(|b| b.scroll_height)
                    .unwrap_or(0.0),
                n.author_sheets
                    .iter()
                    .flat_map(|s| s.unsupported.clone())
                    .collect::<Vec<_>>(),
            )
        };
        view.scroll(0.0, 100.0);
        let scroll = view.evaluate_json("window.scrollY").unwrap();
        let report = serde_json::json!({"page":name,"url":url,"final_url":view.url.as_str(),"status":"PARTIAL","loads":true,"title":title,"text_prefix":text,"positioned_glyphs":glyphs,"missing_glyphs":notdef,"readable_glyph_candidate":glyphs>30 && notdef<glyphs/10,"document_height":height,"scroll_y_after_100px":scroll,"unsupported_css":unsupported,"expected_interactions":"not exercised on public origins","layout_coherence":"requires visual inspection of saved image","no_crash_or_hang":true,"notices":view.notices.iter().collect::<Vec<_>>()});
        println!("{report}");
        reports.push(report);
    }
    std::fs::write(
        dir.join("public-compatibility.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
}
