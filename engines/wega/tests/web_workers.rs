//! Workers and offline execution: real worker realms with Web APIs, script
//! loading through the network stack, structured clone and transfer, worker
//! canvas rendering, and service worker lifecycle and fetch handling.

mod support;

use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use support::{Reply, Server};
use url::Url;
use wega_engine::{
    Engine, Limits, NetworkPolicy, View,
    workers::{ArrayBufferTransfer, MessagePortHub},
};

fn online() -> NetworkPolicy {
    NetworkPolicy {
        allow_loopback: true,
        ..Default::default()
    }
}

fn engine(storage: Option<&std::path::Path>, policy: NetworkPolicy) -> Engine {
    let engine = Engine::new(Limits::default(), policy, Arc::new(|| {})).unwrap();
    match storage {
        Some(path) => engine.with_storage_dir(path.to_owned()),
        None => engine,
    }
}

fn page(engine: &Engine, address: &str) -> View {
    let mut view = engine.create_view(Url::parse(address).unwrap()).unwrap();
    view.set_html("<!doctype html><title>Workers</title><canvas id=c width=40 height=20></canvas>")
        .unwrap();
    view.poll();
    view
}

/// Run an async function body in the page and wait for its result.
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

/// Resolves with the next message (or error) from a worker.
const NEXT: &str = "const next=(w)=>new Promise((resolve,reject)=>{w.onmessage=e=>resolve(e.data);w.onerror=e=>reject(new Error(e.message));});";

#[test]
fn transferable_ownership_detaches_source_buffer() {
    let mut source = vec![10u8, 20, 30, 40, 50];
    assert_eq!(source.len(), 5);

    let transferred = ArrayBufferTransfer::detach(&mut source);
    assert_eq!(
        source.len(),
        0,
        "Source buffer must be detached with 0 length"
    );
    assert_eq!(transferred.byte_length, 5);
    assert_eq!(transferred.bytes, vec![10, 20, 30, 40, 50]);
}

#[test]
fn message_channel_port_entanglement_and_closure() {
    let mut hub = MessagePortHub::new();
    let (p1, p2) = hub.create_channel();

    // Port 1 sends to Port 2
    hub.post_message(p1, "\"hello from p1\"".into(), vec![])
        .unwrap();
    let p2_messages = hub.poll_messages(p2);
    assert_eq!(p2_messages.len(), 1);
    assert_eq!(p2_messages[0].data, "\"hello from p1\"");

    // Port 2 replies to Port 1
    hub.post_message(p2, "\"reply from p2\"".into(), vec![])
        .unwrap();
    let p1_messages = hub.poll_messages(p1);
    assert_eq!(p1_messages.len(), 1);
    assert_eq!(p1_messages[0].data, "\"reply from p2\"");

    // Closing Port 1 detangles and discards messages
    hub.close(p1);
    assert!(
        hub.post_message(p2, "\"lost message\"".into(), vec![])
            .is_ok()
    );
    assert!(hub.poll_messages(p1).is_empty());
}

#[test]
fn dedicated_worker_loads_its_script_and_has_a_real_global_scope() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/worker.js" => script(
            r#"
            importScripts('lib.js');
            self.onmessage = async (event) => {
                const reply = await fetch('data.json').then(r => r.json());
                await new Promise(resolve => setTimeout(resolve, 5));
                postMessage({
                    scope: self instanceof DedicatedWorkerGlobalScope,
                    hasDocument: typeof document,
                    name: self.name,
                    href: location.href,
                    secure: isSecureContext,
                    lib: libValue,
                    fetched: reply.value,
                    echo: event.data,
                    isMap: event.data.map instanceof Map,
                    isDate: event.data.when instanceof Date,
                    bytes: [...event.data.bytes],
                });
            };
            "#,
        ),
        "/lib.js" => script("var libValue = 'imported';"),
        "/data.json" => Reply::ok("application/json", br#"{"value":42}"#.to_vec()),
        _ => Reply::html("").status("404 Not Found"),
    });
    let engine = engine(None, online());
    let mut view = page(&engine, &server.url("localhost", "/app/"));
    let worker = server.url("localhost", "/worker.js");
    let result = ok(
        &mut view,
        &format!(
            "{NEXT} const w=new Worker('{worker}',{{name:'crunch'}});
             w.postMessage({{map:new Map([['a',1]]),when:new Date(5),bytes:new Uint8Array([1,2,3]),nested:{{list:[1,'two']}}}});
             const data=await next(w); w.terminate();
             return {{...data, echo:{{mapSize:data.echo.map.size,when:data.echo.when.getTime(),nested:data.echo.nested}}}};"
        ),
    );
    assert_eq!(
        result,
        json!({
            "scope": true, "hasDocument": "undefined", "name": "crunch", "href": worker,
            "secure": true, "lib": "imported", "fetched": 42, "isMap": true, "isDate": true,
            "bytes": [1, 2, 3],
            "echo": {"mapSize": 1, "when": 5, "nested": {"list": [1, "two"]}},
        })
    );
    assert_eq!(server.requests("/worker.js").len(), 1);
}

#[test]
fn dedicated_worker_failures_are_reported_and_runaway_workers_can_be_terminated() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/throws.js" => script("onmessage = () => { throw new RangeError('boom'); };"),
        "/html.js" => Reply::html("<p>not a script</p>"),
        "/spin.js" => script("postMessage('spinning'); onmessage = () => { for (;;) {} };"),
        _ => Reply::html("").status("404 Not Found"),
    });
    let engine = engine(None, online());
    let mut view = page(&engine, &server.url("localhost", "/"));
    let base = server.url("localhost", "/");
    let failure = |view: &mut View, file: &str, post: bool| {
        run(
            view,
            &format!(
                "{NEXT} const w=new Worker('{base}{file}'); {} return await next(w);",
                if post { "w.postMessage(1);" } else { "" }
            ),
        )
    };
    let missing = failure(&mut view, "missing.js", false);
    assert!(
        missing["message"].as_str().unwrap().contains("404"),
        "{missing}"
    );
    let wrong_type = failure(&mut view, "html.js", false);
    assert!(
        wrong_type["message"].as_str().unwrap().contains("MIME"),
        "{wrong_type}"
    );
    let thrown = failure(&mut view, "throws.js", true);
    assert!(
        thrown["message"].as_str().unwrap().contains("boom"),
        "{thrown}"
    );

    let cross = run(
        &mut view,
        "new Worker('https://other.test/worker.js'); return 1;",
    );
    assert_eq!(cross["error"], "SecurityError");

    // terminate() must stop a worker that never yields.
    let started = Instant::now();
    assert_eq!(
        ok(
            &mut view,
            &format!(
                "{NEXT} const w=new Worker('{base}spin.js'); await next(w); w.postMessage(1); await new Promise(r=>setTimeout(r,50)); w.terminate(); return 'stopped';"
            ),
        ),
        "stopped"
    );
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn blob_and_module_workers_run_and_transfers_move_ownership() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/module.js" => {
            script("import { twice } from './dep.js'; onmessage = e => postMessage(twice(e.data));")
        }
        "/dep.js" => script("export const twice = value => value * 2;"),
        _ => Reply::html("").status("404 Not Found"),
    });
    let engine = engine(None, online());
    let mut view = page(&engine, &server.url("localhost", "/"));
    let result = ok(
        &mut view,
        &format!(
            r#"{NEXT}
            const source = "onmessage = e => {{ const view = new Uint8Array(e.data.buffer); e.data.port.postMessage({{sum: view[0] + view[1], length: view.length}}); }};";
            const blob = new Worker(URL.createObjectURL(new Blob([source], {{type: 'text/javascript'}})));
            const channel = new MessageChannel();
            const buffer = new Uint8Array([40, 2]).buffer;
            const reply = new Promise(resolve => {{ channel.port1.onmessage = e => resolve(e.data); }});
            blob.postMessage({{buffer, port: channel.port2}}, [buffer, channel.port2]);
            const transferred = await reply;
            blob.terminate();
            const module = new Worker('{}', {{type: 'module'}});
            module.postMessage(21);
            const doubled = await next(module);
            module.terminate();
            let illegal = false;
            try {{ new MessagePort(); }} catch (e) {{ illegal = e instanceof TypeError; }}
            let uncloneable = '';
            try {{ channel.port1.postMessage(() => 1); }} catch (e) {{ uncloneable = e.name; }}
            return {{transferred, detached: buffer.byteLength, doubled, illegal, uncloneable}};"#,
            server.url("localhost", "/module.js")
        ),
    );
    assert_eq!(
        result,
        json!({"transferred": {"sum": 42, "length": 2}, "detached": 0, "doubled": 42, "illegal": true, "uncloneable": "DataCloneError"})
    );
}

#[test]
fn shared_worker_is_shared_by_same_origin_documents() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/shared.js" => script(
            "let total = 0; onconnect = e => { const port = e.ports[0]; port.onmessage = m => { total += m.data; port.postMessage({total, scope: self instanceof SharedWorkerGlobalScope, name: self.name}); }; };",
        ),
        _ => Reply::html("").status("404 Not Found"),
    });
    let engine = engine(None, online());
    let address = server.url("localhost", "/");
    let add = format!(
        "const w=new SharedWorker('{}shared.js','tally'); return await new Promise(resolve=>{{ w.port.onmessage=e=>resolve(e.data); w.port.postMessage(AMOUNT); }});",
        address
    );
    let mut first = page(&engine, &address);
    assert_eq!(
        ok(&mut first, &add.replace("AMOUNT", "10")),
        json!({"total": 10, "scope": true, "name": "tally"})
    );
    let mut second = page(&engine, &format!("{address}other"));
    assert_eq!(ok(&mut second, &add.replace("AMOUNT", "25"))["total"], 35);
    assert_eq!(
        server.requests("/shared.js").len(),
        1,
        "one realm serves both documents"
    );
}

#[test]
fn offscreen_canvas_renders_in_a_worker_and_commits_to_its_placeholder() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/paint.js" => script(
            r#"
            onmessage = (event) => {
                const local = new OffscreenCanvas(8, 8);
                const context = local.getContext('2d');
                context.fillStyle = '#ff0000';
                context.fillRect(0, 0, 8, 8);
                const own = [...context.getImageData(2, 2, 1, 1).data];
                const placeholder = event.data.canvas;
                const target = placeholder.getContext('2d');
                target.fillStyle = '#0000ff';
                target.fillRect(0, 0, placeholder.width, placeholder.height);
                setTimeout(() => postMessage({own, size: [placeholder.width, placeholder.height]}), 20);
            };
            "#,
        ),
        _ => Reply::html("").status("404 Not Found"),
    });
    let engine = engine(None, online());
    let mut view = page(&engine, &server.url("localhost", "/"));
    let result = ok(
        &mut view,
        &format!(
            "{NEXT} const canvas=document.getElementById('c'); const offscreen=canvas.transferControlToOffscreen();
             const w=new Worker('{}'); w.postMessage({{canvas:offscreen}},[offscreen]);
             const reply=await next(w); w.terminate();
             const probe=document.createElement('canvas'); probe.width=40; probe.height=20;
             const context=probe.getContext('2d'); context.drawImage(canvas,0,0);
             let detached=false; try {{ offscreen.getContext('2d'); }} catch (e) {{ detached=e.name==='InvalidStateError'; }}
             return {{reply, placeholder:[...context.getImageData(10,10,1,1).data], detached}};",
            server.url("localhost", "/paint.js")
        ),
    );
    assert_eq!(
        result,
        json!({"reply": {"own": [255, 0, 0, 255], "size": [40, 20]}, "placeholder": [0, 0, 255, 255], "detached": true})
    );
}

const SERVICE_WORKER: &str = r#"
const VERSION = 'v1';
self.addEventListener('install', (event) => {
    self.skipWaiting();
    event.waitUntil(caches.open(VERSION).then(cache => cache.addAll(['/app/data.json'])));
});
self.addEventListener('activate', (event) => { event.waitUntil(clients.claim()); });
self.addEventListener('fetch', (event) => {
    const url = new URL(event.request.url);
    if (url.pathname === '/app/generated')
        event.respondWith(new Response('made by ' + VERSION, {status: 203, headers: {'x-from': 'sw'}}));
    else if (url.pathname === '/app/data.json')
        event.respondWith(caches.match(event.request).then(hit => hit || fetch(event.request)));
});
self.addEventListener('message', (event) => {
    event.source.postMessage({echo: event.data, scope: registration.scope, kind: self instanceof ServiceWorkerGlobalScope});
});
"#;

fn service_server(version: Arc<Mutex<String>>) -> Server {
    Server::new(move |seen| match seen.path.as_str() {
        "/app/sw.js" => {
            script(&SERVICE_WORKER.replace("'v1'", &format!("'{}'", version.lock().unwrap())))
        }
        "/app/data.json" => Reply::ok(
            "application/json",
            br#"{"status":"offline-ready"}"#.to_vec(),
        ),
        "/app/plain" => Reply::ok("text/plain", b"from network".to_vec()),
        "/app/broken.js" => script("this is not javascript"),
        "/app/rejects.js" => script(
            "addEventListener('install', e => e.waitUntil(Promise.reject(new Error('no'))));",
        ),
        "/deep/sw.js" => script(""),
        _ => Reply::html("").status("404 Not Found"),
    })
}

/// Register and wait until the worker controls the page.
const ACTIVATE: &str = "const registration=await navigator.serviceWorker.register('/app/sw.js'); \
    const ready=await navigator.serviceWorker.ready; \
    while(!navigator.serviceWorker.controller) await new Promise(r=>setTimeout(r,10));";

#[test]
fn service_worker_script_runs_through_install_activate_and_handles_fetches() {
    let server = service_server(Arc::new(Mutex::new("v1".into())));
    let engine = engine(None, online());
    let mut view = page(&engine, &server.url("localhost", "/app/index.html"));
    let result = ok(
        &mut view,
        "const states=[]; const pending=navigator.serviceWorker.register('/app/sw.js');
             const registration=await pending; const worker=registration.installing||registration.waiting||registration.active;
             states.push(worker.state); worker.addEventListener('statechange',()=>states.push(worker.state));
             const ready=await navigator.serviceWorker.ready;
             while(!navigator.serviceWorker.controller) await new Promise(r=>setTimeout(r,10));
             const made=await fetch('/app/generated'); const plain=await fetch('/app/plain');
             const reply=new Promise(resolve=>navigator.serviceWorker.addEventListener('message',e=>resolve(e.data)));
             navigator.serviceWorker.controller.postMessage({hello:'worker'});
             return {scope:registration.scope, same:ready===registration, final:registration.active.state,
               script:registration.active.scriptURL.endsWith('/app/sw.js'), states,
               made:[made.status, made.headers.get('x-from'), await made.text()], plain:await plain.text(),
               reply:await reply, found:(await navigator.serviceWorker.getRegistration('/app/page')).scope,
               count:(await navigator.serviceWorker.getRegistrations()).length};",
    );
    let scope = server.url("localhost", "/app/");
    assert_eq!(result["scope"], scope);
    assert_eq!(result["same"], true);
    assert_eq!(result["final"], "activated");
    assert_eq!(result["script"], true);
    assert_eq!(result["made"], json!([203, "sw", "made by v1"]));
    assert_eq!(result["plain"], "from network");
    assert_eq!(
        result["reply"],
        json!({"echo": {"hello": "worker"}, "scope": scope, "kind": true})
    );
    assert_eq!(result["found"], scope);
    assert_eq!(result["count"], 1);
    let states = result["states"].as_array().unwrap();
    assert_eq!(states.last().unwrap(), "activated", "{states:?}");
    assert_eq!(
        server.requests("/app/data.json").len(),
        1,
        "install precached the resource"
    );
}

#[test]
fn service_worker_registration_rejects_bad_scripts_and_scopes() {
    let server = service_server(Arc::new(Mutex::new("v1".into())));
    let engine = engine(None, online());
    let mut view = page(&engine, &server.url("localhost", "/app/index.html"));
    let register = |view: &mut View, call: &str| {
        run(
            view,
            &format!("await navigator.serviceWorker.register({call}); return 'registered';"),
        )
    };
    assert_eq!(
        register(&mut view, "'/app/missing.js'")["error"],
        "TypeError"
    );
    assert_eq!(
        register(&mut view, "'/app/broken.js'")["error"],
        "TypeError"
    );
    assert_eq!(
        register(&mut view, "'https://other.test/sw.js'")["error"],
        "SecurityError"
    );
    assert_eq!(
        register(&mut view, "'/deep/sw.js', {scope: '/'}")["error"],
        "SecurityError"
    );
    assert_eq!(
        ok(
            &mut view,
            "return (await navigator.serviceWorker.getRegistrations()).length;"
        ),
        0
    );
    // A rejected install promise makes the worker redundant and leaves no registration.
    let rejected = ok(
        &mut view,
        "const registration=await navigator.serviceWorker.register('/app/rejects.js'); const worker=registration.installing;
         if(worker && worker.state!=='redundant') await new Promise(resolve=>worker.addEventListener('statechange',()=>worker.state==='redundant'&&resolve()));
         await new Promise(r=>setTimeout(r,50));
         return (await navigator.serviceWorker.getRegistrations()).length;",
    );
    assert_eq!(rejected, 0);
    let insecure = engine.create_view(Url::parse("http://insecure.test/").unwrap());
    let mut insecure = insecure.unwrap();
    insecure.set_html("<!doctype html><p>x</p>").unwrap();
    assert_eq!(
        insecure
            .evaluate_json("typeof navigator.serviceWorker")
            .unwrap(),
        "\"undefined\""
    );
}

#[test]
fn service_worker_serves_offline_survives_restart_updates_and_unregisters() {
    let dir = tempfile::tempdir().unwrap();
    let version = Arc::new(Mutex::new(String::from("v1")));
    let server = service_server(version.clone());
    let address = server.url("localhost", "/app/index.html");
    {
        let engine = engine(Some(dir.path()), online());
        let mut view = page(&engine, &address);
        ok(&mut view, &format!("{ACTIVATE} return true;"));
        // Offline: cached and generated responses still work; the network does not.
        engine.set_policy(NetworkPolicy {
            offline: true,
            ..Default::default()
        });
        let offline = run(
            &mut view,
            "const data=await (await fetch('/app/data.json')).json(); const made=await (await fetch('/app/generated')).text();
             let network='reached'; try { await fetch('/app/plain'); } catch (e) { network='blocked'; }
             return {data, made, network};",
        );
        assert_eq!(
            offline["ok"],
            json!({"data": {"status": "offline-ready"}, "made": "made by v1", "network": "blocked"})
        );
    }
    // A new engine restores the registration from storage and serves offline at once.
    {
        let engine = engine(
            Some(dir.path()),
            NetworkPolicy {
                offline: true,
                ..Default::default()
            },
        );
        let mut view = page(&engine, &address);
        let restored = ok(
            &mut view,
            "const registration=await navigator.serviceWorker.getRegistration(); const data=await (await fetch('/app/data.json')).json();
             return {state:registration.active.state, controlled:!!navigator.serviceWorker.controller, data};",
        );
        assert_eq!(
            restored,
            json!({"state": "activated", "controlled": true, "data": {"status": "offline-ready"}})
        );
    }
    // A changed script installs a new worker; an unchanged one does not.
    let engine = engine(Some(dir.path()), online());
    let mut view = page(&engine, &address);
    let before = server.requests("/app/sw.js").len();
    assert_eq!(
        ok(
            &mut view,
            "const registration=await navigator.serviceWorker.getRegistration(); await registration.update(); return !!registration.installing || !!registration.waiting;"
        ),
        false
    );
    assert_eq!(server.requests("/app/sw.js").len(), before + 1);
    *version.lock().unwrap() = "v2".into();
    let updated = ok(
        &mut view,
        "const registration=await navigator.serviceWorker.getRegistration(); let found=false; registration.onupdatefound=()=>found=true;
         await registration.update();
         for(let i=0;i<500;i++){ const text=await (await fetch('/app/generated')).text(); if(text==='made by v2') return {found, text}; await new Promise(r=>setTimeout(r,10)); }
         return {found, text:'stale'};",
    );
    assert_eq!(updated, json!({"found": true, "text": "made by v2"}));
    let removed = ok(
        &mut view,
        "const registration=await navigator.serviceWorker.getRegistration(); const removed=await registration.unregister();
         const after=await (await fetch('/app/plain')).text();
         let generated='gone'; try { const r=await fetch('/app/generated'); generated=String(r.status); } catch (e) { generated='failed'; }
         return {removed, after, generated, left:(await navigator.serviceWorker.getRegistrations()).length};",
    );
    assert_eq!(
        removed,
        json!({"removed": true, "after": "from network", "generated": "404", "left": 0})
    );
}

#[test]
fn document_teardown_terminates_its_workers() {
    let engine = engine(None, online());
    let mut view = page(&engine, "https://app.test/page1.html");
    assert_eq!(
        ok(
            &mut view,
            "const w=new Worker(URL.createObjectURL(new Blob(['let n=0; setInterval(()=>postMessage(++n),5);'])));
             return await new Promise(resolve=>{ w.onmessage=e=>e.data>=3&&resolve('ticking'); });",
        ),
        "ticking"
    );
    drop(view);
    let mut next = page(&engine, "https://app.test/page2.html");
    assert_eq!(next.evaluate_json("2 + 2").unwrap(), "4");
}
