//! Reduced public-page regressions. Geometry is pinned against real Chrome;
//! empty colored blocks avoid differences in platform font rasterization.
mod support;
#[path = "support/native_visual.rs"]
mod visual;
use wega_engine::RenderingBackend;

fn compare(name: &str, body: &str) {
    let html = format!(
        "<!doctype html><style>body{{margin:0;font:16px/20px Arial}}*{{box-sizing:border-box}}</style>{body}"
    );
    let served = html.clone();
    let server = support::Server::new(move |_| support::Reply::html(&served));
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let frame = view.paint(600, 420, 1.0, 1.0, false).unwrap();
    let geometry = visual::geometry(&mut view);
    let dir = std::path::PathBuf::from(
        std::env::var("WEGA_NATIVE_EVIDENCE_DIR")
            .unwrap_or_else(|_| "/private/tmp/wega-public-layout".into()),
    );
    std::fs::create_dir_all(&dir).unwrap();
    if let Some((pixels, reference)) = visual::chrome(&html, None, 0.0, name, &dir) {
        let (diff, delta) = visual::compare(
            &frame.rgba,
            &pixels,
            &geometry,
            &reference,
            visual::Tolerance::default(),
        );
        visual::png(
            &dir.join(format!("{name}-native.png")),
            &frame.rgba,
            600,
            420,
        );
        visual::png(&dir.join(format!("{name}-diff.png")), &delta, 600, 420);
        std::fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_vec_pretty(
                &serde_json::json!({"native":geometry,"chrome":reference,"diff":diff}),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            diff.within_tolerance,
            "{name}: {diff:?}; native={geometry}; Chrome={reference}"
        );
    } else {
        assert_ne!(
            std::env::var("WEGA_NATIVE_PINNED").as_deref(),
            Ok("1"),
            "Chrome is required for this gate"
        );
    }
}
#[test]
fn percentage_basis_in_indefinite_column_preserves_content() {
    compare(
        "indefinite-column",
        "<div data-probe id=column style='display:flex;flex-direction:column;width:400px'><main data-probe id=main style='display:flex;flex-direction:column;flex:1'><header data-probe id=header><div style='height:70px;background:red'></div></header><section data-probe id=section style='padding:10px 0'><div style='height:90px;background:blue'></div></section></main><footer data-probe id=footer style='height:40px;background:green'></footer></div>",
    );
}
#[test]
fn scrollable_sidebar_does_not_push_main_outside_viewport() {
    compare(
        "scrollable-sidebar",
        "<div data-probe id=row style='display:flex;width:600px'><nav data-probe id=sidebar style='flex:0 0 200px;overflow-x:hidden;background:red'><div style='height:100px;color:transparent'>WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW</div></nav><main data-probe id=main style='flex:1;min-width:0;background:blue'><div style='height:120px'></div></main></div>",
    );
}
#[test]
fn specified_width_caps_automatic_flex_minimum() {
    compare(
        "specified-auto-min",
        "<div style='display:flex;width:600px'><div data-probe id=sidebar style='width:200px;flex:0 0 auto;background:red'><div style='height:100px;color:transparent'>WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW</div></div><main data-probe id=main style='flex:1;min-width:0;height:120px;background:blue'></main></div>",
    );
}

#[test]
fn rem_lengths_use_the_document_root_font_size() {
    compare(
        "root-rem",
        "<style>html{font-size:12px}body{font-size:1.5rem}</style><div data-probe id=rem style='width:10rem;height:2rem;padding:0.5rem;background:red'></div><div data-probe id=em style='width:10em;height:2em;background:blue'></div>",
    );
}

#[test]
fn variable_flex_shorthand_retains_basis_and_cascade_order() {
    compare(
        "variable-flex",
        "<style>:root{--sidebar:200px}.sidebar{flex:0 0 var(--sidebar)}.later{flex:0 0 var(--sidebar);flex-basis:150px}</style><div style='display:flex;width:600px'><nav data-probe id=sidebar class=sidebar style='overflow:hidden;height:100px;background:red'></nav><main data-probe id=main style='flex:1;min-width:0;height:120px;background:blue'></main></div><div style='display:flex;width:600px'><nav data-probe id=later class=later style='height:50px;background:green'></nav><main data-probe id=other style='flex:1;height:50px'></main></div>",
    );
}

#[test]
fn inline_block_intrinsic_width_measures_contents() {
    compare(
        "inline-block-intrinsic",
        "<p style='margin:0'>Before <code data-probe id=code style='display:inline-block;font:16px/20px Arial;background:yellow'>Vec&lt;T&gt;</code> after</p><header data-probe id=heading style='display:inline-block'><h2 style='margin:0;font:16px/20px Arial'>Heading</h2></header>",
    );
}

#[test]
fn flex_intrinsic_and_basis_sizes_include_padding_and_border() {
    compare(
        "padded-flex",
        "<div style='display:flex;width:600px'><div data-probe id=auto style='padding:0 20px;border:2px solid red;background:yellow;color:transparent'>Width</div><div data-probe id=basis style='box-sizing:content-box;flex:0 0 100px;padding:0 20px;border:2px solid blue;height:30px'></div><div data-probe id=rest style='flex:1;height:30px;background:green'></div></div>",
    );
}
#[test]
fn nested_inline_replaced_element_contributes_size() {
    compare(
        "nested-inline-image",
        "<div data-probe id=container style='width:400px;text-align:center'><a href='#'><img data-probe id=image width=60 height=60 src='data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22 width=%2260%22 height=%2260%22%3E%3Crect width=%2260%22 height=%2260%22 fill=%22red%22/%3E%3C/svg%3E'></a></div>",
    );
}

#[test]
fn max_width_with_auto_width_centers_auto_margins() {
    compare(
        "max-width-auto-margins",
        "<body data-probe id=body style='max-width:300px;margin:auto;padding:20px;background:yellow'><div data-probe id=content style='height:80px;background:red'></div>",
    );
}
