//! Round 3: observable CSSOM, media, authorized imports and downloaded glyphs.
mod support;
use wega_dom::DocumentView;
use wega_engine::{RenderingBackend, View};

fn paint(view: &mut View, width: u32, height: u32, dark: bool) -> Vec<u8> {
    view.paint(width, height, 1.0, 1.0, dark).unwrap().rgba
}
fn dimensions(view: &mut View) -> String {
    view.evaluate_json("(()=>{let r=document.getElementById('target').getBoundingClientRect();return [r.width,r.height]})()").unwrap()
}
fn pixel(bytes: &[u8], width: usize, x: usize, y: usize) -> &[u8] {
    &bytes[(y * width + x) * 4..(y * width + x) * 4 + 4]
}

#[test]
fn media_resize_orientation_scheme_and_nested_conjunction_have_real_pixels() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            "<style>body{margin:0}#target{width:40px;height:20px;background:red}
         @media (min-width:300px){#target{width:90px;background:blue}
           @media (orientation:portrait){#target{height:60px}}}
         @media (prefers-color-scheme:dark){#target{background:green}}
         @media nonsense{#target{width:999px}}
         @media (orientation:invalid){#target{height:999px}}</style><div id=target></div>",
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let frame = paint(&mut view, 200, 400, false);
    assert_eq!(dimensions(&mut view), "[40,20]");
    assert_eq!(pixel(&frame, 200, 10, 10), &[255, 0, 0, 255]);
    let frame = paint(&mut view, 400, 300, false);
    assert_eq!(dimensions(&mut view), "[90,20]");
    assert_eq!(pixel(&frame, 400, 60, 10), &[0, 0, 255, 255]);
    paint(&mut view, 400, 500, false);
    assert_eq!(dimensions(&mut view), "[90,60]");
    let frame = paint(&mut view, 400, 500, true);
    assert_eq!(pixel(&frame, 400, 10, 10), &[0, 128, 0, 255]);
}

#[test]
fn cssom_insert_delete_disable_and_live_sheet_order_change_geometry_and_pixels() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            "<style id=s>body{margin:0}#target{width:40px;height:30px;background:red}</style><div id=target></div>",
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    paint(&mut view, 400, 300, false);
    assert_eq!(view.evaluate_json("var sheets=document.styleSheets;var sheet=document.getElementById('s').sheet;[sheets.length,sheets[0]===sheet,sheet instanceof CSSStyleSheet,sheet.cssRules.length]").unwrap(),"[1,true,true,2]");
    assert_eq!(
        view.evaluate_json("sheet.insertRule('#target{width:120px;background:blue}',2)")
            .unwrap(),
        "2"
    );
    // Synchronous CSSOM geometry without a paint/invalidate in between.
    assert_eq!(dimensions(&mut view), "[120,30]");
    let frame = paint(&mut view, 400, 300, false);
    assert_eq!(pixel(&frame, 400, 70, 10), &[0, 0, 255, 255]);
    view.evaluate("sheet.deleteRule(2)").unwrap();
    assert_eq!(dimensions(&mut view), "[40,30]");
    view.evaluate("sheet.disabled=true").unwrap();
    assert_ne!(dimensions(&mut view), "[40,30]");
    view.evaluate("document.getElementById('s').disabled=false")
        .unwrap();
    assert_eq!(dimensions(&mut view), "[40,30]");
    assert_eq!(view.evaluate_json("var added=document.createElement('style');added.textContent='#target{width:75px}';document.head.insertBefore(added,document.getElementById('s'));[sheets.length,sheets[0].ownerNode===added]").unwrap(),"[2,true]");
    assert_eq!(dimensions(&mut view), "[40,30]");
    assert_eq!(
        view.evaluate_json("added.remove();sheets.length").unwrap(),
        "1"
    );
    assert_eq!(view.evaluate_json("(()=>{let errors=[];for(let fn of [()=>sheet.insertRule('???',0),()=>sheet.deleteRule(99),()=>sheet.insertRule('p{}',99)]){try{fn()}catch(e){errors.push(e.name)}}return errors})()").unwrap(),"[\"SyntaxError\",\"IndexSizeError\",\"IndexSizeError\"]");
}

#[test]
fn broker_imports_resolve_relative_to_redirect_and_obey_media_and_depth() {
    let server = support::Server::new(|seen| match seen.path.as_str() {
        "/page" => support::Reply::html(
            "<body style='margin:0'><link rel=stylesheet href=/redirect><div id=target></div>",
        ),
        "/redirect" => support::Reply::ok("text/css", "")
            .status("302 Found")
            .header("Location", "/css/main.css"),
        "/css/main.css" => support::Reply::ok(
            "text/css",
            "@import 'nested.css' (min-width:300px);#target{height:30px}",
        ),
        "/css/nested.css" => support::Reply::ok(
            "text/css",
            "@import 'loop.css';#target{width:100px;background:blue}",
        ),
        "/css/loop.css" => support::Reply::ok("text/css", "@import 'loop.css';"),
        _ => support::Reply::ok("text/css", "#target{width:999px}"),
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    for _ in 0..100 {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    let frame = paint(&mut view, 400, 300, false);
    assert_eq!(dimensions(&mut view), "[100,30]");
    assert_eq!(pixel(&frame, 400, 10, 10), &[0, 0, 255, 255]);
    paint(&mut view, 200, 300, false);
    assert_ne!(dimensions(&mut view), "[100,30]");
    assert_eq!(server.requests("/css/nested.css").len(), 1);
    assert!(server.requests("/css/loop.css").len() <= 7);
    assert!(server.requests("/nested.css").is_empty());
}

#[test]
fn foreign_cssom_is_denied_and_csp_blocks_imports_and_fonts() {
    let foreign = support::Server::new(|_| support::Reply::ok("text/css", "#target{width:100px}"));
    let css = foreign.url("localhost", "/foreign.css");
    let server = support::Server::new(move |seen| {
        match seen.path.as_str() {
        "/foreign" => support::Reply::html(&format!("<link id=s rel=stylesheet href='{css}'><div id=target></div>")),
        "/blocked" => support::Reply::html("<style>@import '/blocked.css';@font-face{font-family:blocked;src:url('/blocked.ttf')}#target{font-family:blocked}</style><div id=target>Text</div>").header("Content-Security-Policy","style-src 'unsafe-inline'; font-src 'none'"),
        _ => support::Reply::ok("text/css", "#target{width:999px}"),
    }
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/foreign"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    assert_eq!(view.evaluate_json("(()=>{try{document.getElementById('s').sheet.cssRules;return 'bad'}catch(e){return e.name}})()").unwrap(),"\"SecurityError\"");
    let mut blocked = support::open(&engine, &server.url("localhost", "/blocked"));
    blocked.set_rendering_backend(RenderingBackend::WegaNative);
    paint(&mut blocked, 400, 300, false);
    assert!(server.requests("/blocked.css").is_empty());
    assert!(server.requests("/blocked.ttf").is_empty());
}

#[test]
fn downloaded_font_replaces_fallback_shaping_and_painting() {
    let bytes = include_bytes!("../../vendor/blitz/blitz-dom/assets/moz-bullet-font.otf");
    let server = support::Server::new(move |seen| match seen.path.as_str() {
        "/page" => support::Reply::html(
            "<style>body{margin:0}#target{font-family:CampaignBullet;font-size:40px}</style><div id=target>\u{2022}</div>",
        ),
        _ => support::Reply::ok("font/otf", bytes.to_vec())
            .delay(std::time::Duration::from_millis(100)),
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let before = paint(&mut view, 400, 300, false);
    view.evaluate("document.styleSheets[0].insertRule('@font-face{font-family:CampaignBullet;src:url(\"/font.otf\")}',0)").unwrap();
    for _ in 0..150 {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    let after = paint(&mut view, 400, 300, false);
    let native = view.native_document.as_ref().unwrap().borrow();
    let key = native.doc.borrow().element_by_id("target").unwrap();
    let b = native.find_box(key).unwrap();
    assert!(
        b.lines
            .iter()
            .flat_map(|l| &l.text_runs)
            .any(|r| r.font.data.as_ref() == bytes.as_slice()),
        "downloaded bytes must drive native glyphs"
    );
    assert_ne!(before, after, "downloaded font must change real pixels");
    assert_eq!(server.requests("/font.otf").len(), 1);
}
