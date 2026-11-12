//! Worker runtime lifecycle: cancellation while asynchronous work is pending and nested
//! asynchronous execution inside the adapter-owned executor.

mod support;

use serde_json::{Value, json};
use std::time::{Duration, Instant};
use support::{Reply, Server};
use url::Url;
use wega_engine::{Engine, Limits, NetworkPolicy, View};

fn engine() -> Engine {
    Engine::new(
        Limits::default(),
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        std::sync::Arc::new(|| {}),
    )
    .unwrap()
}
fn page(engine: &Engine, address: &str) -> View {
    let mut view = engine.create_view(Url::parse(address).unwrap()).unwrap();
    view.set_html("<!doctype html><title>Workers</title>")
        .unwrap();
    view.poll();
    view
}
fn run(page: &mut View, body: &str) -> Value {
    page.evaluate(&format!(
        "globalThis.__result=null;(async()=>{{{body}}})().then(value=>__result={{ok:value===undefined?null:value}},error=>__result={{error:error.name,message:String(error.message)}});"
    ))
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        page.poll();
        let result: Value = serde_json::from_str(&page.evaluate_json("__result").unwrap()).unwrap();
        if !result.is_null() {
            return result;
        }
        assert!(Instant::now() < deadline, "page task timed out: {body}");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn ok(page: &mut View, body: &str) -> Value {
    let result = run(page, body);
    assert!(result.get("ok").is_some(), "page task failed: {result}");
    result["ok"].clone()
}
fn script(body: &str) -> Reply {
    Reply::ok("text/javascript", body.as_bytes().to_vec())
}
const NEXT: &str = "const next=(w)=>new Promise((resolve,reject)=>{w.onmessage=e=>resolve(e.data);w.onerror=e=>reject(new Error(e.message));});";

#[test]
fn terminating_a_worker_with_pending_timers_fetches_and_promises_does_not_hang() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/pending.js" => script(
            "postMessage('ready'); setInterval(()=>{},3); \
             fetch('/slow').then(()=>postMessage('never')).catch(()=>{}); \
             new Promise(()=>{}).then(()=>{}); onmessage = ()=>{ for(;;){} };",
        ),
        "/slow" => Reply::ok("text/plain", "late").delay(Duration::from_millis(1500)),
        _ => Reply::html("").status("404 Not Found"),
    });
    let engine = engine();
    let mut view = page(&engine, &server.url("localhost", "/"));
    let base = server.url("localhost", "/");
    let started = Instant::now();
    assert_eq!(
        ok(
            &mut view,
            &format!(
                "{NEXT} const w=new Worker('{base}pending.js'); await next(w); w.postMessage(1); \
                 await new Promise(r=>setTimeout(r,60)); w.terminate(); return 'terminated';"
            ),
        ),
        "terminated"
    );
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "terminate must not wait for pending work"
    );
    // The page and the engine stay usable, and new workers start normally.
    assert_eq!(
        ok(
            &mut view,
            &format!(
                "{NEXT} const w=new Worker(URL.createObjectURL(new Blob(['postMessage(7*6)']))); const v=await next(w); w.terminate(); return v;"
            ),
        ),
        42
    );
}

#[test]
fn nested_async_work_in_classic_and_module_workers_completes() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/classic.js" => script(
            "onmessage = async (e) => { \
               const out = []; \
               await Promise.all([1,2,3].map(async (n) => { \
                 await new Promise(r => setTimeout(r, 5 * n)); \
                 const text = await (await fetch('/data/' + n)).text(); \
                 await Promise.resolve().then(() => queueMicrotask(() => out.push(text))); \
               })); \
               await new Promise(r => setTimeout(r, 20)); \
               postMessage(out.sort().join(',')); };",
        ),
        // The handler is installed synchronously (messages sent during top-level await are
        // not queued, as in browsers); the async work happens inside the handler.
        "/module.js" => script(
            "onmessage = async e => { \
               await new Promise(r => setTimeout(r, 10)); \
               const { twice } = await import('./dep.js'); \
               const body = await (await fetch('/data/9')).text(); \
               postMessage(twice(e.data) + ':' + body); };",
        ),
        "/dep.js" => script("export const twice = (n) => n * 2;"),
        path if path.starts_with("/data/") => Reply::ok("text/plain", &path[6..]),
        _ => Reply::html("").status("404 Not Found"),
    });
    let engine = engine();
    let mut view = page(&engine, &server.url("localhost", "/"));
    let base = server.url("localhost", "/");
    let result = ok(
        &mut view,
        &format!(
            "{NEXT} const c=new Worker('{base}classic.js'); c.postMessage(0); const classic=await next(c); c.terminate(); \
             const m=new Worker('{base}module.js',{{type:'module'}}); m.postMessage(21); const module=await next(m); m.terminate(); \
             return {{classic, module}};"
        ),
    );
    assert_eq!(result, json!({"classic": "1,2,3", "module": "42:9"}));
}

#[test]
fn many_workers_can_be_created_and_terminated_in_parallel_and_in_sequence() {
    let engine = engine();
    let mut view = page(&engine, "https://app.test/");
    let result = ok(
        &mut view,
        &format!(
            "{NEXT} const src=URL.createObjectURL(new Blob(['onmessage=e=>postMessage(e.data+1); setInterval(()=>{{}},2);'])); \
             const batch=[...Array(6)].map(()=>new Worker(src)); \
             const replies=await Promise.all(batch.map((w,i)=>{{const p=next(w); w.postMessage(i); return p;}})); \
             batch.forEach(w=>w.terminate()); \
             let sequential=0; for(let i=0;i<8;i++){{ const w=new Worker(src); const p=next(w); w.postMessage(i); sequential+=await p; w.terminate(); }} \
             return {{replies, sequential}};"
        ),
    );
    assert_eq!(
        result,
        json!({"replies": [1, 2, 3, 4, 5, 6], "sequential": 36})
    );
}
