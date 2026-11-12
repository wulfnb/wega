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
    // Node lookup, node info and subtree walks (getElementById, nodeType/nodeName, deep clone).
    let lookups = repeat(|| {
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');r.textContent='';for(let i=0;i<200;i++){const d=document.createElement('div');d.id='n'+i;d.appendChild(document.createElement('span'));r.appendChild(d);}return r.childNodes.length})()",
        )
        .unwrap();
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{let n=0;for(let i=0;i<2000;i++){const e=document.getElementById('n'+(i%200));n+=e.nodeType+e.nodeName.length+(e.parentNode?1:0);}return n})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 2_000.0
    });
    let clone = repeat(|| {
        // A fresh 51-node subtree per sample keeps the clones within the node budget.
        let mut page = view();
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');for(let i=0;i<25;i++){const d=document.createElement('div');d.appendChild(document.createElement('span'));r.appendChild(d);}return r.childNodes.length})()",
        )
        .unwrap();
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');let n=0;for(let i=0;i<10;i++){n+=r.cloneNode(true).childNodes.length}return n})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 10.0
    });
    // Mutation paths: insertion/removal, attribute and text updates, HTML parsing.
    let insert_remove = repeat(|| {
        let mut page = view();
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');const kids=[];for(let i=0;i<1000;i++){const d=document.createElement('div');r.appendChild(d);kids.push(d);}for(const d of kids)r.removeChild(d);return r.childNodes.length})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 1_000.0
    });
    let attributes = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const e=document.getElementById('root');for(let i=0;i<2000;i++){e.setAttribute('data-k',String(i));e.removeAttribute('data-k');}return 1})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 2_000.0
    });
    let text = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const t=document.createTextNode('a');document.getElementById('root').appendChild(t);for(let i=0;i<2000;i++){t.data='x'+i;}return 1})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 2_000.0
    });
    let parse = repeat(|| {
        let mut html = String::from("<!doctype html><body><div id=root>");
        for i in 0..3_000 {
            html.push_str(&format!(
                "<div id=n{i} data-i={i}><span>text {i}</span></div>"
            ));
        }
        html.push_str("</div></body>");
        let engine =
            Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
        let mut v = engine
            .create_view(Url::parse("https://bench.test/").unwrap())
            .unwrap();
        let start = Instant::now();
        v.set_html(&html).unwrap();
        start.elapsed().as_secs_f64() * 1e3
    });
    // Round 6 additions: fragment parsing, namespaced creation, shallow clone, inline style.
    let fragment_small = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');for(let i=0;i<500;i++){r.innerHTML='<p class=a>one <b>two</b></p><i>three</i>'}return 1})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 500.0
    });
    let fragment_large = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');const h='<div class=c><span>text</span><a href=x>l</a></div>'.repeat(300);for(let i=0;i<10;i++){r.innerHTML=h}return 1})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e3 / 10.0
    });
    let create_ns = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{let n=0;for(let i=0;i<1000;i++){n+=document.createElementNS('http://www.w3.org/2000/svg','circle').nodeType}return n})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 1_000.0
    });
    let shallow_clone = repeat(|| {
        let mut page = view();
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const e=document.createElement('div');e.setAttribute('a','1');let n=0;for(let i=0;i<500;i++){n+=e.cloneNode(false).nodeType}return n})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 500.0
    });
    let style = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const e=document.getElementById('root');for(let i=0;i<1000;i++){e.style.setProperty('color','red');e.style.removeProperty('color')}return 1})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 1_000.0
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
    println!("BENCH lookup+nodeType+nodeName+parentNode: {lookups:.2} us/iteration");
    println!("BENCH deep clone of a 51-node subtree: {clone:.2} us/clone");
    println!("BENCH appendChild+removeChild: {insert_remove:.2} us/node");
    println!("BENCH setAttribute+removeAttribute: {attributes:.2} us/pair");
    println!("BENCH text node data update: {text:.2} us/update");
    println!("BENCH innerHTML small fragment (3 nodes): {fragment_small:.2} us/call");
    println!("BENCH innerHTML large fragment (900 nodes): {fragment_large:.2} ms/call");
    println!("BENCH createElementNS: {create_ns:.2} us/call");
    println!("BENCH cloneNode(false): {shallow_clone:.2} us/call");
    println!("BENCH style.setProperty+removeProperty: {style:.2} us/pair");
    println!("BENCH parse 9000-node document (set_html): {parse:.2} ms");
    println!("BENCH create_view+set_html (document lifecycle): {lifecycle:.2} ms/document");
}
