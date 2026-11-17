mod support;
use wega_engine::RenderingBackend;
#[test]
fn authorized_raster_images_have_intrinsic_ratio_and_real_pixels() {
    let pixels = [255u8, 0, 0, 255, 255, 0, 0, 255];
    let info = skia_safe::ImageInfo::new(
        (2, 1),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let image =
        skia_safe::images::raster_from_data(&info, skia_safe::Data::new_copy(&pixels), 8).unwrap();
    let png = image
        .encode(None, skia_safe::EncodedImageFormat::PNG, None)
        .unwrap()
        .as_bytes()
        .to_vec();
    let server = support::Server::new(move |seen| {
        if seen.path == "/page" {
            support::Reply::html(
                "<body style='margin:0'><img id=image style='display:block;width:40px' src='/image.png'></body>",
            )
        } else {
            support::Reply::ok("image/png", png.clone())
        }
    });
    let engine = support::engine();
    let mut view = engine
        .create_view(wega_engine::Url::parse(&server.url("localhost", "/page")).unwrap())
        .unwrap();
    view.set_rendering_backend(RenderingBackend::WegaNative);
    support::settle(&mut view);
    let frame = view.paint(100, 100, 1.0, 1.0, false).unwrap();
    assert_eq!(
        &frame.rgba[(10 * 100 + 10) * 4..(10 * 100 + 10) * 4 + 4],
        &[255, 0, 0, 255]
    );
    assert_eq!(view.evaluate_json("(()=>{let r=document.getElementById('image').getBoundingClientRect();return [r.width,r.height]})()").unwrap(),"[40,20]");
    assert_eq!(server.requests("/image.png").len(), 1);
}
