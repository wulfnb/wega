mod support;
#[path = "support/native_visual.rs"]
mod visual;
use wega_engine::RenderingBackend;
struct Case {
    name: &'static str,
    html: String,
    script: Option<&'static str>,
    scroll: f64,
    limitations: &'static [&'static str],
}
fn corpus() -> Vec<Case> {
    let base = "<style>body{margin:0;background:white;color:black;font-family:Arial;font-size:16px;line-height:24px} p,h1{margin:0} h1{font-size:28px;line-height:36px;font-weight:700}*{box-sizing:border-box}</style>";
    let rows = [
        (
            "boxes",
            "<div data-probe id=a style='width:120px;height:70px;background:red;border:4px solid blue'></div><div data-probe id=b style='width:180px;height:50px;background:green'></div>",
            None,
            0.0,
            &[][..],
        ),
        (
            "article",
            "<article data-probe id=article style='width:520px;padding:16px'><h1>Readable native typography</h1><p>The quick brown fox jumps over the lazy dog. <b>Bold</b>, <i>italic</i>, café café, العربية שלום 你好. English paragraphs wrap across nested inline elements with measured glyph advances.</p><p>A second paragraph checks line formation and font fallback.</p></article>",
            None,
            0.0,
            &["font rasterization and UA typography differences"][..],
        ),
        (
            "navigation",
            "<nav data-probe id=nav style='display:flex;width:560px;height:64px;padding:12px;justify-content:space-between;align-items:center;background:#d0e0f0'><b>Wega</b><a href='#'>Home</a><a href='#'>Account</a></nav>",
            None,
            0.0,
            &["anonymous flex inline item blockification"][..],
        ),
        (
            "login",
            "<form data-probe id=form style='width:300px;padding:20px'><h1>Sign in</h1><label>Email</label><input style='display:block;width:260px;height:32px' value='person@example.test'><label>Password</label><input type=password style='display:block;width:260px;height:32px'><button style='display:block;width:260px;height:36px'>Sign in</button></form>",
            None,
            0.0,
            &["control typography and UA appearance remain partial"][..],
        ),
        (
            "dashboard",
            "<main data-probe id=dashboard style='display:grid;width:560px;grid-template-columns:1fr 2fr;gap:12px'><section style='padding:12px;background:#e0e8ff'><h1>42</h1><p>Visitors</p></section><section style='padding:12px;background:#ffe0e8'><h1>128</h1><p>Requests</p></section><footer style='grid-column:1 / span 2;display:flex;justify-content:space-between'><span>Healthy</span><span>Updated now</span></footer></main>",
            None,
            0.0,
            &["intrinsic grid and anonymous flex sizing partial"][..],
        ),
        (
            "scrolling",
            "<main style='height:1400px;background:#eeeeff'><div data-probe id=card style='margin-top:200px;height:80px;background:green'>Scrolled content</div></main><div data-probe id=fixed style='position:fixed;top:10px;left:400px;width:100px;height:30px;background:red'>Fixed</div>",
            None,
            180.0,
            &["root margin propagation partial"][..],
        ),
        (
            "positioned",
            "<div data-probe id=outer style='position:relative;width:400px;height:250px;padding:20px;background:#ddd'><div data-probe id=inner style='position:absolute;right:20px;bottom:30px;width:120px;height:80px;background:blue;color:white'>Positioned</div></div>",
            None,
            0.0,
            &[][..],
        ),
        (
            "table",
            "<table data-probe id=table style='width:500px;border-spacing:4px'><thead><tr><th>Item</th><th>Count</th></tr></thead><tbody><tr><td>Shaped text</td><td>12</td></tr><tr><td colspan=2>A spanning cell</td></tr></tbody></table>",
            None,
            0.0,
            &["CSS table intrinsic distribution and header styling partial"][..],
        ),
        (
            "image",
            "<div data-probe id=media style='width:320px;height:180px;background:#ddd'><img width=100 height=100 src='data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22 width=%22100%22 height=%22100%22%3E%3Crect width=%22100%22 height=%22100%22 fill=%22red%22/%3E%3C/svg%3E'><p>Media placeholder</p></div>",
            None,
            0.0,
            &["SVG image supported; inline formatting and UA defaults may differ"][..],
        ),
        (
            "dynamic",
            "<div data-probe id=target style='width:100px;height:50px;background:red'>Before</div>",
            Some(
                "let t=document.getElementById('target');t.style.width='260px';t.textContent='After JavaScript mutation';t.style.backgroundColor='green'",
            ),
            0.0,
            &[][..],
        ),
        (
            "iframe",
            "<iframe data-probe id=f style='display:block;width:250px;height:120px;border:4px solid blue' src='/child'></iframe>",
            None,
            0.0,
            &[][..],
        ),
        (
            "overlap",
            "<div data-probe id=one style='position:absolute;left:40px;top:40px;width:100px;height:100px;background:red;transform:rotate(15deg);z-index:2'></div><div data-probe id=two style='position:absolute;left:80px;top:60px;width:100px;height:100px;background:blue;z-index:1'></div>",
            None,
            0.0,
            &[][..],
        ),
    ];
    rows.into_iter()
        .map(|(name, html, script, scroll, limitations)| Case {
            name,
            html: format!("<!doctype html><meta charset=utf-8>{base}{html}"),
            script,
            scroll,
            limitations,
        })
        .collect()
}
#[test]
fn deterministic_native_blitz_visual_corpus() {
    let cases = corpus();
    let routes: Vec<_> = cases
        .iter()
        .map(|c| (format!("/{}", c.name), c.html.clone()))
        .collect();
    let server = support::Server::new(move |seen| {
        if seen.path == "/child" {
            return support::Reply::html(
                "<body style='margin:0;background:red;font:16px Arial'><p style='margin:0;width:500px;height:300px'>Actual readable child frame</p></body>",
            );
        }
        support::Reply::html(
            routes
                .iter()
                .find(|(p, _)| *p == seen.path)
                .map(|(_, h)| h.as_str())
                .unwrap_or("404"),
        )
    });
    let engine = support::engine();
    let dir = std::path::PathBuf::from(
        std::env::var("WEGA_NATIVE_EVIDENCE_DIR")
            .unwrap_or_else(|_| "/private/tmp/wega-native-evidence".into()),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let tolerance = visual::Tolerance {
        channel: std::env::var("WEGA_DIFF_CHANNEL")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8),
        mismatch_fraction: std::env::var("WEGA_DIFF_FRACTION")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.01),
        geometry_px: 0.5,
    };
    let mut reports = Vec::new();
    for case in cases {
        let mut captures = Vec::new();
        for backend in [RenderingBackend::WegaNative, RenderingBackend::Blitz] {
            let mut view = engine
                .create_view(
                    wega_engine::Url::parse(&server.url("localhost", &format!("/{}", case.name)))
                        .unwrap(),
                )
                .unwrap();
            view.set_rendering_backend(backend);
            support::settle(&mut view);
            if let Some(script) = case.script {
                view.evaluate(script).unwrap();
            }
            view.paint(600, 420, 1.0, 1.0, false).unwrap();
            if case.scroll != 0.0 {
                view.scroll(0.0, case.scroll);
            }
            let frame = view.paint(600, 420, 1.0, 1.0, false).unwrap();
            let geometry = visual::geometry(&mut view);
            let readable = frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[0] < 80 && p[1] < 80 && p[2] < 80)
                .count();
            assert_eq!(frame.rgba.len(), 600 * 420 * 4);
            let filename = format!(
                "{}-{}.png",
                case.name,
                if backend == RenderingBackend::WegaNative {
                    "native"
                } else {
                    "reference"
                }
            );
            visual::png(&dir.join(filename), &frame.rgba, 600, 420);
            captures.push((frame.rgba, geometry, readable));
        }
        let (diff, pixels) = visual::compare(
            &captures[0].0,
            &captures[1].0,
            &captures[0].1,
            &captures[1].1,
            tolerance,
        );
        visual::png(
            &dir.join(format!("{}-diff.png", case.name)),
            &pixels,
            600,
            420,
        );
        println!(
            "{}: pixels {:.3}%, geometry {:.3}px, passes {}",
            case.name,
            diff.mismatch_fraction * 100.0,
            diff.geometry_max_error_px,
            diff.within_tolerance
        );
        if case.name == "boxes" {
            assert!(diff.within_tolerance, "control boxes must match: {diff:?}");
        }
        let mature = if matches!(case.name, "boxes" | "article" | "scrolling" | "overlap") {
            visual::chrome(&case.html, case.script, case.scroll, case.name, &dir).map(
                |(pixels, geometry)| {
                    let (diff, difference) = visual::compare(
                        &captures[0].0,
                        &pixels,
                        &captures[0].1,
                        &geometry,
                        tolerance,
                    );
                    visual::png(
                        &dir.join(format!("{}-chrome-diff.png", case.name)),
                        &difference,
                        600,
                        420,
                    );
                    if std::env::var("WEGA_NATIVE_PINNED").as_deref() == Ok("1") {
                        assert!(
                            diff.within_tolerance,
                            "Pinned legacy Chrome fixture {} regressed: {diff:?}",
                            case.name
                        );
                    }
                    serde_json::json!({"geometry":geometry,"diff":diff})
                },
            )
        } else {
            None
        };
        reports.push(serde_json::json!({"page":case.name,"chrome":mature,"loads":true,"native_ink_pixels":captures[0].2,"native_geometry":captures[0].1,"reference_geometry":captures[1].1,"diff":diff,"known_limitations":case.limitations,"status":if diff.within_tolerance {"COMPLETE"} else {"PARTIAL"}}));
    }
    std::fs::write(
        dir.join("visual-corpus.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
}
