//! Malformed and oversized SVG inputs must fail before native expansion.
#[test]
fn svg_limits_reject_dimensions_nodes_and_input_without_poisoning_later_images() {
    use blitz_dom::node::SvgImageData;
    let parse_svg_image = |bytes: &[u8]| SvgImageData::from_data(bytes, &Default::default());
    let oversized = br#"<svg xmlns="http://www.w3.org/2000/svg" width="9000" height="20"/>"#;
    assert!(parse_svg_image(oversized).is_err());
    let nodes = format!(
        "<svg xmlns='http://www.w3.org/2000/svg'>{}</svg>",
        "<g/>".repeat(17000)
    );
    assert!(parse_svg_image(nodes.as_bytes()).is_err());
    assert!(parse_svg_image(&vec![b' '; 8 * 1024 * 1024 + 1]).is_err());
    let small = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#;
    let image = parse_svg_image(small).unwrap();
    assert_eq!(image.tree.size().width(), 20.0);
    assert_eq!(image.tree.size().height(), 10.0);
}
