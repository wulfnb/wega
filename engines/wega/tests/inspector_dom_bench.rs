//! Reproducible bounded-reader latency/RSS measurements; no layout or script execution.
use std::time::Instant;
use wega_adapter_blitz::{BlitzDocument, DocumentSetup};
use wega_dom::{DocumentView, MutationClock, NodeView};
use wega_engine::inspector::dom::inspect;
use wega_protocol::{
    DocumentContext, DocumentId, DocumentRevision,
    inspector::{dom::*, *},
};
#[test]
#[ignore = "BENCH: run explicitly, report build profile and machine load"]
fn bounded_dom_latency_and_memory() {
    let document = DocumentContext {
        document: DocumentId(1),
        revision: DocumentRevision(1),
    };
    let rss = || wega_platform::resident_bytes().unwrap_or(0) as f64 / 1_048_576.0;
    for (name, html) in [
        (
            "small",
            format!(
                "<!doctype html><body><main id=target>{}</main>",
                "<p data-v=x>text</p>".repeat(16)
            ),
        ),
        (
            "large",
            format!(
                "<!doctype html><body><main id=target>{}</main>",
                "<i data-v=x>text</i>".repeat(15000)
            ),
        ),
        (
            "deep",
            format!(
                "<!doctype html><body><main id=target>{}x{}</main>",
                "<div>".repeat(1000),
                "</div>".repeat(1000)
            ),
        ),
    ] {
        let before = rss();
        let d = BlitzDocument::from_html(&html, DocumentSetup::default());
        let built = rss();
        let key = d.element_by_id("target").unwrap();
        let epoch = d.mutation_epoch().as_u64();
        let id = InspectorNodeId {
            document,
            key: key.as_u64(),
        };
        let offset = if name == "large" { 14936 } else { 0 };
        let mut deepest = key;
        if name == "deep" {
            for _ in 0..140 {
                deepest = d.node(deepest).unwrap().children().next().unwrap();
            }
        }
        for (operation, command, args) in [
            ("root", InspectorCommand::GetDomRoot, InspectorArgs::None),
            (
                "children",
                InspectorCommand::GetDomChildren,
                InspectorArgs::DomChildren {
                    node: id,
                    epoch,
                    offset,
                    limit: 64,
                },
            ),
            (
                "describe",
                InspectorCommand::DescribeDomNode,
                InspectorArgs::DomNode {
                    node: InspectorNodeId {
                        document,
                        key: deepest.as_u64(),
                    },
                },
            ),
        ] {
            let mut samples = Vec::new();
            let mut max_bytes = 0;
            let mut errors = 0;
            let retained = rss();
            for _ in 0..1000 {
                let start = Instant::now();
                match inspect(&d, document, command, &args) {
                    Ok(page) => max_bytes = max_bytes.max(serde_json::to_vec(&page).unwrap().len()),
                    Err(e) => {
                        errors += 1;
                        assert!(
                            e.code == InspectorErrorCode::BudgetExceeded
                                || e.code == InspectorErrorCode::Timeout
                        );
                    }
                }
                samples.push(start.elapsed().as_secs_f64() * 1e6);
            }
            samples.sort_by(f64::total_cmp);
            println!(
                "DOM_BENCH {name} {operation}: median_us={:.3} p95_us={:.3} max_us={:.3} response_bytes={max_bytes} refused={errors} rss_growth_mib={:.3}",
                samples[500],
                samples[950],
                samples[999],
                rss() - retained
            );
        }
        assert_eq!(d.mutation_epoch().as_u64(), epoch);
        println!(
            "DOM_BENCH {name}: document_rss_mib={:.3} after_inspection_rss_mib={:.3}",
            built - before,
            rss() - built
        );
    }
}
