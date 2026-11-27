//! D3.0 comparison: the identical benchmark runs against baseline and D3-A adapters.
use html5ever::{LocalName, QualName, ns};
use std::time::Instant;
use wega_adapter_blitz::{BlitzDocument, DocumentSetup};
use wega_dom::{DocumentMutation, DocumentView, MutationClock, Mutator, NodeView};
#[test]
#[ignore = "BENCH: compare identical profiles on the same machine"]
fn native_mutation_epoch_cost() {
    let mut d = BlitzDocument::from_html(
        "<!doctype html><body><p id=x>text</p>",
        DocumentSetup::default(),
    );
    let p = d.element_by_id("x").unwrap();
    let text = d.node(p).unwrap().child_at(0).unwrap();
    let name = QualName::new(None, ns!(), LocalName::from("data-k"));
    for op in ["attribute", "text"] {
        let epoch = d.mutation_epoch().as_u64();
        let mut samples = Vec::new();
        for _ in 0..7 {
            let start = Instant::now();
            let mut m = d.mutate();
            for i in 0..20_000 {
                let value = if i % 2 == 0 { "a" } else { "b" };
                if op == "attribute" {
                    m.set_attribute(p, name.clone(), value).unwrap();
                } else {
                    m.set_text(text, value).unwrap();
                }
            }
            m.finish();
            samples.push(start.elapsed().as_secs_f64() * 1e6 / 20_000.0);
        }
        assert_eq!(d.mutation_epoch().as_u64().wrapping_sub(epoch), 140_000);
        samples.sort_by(f64::total_cmp);
        println!(
            "MUTATION_BENCH {op}: median_us={:.4} min_us={:.4} max_us={:.4}",
            samples[3], samples[0], samples[6]
        );
    }
}
