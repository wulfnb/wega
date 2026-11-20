//! End-to-end algorithm slice with one Wega owner and no reference document.
use std::{cell::RefCell, rc::Rc};
use wega_dom::{Document, DocumentMutation, DocumentView, Mutator, NodeView};
use wega_html::{HtmlDiagnostic, ParseReport, Parser, Step};
use wega_native::{
    css::StyleSheet,
    display_list::DisplayList,
    layout::{LayoutContext, Point, build_box_tree, perform_layout},
};
struct Quiet;
impl ParseReport for Quiet {
    fn warning(&self, _: HtmlDiagnostic<'_>) {}
}
fn document(html: &str) -> Rc<RefCell<Document>> {
    let d = Rc::new(RefCell::new(Document::new(
        url::Url::parse("https://wega.test/").unwrap(),
    )));
    let mut p = Parser::new(d.clone(), html, 30_000, Quiet);
    match p.step() {
        Step::Finished => (),
        Step::Script(_) => panic!("fixture has no scripts"),
        Step::Overflow => panic!("fixture overflow"),
    }
    d
}
#[test]
fn owned_parser_mutation_layout_picking_scene_and_raster_share_one_tree() {
    let d = document(
        "<!doctype html><body style='margin:0'><div id=box style='width:100px;height:40px;background:red'>Native</div>",
    );
    let target = d.borrow().element_by_id("box").unwrap();
    let render = || {
        let d = d.borrow();
        let mut tree = build_box_tree(&*d, d.root(), None, &[], None).unwrap();
        perform_layout(
            &mut tree,
            &LayoutContext {
                viewport_width: 600.0,
                viewport_height: 420.0,
                font_size_root: 16.0,
            },
        );
        DisplayList::build(&tree, &[], 1.0)
    };
    let first = render();
    assert_eq!(first.node_rects(target)[0].width, 100.0);
    assert_eq!(first.hit_test(Point { x: 90.0, y: 30.0 }), Some(target));
    let style_name = d
        .borrow()
        .node(target)
        .unwrap()
        .attributes()
        .find(|(n, _)| &*n.local == "style")
        .unwrap()
        .0
        .clone();
    d.borrow_mut()
        .mutate()
        .set_attribute(
            target,
            style_name,
            "width:200px;height:40px;background:blue",
        )
        .unwrap();
    let second = render();
    assert_eq!(second.node_rects(target)[0].width, 200.0);
    assert_eq!(second.hit_test(Point { x: 190.0, y: 30.0 }), Some(target));
    use anyrender::{ImageRenderer, PaintScene};
    let scene = second.to_anyrender_scene();
    let mut pixels = Vec::new();
    anyrender_skia::SkiaImageRenderer::new(600, 420).render_to_vec(
        |p| p.append_scene(scene.clone(), kurbo::Affine::IDENTITY),
        &mut pixels,
    );
    assert_eq!(
        &pixels[(30 * 600 + 190) * 4..(30 * 600 + 190) * 4 + 4],
        &[0, 0, 255, 255]
    );
    d.borrow_mut().mutate().remove_and_drop(target).unwrap();
    assert!(d.borrow().node(target).is_none());
    assert!(render().node_rects(target).is_empty());
}
#[test]
fn native_depth_limit_is_enforced_with_the_owned_tree() {
    for (depth, accepted) in [(120, true), (130, false)] {
        let html = format!(
            "<!doctype html><body>{}text{}",
            "<div>".repeat(depth),
            "</div>".repeat(depth)
        );
        let d = document(&html);
        let d = d.borrow();
        assert_eq!(
            build_box_tree(&*d, d.root(), None, &[&StyleSheet::default()], None).is_some(),
            accepted
        );
    }
}
#[test]
fn dependency_graph_excludes_reference_runtime_for_normal_and_test_builds() {
    let out = std::process::Command::new("cargo")
        .args([
            "tree",
            "--locked",
            "--offline",
            "-p",
            "wega-native",
            "-e",
            "normal,build,dev",
            "--prefix",
            "none",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let tree = String::from_utf8(out.stdout).unwrap();
    for line in tree.lines() {
        let name = line.split_whitespace().next().unwrap_or("");
        assert!(
            !name.starts_with("blitz-") && name != "wega-adapter-blitz",
            "reference dependency: {line}"
        );
    }
}
