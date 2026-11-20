//! D3 final benchmarks: geometry read, hit test, picker round trip, DOM edit and inline-CSS edit
//! latency (in process, and through the real worker pipe). Run optimized:
//! `cargo test --release --locked -p wega-engine --test inspector_edit_bench -- --ignored --nocapture --test-threads=1`
mod support;
use std::{sync::Arc, time::Instant};
use support::inspector_worker::Worker;
use wega_engine::{Engine, Limits, NetworkPolicy, PagePort, Url, View};
use wega_protocol::{
    DocumentContext, DocumentId, DocumentRevision,
    inspector::{
        InspectorArgs, InspectorCommand, InspectorResponse, dom::InspectorNodeId, edit::MutationOp,
        geometry::to_milli,
    },
};

fn stamp() -> DocumentContext {
    DocumentContext {
        document: DocumentId(1),
        revision: DocumentRevision(1),
    }
}
fn view(html: &str) -> View {
    let mut v = Engine::new(
        Limits::default(),
        NetworkPolicy {
            offline: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .create_view(Url::parse("https://probe.test/").unwrap())
    .unwrap();
    v.set_html(html).unwrap();
    v.poll();
    v.paint(800, 600, 1.0, 1.0, false).unwrap();
    v
}
fn find(v: &View, id: &str) -> InspectorNodeId {
    use wega_protocol::inspector::InspectorCommand as C;
    let d = stamp();
    let root = v
        .inspect_dom(d, C::GetDomRoot, &InspectorArgs::None)
        .unwrap();
    let mut stack = vec![(root.nodes[0].id, root.epoch)];
    while let Some((node, epoch)) = stack.pop() {
        let mut offset = 0;
        loop {
            let page = v
                .inspect_dom(
                    d,
                    C::GetDomChildren,
                    &InspectorArgs::DomChildren {
                        node,
                        epoch,
                        offset,
                        limit: 64,
                    },
                )
                .unwrap();
            for child in &page.nodes {
                if child
                    .attributes
                    .iter()
                    .any(|a| a.name == "id" && a.value == id)
                {
                    return child.id;
                }
                if child.node_type == 1 && child.child_count > 0 {
                    stack.push((child.id, epoch));
                }
            }
            match page.next_offset {
                Some(next) => offset = next,
                None => break,
            }
        }
    }
    panic!("no #{id}")
}
fn stats(mut s: Vec<f64>) -> (f64, f64, f64) {
    s.sort_by(f64::total_cmp);
    (
        s[s.len() / 2],
        s[((s.len() - 1) as f64 * 0.95).round() as usize],
        s[s.len() - 1],
    )
}
fn report(name: &str, samples: Vec<f64>) {
    let (p50, p95, max) = stats(samples);
    println!("BENCH {name}: p50={p50:.3} us p95={p95:.3} us max={max:.3} us");
}
fn time<F: FnMut()>(n: usize, mut f: F) -> Vec<f64> {
    (0..n)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1e6
        })
        .collect()
}

const SMALL: &str = "<!doctype html><body style='margin:0'><div id=box style='margin:10px;padding:5px;border:2px solid;width:100px;height:50px'>x</div>";

#[test]
#[ignore = "BENCH"]
fn in_process_latencies() {
    for (label, html) in [
        ("small page", SMALL.to_string()),
        (
            "5000 paragraphs",
            format!("{SMALL}{}", "<p>row</p>".repeat(5000)),
        ),
        (
            "20000-rule stylesheet",
            format!(
                "<!doctype html><style>{}</style><body style='margin:0'><div id=box style='width:100px;height:50px'>x</div>",
                (0..20_000)
                    .map(|i| format!(".c{i}{{color:red}}"))
                    .collect::<String>()
            ),
        ),
    ] {
        let mut v = view(&html);
        let node = find(&v, "box");
        let d = stamp();
        report(
            &format!("[{label}] GetGeometry read"),
            time(5000, || {
                std::hint::black_box(v.inspect_geometry(d, node).unwrap());
            }),
        );
        report(
            &format!("[{label}] PickNode (hit test + report)"),
            time(5000, || {
                std::hint::black_box(v.pick_node(d, 20.0, 20.0).unwrap());
            }),
        );
        let mut n = 0u64;
        report(
            &format!("[{label}] attribute edit (apply_edit)"),
            time(3000, || {
                n += 1;
                let epoch = v.mutation_epoch();
                v.apply_edit(
                    d,
                    node,
                    epoch,
                    &MutationOp::SetAttribute {
                        name: "data-b".into(),
                        value: n.to_string(),
                    },
                    false,
                )
                .unwrap();
            }),
        );
        report(
            &format!("[{label}] inline CSS edit (apply_edit)"),
            time(3000, || {
                n += 1;
                let epoch = v.mutation_epoch();
                v.apply_edit(
                    d,
                    node,
                    epoch,
                    &MutationOp::SetInlineProperty {
                        property: "width".into(),
                        value: format!("{}px", 100 + n % 50),
                        important: false,
                    },
                    false,
                )
                .unwrap();
            }),
        );
        // The cost the page pays for the edit: the next style + layout flush.
        report(
            &format!("[{label}] inline CSS edit + next layout flush"),
            time(300, || {
                n += 1;
                let epoch = v.mutation_epoch();
                v.apply_edit(
                    d,
                    node,
                    epoch,
                    &MutationOp::SetInlineProperty {
                        property: "height".into(),
                        value: format!("{}px", 40 + n % 50),
                        important: false,
                    },
                    false,
                )
                .unwrap();
                v.evaluate_json("document.getElementById('box').getBoundingClientRect().height")
                    .unwrap();
            }),
        );
    }
}

#[test]
#[ignore = "BENCH"]
fn real_worker_round_trips() {
    let mut w = Worker::start();
    let s = w.attach();
    w.evaluate(&format!(
        "document.body.innerHTML={SMALL:?};document.body.getBoundingClientRect().height;true"
    ))
    .unwrap();
    let root = match w.call(s, InspectorCommand::GetDomRoot) {
        Ok(InspectorResponse::Dom(p)) => p.nodes[0].id,
        o => panic!("{o:?}"),
    };
    let _ = root;
    // A node to read: the document root is always valid.
    let node = root;
    report(
        "[real worker] GetGeometry round trip (IPC + read)",
        time(1500, || {
            let r = w.call_with(
                s,
                InspectorCommand::GetGeometry,
                InspectorArgs::DomNode { node },
            );
            assert!(matches!(r, Ok(InspectorResponse::Geometry(_))), "{r:?}");
        }),
    );
    report(
        "[real worker] PickNode round trip = picker hover latency (IPC + hit test + report)",
        time(1500, || {
            let r = w.call_with(
                s,
                InspectorCommand::PickNode,
                InspectorArgs::Pick {
                    x_milli: to_milli(20.0),
                    y_milli: to_milli(20.0),
                },
            );
            assert!(matches!(r, Ok(InspectorResponse::Picked(_))), "{r:?}");
        }),
    );
    report(
        "[real worker] GetStatusSnapshot round trip (reference)",
        time(1500, || {
            w.snapshot(s);
        }),
    );
}
