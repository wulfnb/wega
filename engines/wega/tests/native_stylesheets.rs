mod support;
use wega_dom::DocumentView;
use wega_engine::RenderingBackend;
fn width(view: &mut wega_engine::View) -> f64 {
    view.paint(400, 300, 1.0, 1.0, false).unwrap();
    let native = view.native_document.as_ref().unwrap().borrow();
    let key = native.doc.borrow().element_by_id("target").unwrap();
    native.bounding_rect(key).unwrap().width
}
#[test]
fn authorized_external_css_order_redirect_mime_and_dynamic_link() {
    let server = support::Server::new(|seen| match seen.path.as_str() {
        "/page" => support::Reply::html(
            "<link rel=stylesheet href='/redirect'><style>#target {width:140px}</style><div id=target></div>",
        ),
        "/redirect" => support::Reply::ok("text/css", "")
            .status("302 Found")
            .header("Location", "/first.css"),
        "/first.css" => support::Reply::ok("text/css", "#target {width:100px}"),
        "/second" => support::Reply::ok("text/css", "#target {width:230px}"),
        "/bad.css" => support::Reply::ok("text/html", "#target {width:999px}"),
        _ => support::Reply::html("404"),
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    assert_eq!(width(&mut view), 140.0);
    view.evaluate("var link=document.createElement('link');link.rel='stylesheet';link.href='/second';document.head.appendChild(link)").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while width(&mut view) != 230.0 && std::time::Instant::now() < deadline {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(width(&mut view), 230.0);
    view.evaluate("link.remove()").unwrap();
    assert_eq!(width(&mut view), 140.0);
    view.evaluate("link.href='/bad.css';document.head.appendChild(link)")
        .unwrap();
    support::settle(&mut view);
    for _ in 0..20 {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(width(&mut view), 140.0);
    assert!(
        server
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.path == "/first.css")
    );
}
#[test]
fn csp_denies_link_stylesheet() {
    let server = support::Server::new(|seen| match seen.path.as_str() {
        "/page" => support::Reply::html(
            "<link rel=stylesheet href='/blocked.css'><div id=target style='width:80px'></div>",
        )
        .header("Content-Security-Policy", "style-src 'none'"),
        _ => support::Reply::ok("text/css", "#target {width:999px !important}"),
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    // Inline style behavior is tested separately; here verify no external rule
    // can enter the native sheet set or reach the server.
    let _ = width(&mut view);
    assert!(
        !server
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.path == "/blocked.css")
    );
    assert!(
        view.native_document
            .as_ref()
            .unwrap()
            .borrow()
            .author_sheets
            .iter()
            .all(|s| s.rules.is_empty())
    );
}

#[test]
fn native_style_element_csp_nonce_and_media_are_enforced() {
    let server = support::Server::new(|_| {
        support::Reply::html("<style>#target{height:999px}</style><style nonce=allowed>#target{height:30px}</style><style nonce=allowed media=print>#target{height:666px}</style><div id=target></div>").header("Content-Security-Policy","style-src 'nonce-allowed'")
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.paint(400, 300, 1.0, 1.0, false).unwrap();
    assert_eq!(
        view.evaluate_json("document.getElementById('target').getBoundingClientRect().height")
            .unwrap(),
        "30"
    );
}
