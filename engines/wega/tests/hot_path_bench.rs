//! Hot-path timings used to check that architectural refactors add no measurable cost.
//! Run: `cargo test -p wega-engine --test hot_path_bench -- --ignored --nocapture`
//! (debug build; compare numbers only between runs of the same profile and machine state).
use std::{sync::Arc, time::Instant};
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

fn view() -> View {
    let engine = Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
    let mut view = engine
        .create_view(Url::parse("https://bench.test/").unwrap())
        .unwrap();
    view.set_html("<!doctype html><title>b</title><body><div id=root></div></body>")
        .unwrap();
    view
}
fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}
fn repeat(mut run: impl FnMut() -> f64) -> f64 {
    median((0..7).map(|_| run()).collect())
}
#[test]
#[ignore]
fn hot_paths() {
    let mut page = view();
    let eval = repeat(|| {
        let start = Instant::now();
        for _ in 0..20_000 {
            page.evaluate_json("1+1").unwrap();
        }
        start.elapsed().as_secs_f64() * 1e6 / 20_000.0
    });
    let dom_ops = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');r.textContent='';for(let i=0;i<2000;i++){const d=document.createElement('div');d.setAttribute('data-i',String(i));r.appendChild(d);d.getAttribute('data-i');}return r.childNodes.length})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 2_000.0
    });
    let engine = Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
    let lifecycle = repeat(|| {
        let start = Instant::now();
        for _ in 0..30 {
            let mut v = engine
                .create_view(Url::parse("https://bench.test/").unwrap())
                .unwrap();
            v.set_html("<!doctype html><title>x</title><body>x</body>")
                .unwrap();
        }
        start.elapsed().as_secs_f64() * 1e3 / 30.0
    });
    println!("BENCH evaluate_json(1+1): {eval:.2} us/call");
    println!(
        "BENCH dom ops (createElement+setAttribute+appendChild+getAttribute): {dom_ops:.2} us/iteration"
    );
    println!("BENCH create_view+set_html (document lifecycle): {lifecycle:.2} ms/document");
}
