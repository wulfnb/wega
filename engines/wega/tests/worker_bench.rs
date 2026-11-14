//! Worker execution timings (Round 12): worker creation, message round trips and parallel
//! workers, using blob workers so no server is needed. Run:
//! `cargo test --release -p wega-engine --test worker_bench -- --ignored --nocapture`
use std::sync::Arc;
use std::time::{Duration, Instant};
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

fn run(page: &mut View, body: &str) -> f64 {
    let start = Instant::now();
    page.evaluate(&format!(
        "globalThis.__result=null;(async()=>{{{body}}})().then(v=>__result={{ok:v===undefined?null:v}},e=>__result={{error:String(e)}});"
    ))
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        page.poll();
        let result = page.evaluate_json("__result").unwrap();
        if result != "null" {
            assert!(result.contains("ok"), "{result}");
            return start.elapsed().as_secs_f64();
        }
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_micros(200));
    }
}
fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}
#[test]
#[ignore]
fn worker_paths() {
    let engine = Engine::new(
        Limits::default(),
        NetworkPolicy {
            offline: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap();
    let mut page = engine
        .create_view(Url::parse("https://bench.test/").unwrap())
        .unwrap();
    page.set_html("<!doctype html><title>w</title>").unwrap();
    page.poll();
    let echo = "const mk=()=>new Worker(URL.createObjectURL(new Blob(['onmessage=e=>postMessage(e.data)'],{type:'text/javascript'})));";
    let spawn = median((0..7).map(|_| run(&mut page, &format!("{echo} for(let i=0;i<10;i++){{const w=mk();await new Promise(r=>{{w.onmessage=r;w.postMessage(i)}});w.terminate()}}")) * 1e3 / 10.0).collect());
    let round_trip = median((0..7).map(|_| run(&mut page, &format!("{echo} const w=mk();await new Promise(r=>{{w.onmessage=r;w.postMessage(0)}});for(let i=0;i<200;i++)await new Promise(r=>{{w.onmessage=r;w.postMessage(i)}});w.terminate()")) * 1e6 / 200.0).collect());
    let parallel = median((0..7).map(|_| run(&mut page, &format!("{echo} const ws=[...Array(8)].map(mk);await Promise.all(ws.map((w,i)=>new Promise(r=>{{w.onmessage=r;w.postMessage(i)}})));ws.forEach(w=>w.terminate())")) * 1e3).collect());
    println!("BENCH worker create + first reply + terminate: {spawn:.2} ms/worker");
    println!("BENCH worker message round trip: {round_trip:.2} us/round-trip");
    println!("BENCH 8 parallel workers create + reply: {parallel:.2} ms/batch");
}
