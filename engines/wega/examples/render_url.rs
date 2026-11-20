//! Render one page with the native engine and save what it produced.
//!
//!     cargo run --example render_url -- <url-or-file> <out.png> [width] [height]
//!
//! Writes the PNG and `<out>.layout.txt`, an indented dump of the layout tree
//! (box type, border box, element tag/id/classes) for comparing with Chrome.
use std::{
    fmt::Write as _,
    sync::Arc,
    time::{Duration, Instant},
};
use wega_dom::{DocumentView, NodeView};
use wega_engine::{Engine, Limits, NetworkPolicy, Url};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [target, out, ..] = args.as_slice() else {
        eprintln!("usage: render_url <url-or-file> <out.png> [width] [height]");
        std::process::exit(2);
    };
    let width = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1280u32);
    let height = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(800u32);
    let url = Url::parse(target).unwrap_or_else(|_| {
        Url::from_file_path(std::fs::canonicalize(target).expect("no such file")).unwrap()
    });

    let engine = Engine::new(
        Limits {
            script_time: Duration::from_secs(3),
            ..Limits::default()
        },
        NetworkPolicy::default(),
        Arc::new(|| {}),
    )
    .expect("engine");
    let mut view = engine.create_view(url).expect("view");
    let started = Instant::now();
    let deadline = started + Duration::from_secs(30);
    view.poll();
    while view.loading && Instant::now() < deadline {
        view.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    // Let timers and late stylesheets/images land.
    let settle = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < settle {
        view.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    eprintln!(
        "loaded={} in {:?}; notices: {:?}",
        !view.loading,
        started.elapsed(),
        view.notices
    );

    // The first layout requests CSS background images; paint again once
    // they have arrived, as the browser does on its resource wake-up.
    view.paint(width, height, 1.0, 1.0, false).expect("paint");
    let settle = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < settle {
        view.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    let frame = view.paint(width, height, 1.0, 1.0, false).expect("paint");
    let info = skia_safe::ImageInfo::new(
        (width as i32, height as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let image = skia_safe::images::raster_from_data(
        &info,
        skia_safe::Data::new_copy(&frame.rgba),
        width as usize * 4,
    )
    .expect("image");
    let png = image
        .encode(None, skia_safe::EncodedImageFormat::PNG, None)
        .expect("encode");
    std::fs::write(out, png.as_bytes()).expect("write png");

    let mut dump = String::new();
    if let Some(native) = &view.native_document {
        let native = native.borrow();
        let doc = native.doc.borrow();
        if let Some(tree) = &native.layout_tree {
            fn walk(
                b: &wega_engine::native_engine::layout::LayoutBox,
                doc: &impl DocumentView,
                depth: usize,
                out: &mut String,
            ) {
                let r = b.border_box;
                let label = b
                    .node
                    .and_then(|k| doc.node(k))
                    .map(|n| match n.element_name() {
                        Some(e) => {
                            let mut s = e.local.to_string();
                            if let Some(id) = n.attr("id") {
                                let _ = write!(s, "#{id}");
                            }
                            for c in n
                                .attr("class")
                                .unwrap_or_default()
                                .split_whitespace()
                                .take(3)
                            {
                                let _ = write!(s, ".{c}");
                            }
                            s
                        }
                        None => n
                            .text()
                            .map(|t| format!("{:?}", t.chars().take(40).collect::<String>()))
                            .unwrap_or_default(),
                    })
                    .unwrap_or_default();
                let _ = writeln!(
                    out,
                    "{:indent$}{:?}{} {:.0},{:.0} {:.0}x{:.0} {}",
                    "",
                    b.box_type,
                    if b.inline_level { "(inline)" } else { "" },
                    r.x,
                    r.y,
                    r.width,
                    r.height,
                    label,
                    indent = depth * 2
                );
                for c in &b.children {
                    walk(c, doc, depth + 1, out);
                }
            }
            walk(tree, &*doc, 0, &mut dump);
        } else {
            dump.push_str("no layout tree (layout refused or not built)\n");
        }
    }
    std::fs::write(format!("{out}.layout.txt"), dump).expect("write layout");
    eprintln!("wrote {out}");
}
