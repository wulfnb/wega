//! Pinned common-behavior gates. These are small original fixtures; the separate
//! `wpt` and `iframe_wpt` suites execute the pinned unmodified upstream programs.
mod support;
#[path = "support/native_visual.rs"]
mod visual;
use base64::Engine as _;
use wega_engine::RenderingBackend;
struct Case {
    name: &'static str,
    html: String,
    script: Option<&'static str>,
    scroll: f64,
}
fn cases() -> Vec<Case> {
    let base = "<!doctype html><meta charset=utf-8><style>body{margin:0;background:white;color:black;font-family:Arial;font-size:16px;line-height:20px}*{box-sizing:border-box}p{margin:0}input,textarea,button{font-family:Arial;font-size:16px;line-height:20px}</style>";
    let rows = [
        (
            "flex",
            "<div data-probe id=f style='display:flex;width:300px;height:80px;gap:10px;align-items:stretch'><div data-probe id=a style='width:60px;background:red'></div><div data-probe id=b style='flex:1;background:blue'></div></div>",
            None,
            0.0,
        ),
        (
            "grid",
            "<div data-probe id=g style='display:grid;width:320px;height:110px;grid-template-columns:100px 1fr;grid-template-rows:40px 60px;gap:10px'><div data-probe id=a style='background:red'></div><div data-probe id=b style='background:blue'></div><div data-probe id=c style='grid-column:1 / span 2;background:green'></div></div>",
            None,
            0.0,
        ),
        (
            "table",
            "<table data-probe id=t style='width:300px;border-spacing:0;table-layout:fixed'><tr><td data-probe id=a style='width:100px;height:40px;padding:0;background:red'></td><td data-probe id=b style='width:200px;height:40px;padding:0;background:blue'></td></tr></table>",
            None,
            0.0,
        ),
        (
            "responsive-cssom",
            "<style id=s>#a{width:50px;height:40px;background:red}@media(min-width:500px){#a{width:220px}}@media(min-width:500px){@media(orientation:portrait){#a{width:999px}}}</style><div data-probe id=a></div>",
            Some(
                "document.getElementById('s').sheet.insertRule('#a{height:65px;background:blue}',2)",
            ),
            0.0,
        ),
        (
            "forms",
            "<input data-probe id=i style='display:block;width:200px;height:32px;border:1px solid black;padding:4px;background:white;color:black' value='Text'><textarea data-probe id=a style='display:block;width:200px;height:60px;border:1px solid black;padding:4px;resize:none'>Two lines</textarea>",
            Some("document.getElementById('i').value='Edited value'"),
            0.0,
        ),
        (
            "svg",
            "<svg data-probe id=s width=160 height=100 viewBox='0 0 100 100' style='display:block'><rect width=100 height=100 fill=red/><circle cx=50 cy=50 r=30 fill=blue/></svg>",
            None,
            0.0,
        ),
        (
            "canvas",
            "<canvas data-probe id=c width=140 height=70 style='display:block'></canvas>",
            Some(
                "var x=document.getElementById('c').getContext('2d');x.fillStyle='red';x.fillRect(0,0,140,70);x.save();x.translate(20,10);x.fillStyle='blue';x.fillRect(0,0,40,30);x.restore()",
            ),
            0.0,
        ),
        (
            "scrolling",
            "<div style='height:1000px;background:#eee'><div data-probe id=a style='position:absolute;top:200px;left:20px;width:100px;height:60px;background:red'></div></div><div data-probe id=b style='position:fixed;top:10px;left:250px;width:80px;height:40px;background:blue'></div>",
            None,
            180.0,
        ),
        (
            "mutations-events",
            "<div data-probe id=a style='width:80px;height:40px;background:red'></div><button id=b style='display:none'>Change</button>",
            Some(
                "document.getElementById('b').addEventListener('click',()=>{let a=document.getElementById('a');a.style.width='180px';a.style.backgroundColor='blue';let child=document.createElement('div');child.style.height='20px';child.style.backgroundColor='green';a.appendChild(child)});document.getElementById('b').click()",
            ),
            0.0,
        ),
        (
            "iframe",
            "<iframe data-probe id=f style='display:block;width:160px;height:90px;border:0' srcdoc=\"<body style='margin:0;background:red'><div style='width:60px;height:40px;background:blue'></div></body>\"></iframe>",
            None,
            0.0,
        ),
    ];
    let mut cases: Vec<_> = rows
        .into_iter()
        .map(|(name, html, script, scroll)| Case {
            name,
            html: format!("{base}{html}"),
            script,
            scroll,
        })
        .collect();
    let font = base64::engine::general_purpose::STANDARD.encode(include_bytes!(
        "../../vendor/blitz/blitz-dom/assets/moz-bullet-font.otf"
    ));
    cases.push(Case{name:"webfont",html:format!("{base}<style>@font-face{{font-family:Fixture;src:url(data:font/otf;base64,{font})}}#a{{font-family:Fixture;font-size:40px;line-height:50px;width:200px;height:60px}}</style><div data-probe id=a>• • •</div>"),script:None,scroll:0.0});
    cases
}
#[test]
fn pinned_chrome_common_behavior_has_no_regressions() {
    if std::env::var_os("WEGA_CHROME_BIN").is_none() {
        assert_ne!(
            std::env::var("WEGA_NATIVE_PINNED").as_deref(),
            Ok("1"),
            "Pinned gate requires Chrome"
        );
        return;
    }
    let dir = std::path::PathBuf::from(
        std::env::var("WEGA_NATIVE_EVIDENCE_DIR")
            .unwrap_or("/private/tmp/wega-round5-evidence".into()),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let cases = cases();
    let routes: Vec<_> = cases
        .iter()
        .map(|c| (format!("/{}", c.name), c.html.clone()))
        .collect();
    let server = support::Server::new(move |s| {
        support::Reply::html(
            routes
                .iter()
                .find(|(p, _)| p == &s.path)
                .map(|(_, h)| h.as_str())
                .unwrap_or("404"),
        )
    });
    let engine = support::engine();
    let mut reports = Vec::new();
    let mut failed = Vec::new();
    for case in cases {
        let mut view = support::open(
            &engine,
            &server.url("localhost", &format!("/{}", case.name)),
        );
        view.set_rendering_backend(RenderingBackend::WegaNative);
        if let Some(script) = case.script {
            view.evaluate(script).unwrap();
        }
        view.paint(600, 420, 1.0, 1.0, false).unwrap();
        if case.scroll != 0.0 {
            view.scroll(0.0, case.scroll);
        }
        let native = view.paint(600, 420, 1.0, 1.0, false).unwrap().rgba;
        let geometry = visual::geometry(&mut view);
        let (chrome, cg) = visual::chrome(
            &case.html,
            case.script,
            case.scroll,
            &format!("pin-{}", case.name),
            &dir,
        )
        .expect("Chrome capture is mandatory for pinned fixtures");
        let (diff, pixels) = visual::compare(
            &native,
            &chrome,
            &geometry,
            &cg,
            visual::Tolerance::default(),
        );
        visual::png(
            &dir.join(format!("pin-{}-native.png", case.name)),
            &native,
            600,
            420,
        );
        visual::png(
            &dir.join(format!("pin-{}-diff.png", case.name)),
            &pixels,
            600,
            420,
        );
        println!("{}: {:?}", case.name, diff);
        if !diff.within_tolerance {
            failed.push(case.name);
        }
        reports.push(serde_json::json!({"name":case.name,"diff":diff,"native_geometry":geometry,"chrome_geometry":cg}));
    }
    std::fs::write(
        dir.join("chrome-pins.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
    assert!(failed.is_empty(), "Pinned Chrome regression(s): {failed:?}");
}

#[test]
fn native_pinned_upstream_overflow_clip_reftest() {
    fn pixels(html: &str) -> Vec<u8> {
        let engine = wega_engine::Engine::new(
            Default::default(),
            wega_engine::NetworkPolicy {
                offline: true,
                ..Default::default()
            },
            std::sync::Arc::new(|| {}),
        )
        .unwrap();
        let mut view = engine
            .create_view(wega_engine::Url::parse("about:blank").unwrap())
            .unwrap();
        view.set_rendering_backend(RenderingBackend::WegaNative);
        view.set_html(html).unwrap();
        view.poll();
        view.paint(400, 400, 1.0, 1.0, false).unwrap().rgba
    }
    // Both files are checksum-pinned unmodified WPT sources; css_wpt verifies provenance.
    let actual = pixels(include_str!("fixtures/css-wpt/clip-001.html"));
    let reference = pixels(include_str!("fixtures/css-wpt/clip-001-ref.html"));
    assert_eq!(
        actual
            .iter()
            .zip(reference)
            .filter(|(a, b)| **a != *b)
            .count(),
        0,
        "WPT reftest must have identical pixels"
    );
}
