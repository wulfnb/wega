use image::ImageEncoder;
use wega_native::images::{RasterDecodeError, RasterImageData, SvgImageData, decode_raster};

fn png() -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&[255, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    out
}

#[test]
fn raster_decoder_preserves_real_rgba_and_the_admitted_allocation_owner() {
    let decoded = decode_raster(&png()).unwrap();
    assert_eq!((decoded.width, decoded.height), (1, 1));
    assert_eq!(&*decoded.data, &[255, 0, 0, 255]);
    let weak = std::sync::Arc::downgrade(&decoded.data);
    let image = RasterImageData::new(decoded.width, decoded.height, decoded.data);
    assert!(weak.upgrade().is_some());
    let retained = image.clone();
    drop(image);
    assert!(weak.upgrade().is_some());
    drop(retained);
    assert!(weak.upgrade().is_none());
}

#[test]
fn raster_pixel_budget_rejects_header_before_expanding_invalid_pixel_data() {
    let mut data = png();
    // Rewrite only the fixture's IHDR size and CRC: the decoder must reject
    // 9M pixels from metadata before trying to expand the one-pixel payload.
    data[16..20].copy_from_slice(&3000u32.to_be_bytes());
    data[20..24].copy_from_slice(&3000u32.to_be_bytes());
    let mut crc = !0u32;
    for &byte in &data[12..29] {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320u32 & 0u32.wrapping_sub(crc & 1));
        }
    }
    data[29..33].copy_from_slice(&(!crc).to_be_bytes());
    assert!(matches!(
        decode_raster(&data),
        Err(RasterDecodeError::Rejected(_))
    ));
    assert!(
        decode_raster(&png()).is_ok(),
        "rejection must release admission"
    );
}

#[test]
fn svg_limits_and_gzip_expansion_preserve_admission_after_rejection() {
    use std::io::Write;
    let parse = |bytes: &[u8]| SvgImageData::from_data(bytes, &Default::default());
    assert!(
        parse(br#"<svg xmlns="http://www.w3.org/2000/svg" width="9000" height="20"/>"#).is_err()
    );
    let nodes = format!(
        "<svg xmlns='http://www.w3.org/2000/svg'>{}</svg>",
        "<g/>".repeat(17000)
    );
    assert!(parse(nodes.as_bytes()).is_err());
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gzip.write_all(&vec![b' '; 8 * 1024 * 1024 + 1]).unwrap();
    assert!(parse(&gzip.finish().unwrap()).is_err());
    let svg = parse(
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10" viewBox="0 0 40 20"/>"#,
    )
    .unwrap();
    assert_eq!(svg.intrinsic_size(), (20.0, 10.0));
}

#[test]
fn svg_ignores_injected_external_resolvers_and_preserves_intrinsic_viewbox_rules() {
    let mut options = usvg::Options::default();
    options.image_href_resolver.resolve_string =
        Box::new(|_, _| panic!("external resolver invoked"));
    let svg = SvgImageData::from_data(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"><image href="file:///must-not-open.png" width="20" height="10"/><image href="https://must-not-fetch.test/image.png" width="20" height="10"/></svg>"#, &options).unwrap();
    assert_eq!(svg.intrinsic_width(), None);
    assert_eq!(svg.intrinsic_height(), None);
    assert_eq!(svg.intrinsic_size(), (300.0, 150.0));
    assert!(svg.tree.root().children().is_empty());
    let hidden = SvgImageData::from_data(
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10" viewBox="0 0 0 10"/>"#,
        &Default::default(),
    )
    .unwrap();
    assert!(hidden.intrinsic_dimensions.degenerate_view_box);
}
