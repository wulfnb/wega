//! Resource CSP must authorize every redirect target before the broker fetches it.
mod support;
use wega_engine::RenderingBackend;
#[test]
fn csp_rechecks_stylesheet_font_and_background_redirect_targets() {
    let foreign = support::Server::new(|r| match r.path.as_str() {
        "/font" => support::Reply::ok(
            "font/otf",
            include_bytes!("../../vendor/blitz/blitz-dom/assets/moz-bullet-font.otf").to_vec(),
        )
        .header("Access-Control-Allow-Origin", "*"),
        "/image" => support::Reply::ok(
            "image/svg+xml",
            "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'><rect width='10' height='10' fill='red'/></svg>",
        ),
        _ => support::Reply::ok("text/css", "#target{width:999px!important}"),
    });
    let font = foreign.url("localhost", "/font");
    let image = foreign.url("localhost", "/image");
    let sheet = foreign.url("localhost", "/style");
    let main = support::Server::new(move |r| {
        match r.path.as_str(){
        "/page"=>support::Reply::html("<style>@import '/style-redirect';@font-face{font-family:Test;src:url('/font-redirect')}#target{font-family:Test;width:80px;height:40px;background-image:url('/image-redirect')}</style><div id=target>Text</div>").header("Content-Security-Policy","style-src 'self' 'unsafe-inline';font-src 'self';img-src 'self'"),
        "/font-redirect"=>support::Reply::ok("font/otf","").status("302 Found").header("Location",&font),
        "/image-redirect"=>support::Reply::ok("image/svg+xml","").status("302 Found").header("Location",&image),
        _=>support::Reply::ok("text/css","").status("302 Found").header("Location",&sheet),
    }
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &main.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.paint(400, 300, 1.0, 1.0, false).unwrap();
    for _ in 0..100 {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    assert_eq!(main.requests("/style-redirect").len(), 1);
    assert_eq!(main.requests("/font-redirect").len(), 1);
    assert_eq!(main.requests("/image-redirect").len(), 1);
    assert!(
        foreign.seen.lock().unwrap().is_empty(),
        "CSP-disallowed redirects must not reach the foreign server"
    );
    assert_eq!(
        view.evaluate_json("document.getElementById('target').getBoundingClientRect().width")
            .unwrap(),
        "80"
    );
}

#[test]
fn replacing_style_text_after_cssom_edits_reparses_even_when_text_is_identical() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            "<style id=s>#target{width:80px;height:30px}</style><div id=target></div>",
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.paint(400, 300, 1.0, 1.0, false).unwrap();
    view.evaluate(
        "var s=document.getElementById('s');s.sheet.insertRule('#target{width:150px}',1)",
    )
    .unwrap();
    assert_eq!(
        view.evaluate_json("document.getElementById('target').getBoundingClientRect().width")
            .unwrap(),
        "150"
    );
    view.evaluate("s.textContent=s.textContent").unwrap();
    assert_eq!(
        view.evaluate_json("document.getElementById('target').getBoundingClientRect().width")
            .unwrap(),
        "80"
    );
}
