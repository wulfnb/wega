//! Round 4 uses actual frame pixels and shared paint/pick geometry.
mod support;
#[path = "support/native_visual.rs"]
mod visual;
use wega_engine::{RenderingBackend, View};
fn frame(view: &mut View) -> Vec<u8> {
    view.paint(600, 420, 1.0, 1.0, false).unwrap().rgba
}
fn pixel(p: &[u8], x: usize, y: usize) -> &[u8] {
    &p[(y * 600 + x) * 4..(y * 600 + x) * 4 + 4]
}
const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 50 50"><defs><clipPath id="clip"><circle cx="25" cy="25" r="20"/></clipPath></defs><g clip-path="url(#clip)" transform="translate(5 0)"><path d="M0 0H50V50H0Z" fill="red"/><circle cx="20" cy="25" r="8" fill="blue" stroke="black" stroke-width="2"/></g></svg>"#;
#[test]
fn inline_svg_and_broker_svg_image_paint_shapes_paths_transforms_and_clips() {
    let server = support::Server::new(|s| {
        if s.path == "/asset.svg" {
            support::Reply::ok("image/svg+xml", SVG)
        } else {
            support::Reply::html(&format!(
                "<body style='margin:0'><div style='position:absolute;left:0;top:0'>{SVG}</div><img id=img style='position:absolute;left:120px;top:0;width:100px;height:100px' src=/asset.svg>"
            ))
        }
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let p = frame(&mut view);
    for x in [0, 120] {
        assert_eq!(pixel(&p, x + 50, 50), &[0, 0, 255, 255]);
        assert_eq!(pixel(&p, x + 30, 30), &[255, 0, 0, 255]);
        assert_eq!(pixel(&p, x + 2, 2), &[255, 255, 255, 255]);
    }
    assert_eq!(server.requests("/asset.svg").len(), 1);
    view.evaluate("document.querySelector('path').setAttribute('fill','green')")
        .unwrap();
    let p = frame(&mut view);
    assert_eq!(pixel(&p, 30, 30), &[0, 128, 0, 255]);
    assert_eq!(pixel(&p, 150, 30), &[255, 0, 0, 255]);
}
#[test]
fn canvas_surfaces_paint_and_resize_resets_pixels_and_state() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            "<body style='margin:0'><canvas id=c width=100 height=60 style='display:block'></canvas>",
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.evaluate("var c=document.getElementById('c');var ctx=c.getContext('2d');ctx.fillStyle='red';ctx.fillRect(0,0,100,60);ctx.save();ctx.translate(20,10);ctx.beginPath();ctx.rect(0,0,30,20);ctx.clip();ctx.fillStyle='blue';ctx.fillRect(0,0,100,60);ctx.restore()").unwrap();
    let p = frame(&mut view);
    assert_eq!(pixel(&p, 5, 5), &[255, 0, 0, 255]);
    assert_eq!(pixel(&p, 25, 15), &[0, 0, 255, 255]);
    assert_eq!(pixel(&p, 70, 15), &[255, 0, 0, 255]);
    view.evaluate("c.width=80").unwrap();
    let p = frame(&mut view);
    assert_eq!(pixel(&p, 5, 5), &[255, 255, 255, 255]);
    view.evaluate("ctx.fillRect(0,0,20,20)").unwrap();
    let p = frame(&mut view);
    assert_eq!(pixel(&p, 5, 5), &[0, 0, 0, 255]);
    view.evaluate("c.setAttribute('width','40')").unwrap();
    let p = frame(&mut view);
    assert_eq!(pixel(&p, 5, 5), &[255, 255, 255, 255]);
    assert_eq!(
        view.evaluate_json("c.getBoundingClientRect().width")
            .unwrap(),
        "40"
    );
}
#[test]
fn clip_path_and_compound_transform_share_pixels_and_hit_testing() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            "<body style='margin:0'><div id=under style='position:absolute;width:200px;height:180px;background:blue'></div><div id=top style='position:absolute;left:20px;top:20px;width:60px;height:60px;background:red;transform-origin:0 0;transform:translate(20px,10px) scale(2);clip-path:polygon(0% 0%,100% 0%,0% 100%)'></div>",
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let p = frame(&mut view);
    assert_eq!(pixel(&p, 50, 40), &[255, 0, 0, 255]);
    assert_eq!(pixel(&p, 150, 140), &[0, 0, 255, 255]);
    assert_eq!(
        view.evaluate_json(
            "[document.elementFromPoint(50,40).id,document.elementFromPoint(150,140).id]"
        )
        .unwrap(),
        "[\"top\",\"under\"]"
    );
    assert_eq!(view.evaluate_json("(()=>{let r=document.getElementById('top').getBoundingClientRect();return [r.x,r.y,r.width,r.height]})()").unwrap(),"[40,30,120,120]");
}
#[test]
fn background_position_size_and_shadow_spread_have_pixels_and_csp() {
    let server = support::Server::new(|s| match s.path.as_str() {
        "/asset.svg" => support::Reply::ok(
            "image/svg+xml",
            "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'><rect width='10' height='10' fill='red'/></svg>",
        ),
        "/blocked" => support::Reply::html(
            "<div style='width:100px;height:100px;background-image:url(/denied.svg)'></div>",
        )
        .header("Content-Security-Policy", "img-src 'none'"),
        _ => support::Reply::html(
            "<body style='margin:0'><div style='position:absolute;left:20px;top:20px;width:100px;height:100px;background-color:white;background-image:url(/asset.svg);background-repeat:no-repeat;background-size:20px 20px;background-position:right bottom;box-shadow:0px 0px 0px 8px blue'></div>",
        ),
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let p = loop {
        view.poll();
        let p = frame(&mut view);
        if pixel(&p, 110, 110) == [255, 0, 0, 255] || std::time::Instant::now() > deadline {
            break p;
        }
        std::thread::sleep(std::time::Duration::from_millis(3));
    };
    assert_eq!(pixel(&p, 110, 110), &[255, 0, 0, 255]);
    assert_eq!(pixel(&p, 40, 40), &[255, 255, 255, 255]);
    assert_eq!(pixel(&p, 15, 30), &[0, 0, 255, 255]);
    let mut denied = support::open(&engine, &server.url("localhost", "/blocked"));
    denied.set_rendering_backend(RenderingBackend::WegaNative);
    frame(&mut denied);
    for _ in 0..20 {
        denied.poll();
    }
    assert!(server.requests("/denied.svg").is_empty());
}
#[test]
fn svg_nested_external_resources_never_open_unbrokered_urls() {
    let server = support::Server::new(|s| {
        if s.path == "/page" {
            support::Reply::html(
                "<body><svg width='100' height='100'><image href='/forbidden.png' width='100' height='100'/><image href='file:///etc/passwd'/><script>globalThis.svgExecuted=true</script></svg></body>",
            )
        } else {
            support::Reply::html("unexpected")
        }
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    frame(&mut view);
    assert!(server.requests("/forbidden.png").is_empty());
    assert_eq!(
        view.evaluate_json("typeof svgExecuted").unwrap(),
        "\"undefined\""
    );
}
#[test]
fn tainted_canvas_can_present_but_readback_stays_denied_until_resize() {
    use wega_engine::canvas::{CanvasContext2D, CanvasId};
    let mut ctx = CanvasContext2D::new(CanvasId(1), 4, 4, false).unwrap();
    ctx.origin_clean = false;
    assert!(
        ctx.get_image_data(0, 0, 1, 1)
            .unwrap_err()
            .contains("SecurityError")
    );
    assert!(
        ctx.to_data_url("image/png")
            .unwrap_err()
            .contains("SecurityError")
    );
    ctx.resize(4, 4).unwrap();
    assert_eq!(ctx.get_image_data(0, 0, 1, 1).unwrap(), vec![0, 0, 0, 0]);
}
#[test]
fn svg_viewbox_preserves_aspect_ratio_when_css_resizes_the_viewport() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            "<body style='margin:0'><svg width='100' height='100' viewBox='0 0 50 50' style='display:block;width:200px;height:100px'><rect width='50' height='50' fill='red'/></svg>",
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let p = frame(&mut view);
    assert_eq!(pixel(&p, 25, 50), &[255, 255, 255, 255]);
    assert_eq!(pixel(&p, 100, 50), &[255, 0, 0, 255]);
    assert_eq!(pixel(&p, 175, 50), &[255, 255, 255, 255]);
}
#[test]
fn chrome_graphics_pixel_and_geometry_gate() {
    if std::env::var_os("WEGA_CHROME_BIN").is_none() {
        return;
    }
    let html = String::from(
        "<body style='margin:0;background:white'><svg data-probe id=s width=100 height=100 style='position:absolute;left:10px;top:10px' viewBox='0 0 50 50'><path d='M0 0H50V50H0Z' fill='red'/><circle cx='25' cy='25' r='15' fill='blue'/></svg><canvas data-probe id=c width=80 height=60 style='position:absolute;left:140px;top:10px'></canvas><div data-probe id=t style='position:absolute;left:260px;top:10px;width:60px;height:60px;background:green;transform-origin:0 0;transform:translate(10px,10px) scale(1.5);clip-path:circle(50% at 50% 50%)'></div>",
    );
    let script = "var ctx=document.getElementById('c').getContext('2d');ctx.fillStyle='red';ctx.fillRect(0,0,80,60);ctx.fillStyle='blue';ctx.fillRect(20,10,30,20)";
    let served = html.clone();
    let server = support::Server::new(move |_| support::Reply::html(&served));
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.evaluate(script).unwrap();
    let native = frame(&mut view);
    let geometry = visual::geometry(&mut view);
    let dir = std::path::PathBuf::from(
        std::env::var("WEGA_NATIVE_EVIDENCE_DIR")
            .unwrap_or("/private/tmp/wega-round4-evidence".into()),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let (chrome, cg) = visual::chrome(&html, Some(script), 0.0, "graphics", &dir).unwrap();
    let (diff, pixels) = visual::compare(
        &native,
        &chrome,
        &geometry,
        &cg,
        visual::Tolerance::default(),
    );
    visual::png(&dir.join("graphics-native.png"), &native, 600, 420);
    visual::png(&dir.join("graphics-diff.png"), &pixels, 600, 420);
    std::fs::write(
        dir.join("graphics.json"),
        serde_json::to_vec_pretty(&diff).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("graphics-geometry.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"native":geometry,"chrome":cg})).unwrap(),
    )
    .unwrap();
    assert!(diff.within_tolerance, "{diff:?}");
}
