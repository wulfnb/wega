//! Console call cost, measured in-process. Uses only APIs that exist in both the D1 and D2
//! engines, so the same file can be run against either source tree (see the D2 report):
//!
//!     cargo test -p wega-engine --test console_bench -- --ignored --nocapture 2>/dev/null
//!
//! `BENCH name: value unit` lines; the per-call figures include the page's existing console
//! formatting and stderr diagnostics, which D2 does not change.
use std::{sync::Arc, time::Instant};
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

fn page() -> View {
    let mut view = Engine::new(
        Limits::default(),
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .create_view(Url::parse("about:blank").unwrap())
    .unwrap();
    view.stop();
    view.url = Url::parse("https://fixture.test/bench.html").unwrap();
    view.set_html("<p>bench</p>").unwrap();
    for _ in 0..8 {
        view.poll();
    }
    view
}
fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}
fn p95(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[((samples.len() - 1) as f64 * 0.95).round() as usize]
}

#[test]
#[ignore = "BENCH"]
fn console_paths() {
    let mut view = page();
    let rss = || wega_platform::resident_bytes().unwrap_or(0) as f64 / 1_048_576.0;
    view.evaluate_json("globalThis.wide=Object.fromEntries(Array.from({length:100},(_, i)=>['k'+i,'v'+i]));globalThis.nested={a:{b:{c:[1,2,3]}},s:'str',n:1};true").unwrap();
    let cases = [
        ("console.log('x', i)", "log(string, number)"),
        ("console.log('x', nested)", "log(string, nested object)"),
        ("console.warn('w', i)", "warn(string, number)"),
        ("console.log(i,i,i,i,i,i,i,i)", "log(8 numbers)"),
        ("console.log(wide)", "log(100-property object)"),
    ];
    let n = 2000;
    for (call, label) in cases {
        let mut samples = Vec::new();
        for _ in 0..7 {
            let source = format!("for(let i=0;i<{n};i++){{{call}}};true");
            let started = Instant::now();
            view.evaluate_json(&source).unwrap();
            samples.push(started.elapsed().as_secs_f64() * 1e6 / n as f64);
        }
        println!(
            "BENCH console {label} (median): {:.2} us/call",
            median(samples.clone())
        );
        println!("BENCH console {label} (p95): {:.2} us/call", p95(samples));
    }
    // Memory under a sustained flood: 5 x 20,000 calls with object arguments.
    let before = rss();
    let mut peak = before;
    for _ in 0..5 {
        view.evaluate_json(
            "for(let i=0;i<20000;i++)console.log('flood',i,{i,s:'z'.repeat(i%300)});true",
        )
        .unwrap();
        peak = peak.max(rss());
    }
    println!("BENCH console flood resident start: {before:.1} MiB");
    println!("BENCH console flood resident peak after 100,000 calls: {peak:.1} MiB");
    println!(
        "BENCH console flood resident growth: {:.1} MiB",
        peak - before
    );
}
