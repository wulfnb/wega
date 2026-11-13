//! Page-facing Cache API regressions, using real V8 bindings and SQLite storage.
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

fn page(root: &std::path::Path, address: &str) -> View {
    let engine = Engine::new(
        Limits::default(),
        NetworkPolicy {
            offline: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .with_storage_dir(root.to_owned());
    let mut page = engine.create_view(Url::parse(address).unwrap()).unwrap();
    page.set_html("<!doctype html><title>Cache API</title><body>Storage</body>")
        .unwrap();
    page
}
fn run(page: &mut View, body: &str) -> Value {
    page.evaluate(&format!("globalThis.cacheResult=null;(async()=>{{{body}}})().then(value=>cacheResult={{ok:value}},error=>cacheResult={{error:error.name,message:error.message}});")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        page.poll();
        let result: Value =
            serde_json::from_str(&page.evaluate_json("cacheResult").unwrap()).unwrap();
        if !result.is_null() {
            return result;
        }
        assert!(Instant::now() < deadline, "Cache API task timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn cache_responses_preserve_bytes_metadata_and_consume_only_the_supplied_body() {
    let dir = tempfile::tempdir().unwrap();
    let mut view = page(dir.path(), "https://cache.test/page");
    assert_eq!(
        run(
            &mut view,
            r#"
        const cache=await caches.open('assets');
        const response=new Response(new Uint8Array([0,127,128,255]), {status:201,statusText:'Created',headers:{'x-cache':'yes'}});
        const copy=response.clone();
        await cache.put('/binary#ignored', response);
        const found=await cache.match('/binary');
        let immutable=false;
        try { found.headers.set('x-cache','changed'); } catch(e) { immutable=e instanceof TypeError; }
        return {bytes:[...new Uint8Array(await found.arrayBuffer())],copy:[...new Uint8Array(await copy.arrayBuffer())],status:found.status,text:found.statusText,url:found.url,header:found.headers.get('x-cache'),used:response.bodyUsed,immutable,keys:(await cache.keys()).map(r=>r.url)};
    "#
        ),
        json!({"ok":{"bytes":[0,127,128,255],"copy":[0,127,128,255],"status":201,"text":"Created","url":"","header":"yes","used":true,"immutable":true,"keys":["https://cache.test/binary#ignored"]}})
    );
}
#[test]
fn cache_matching_respects_vary_query_method_and_named_cache_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut view = page(dir.path(), "https://cache.test/");
    assert_eq!(
        run(
            &mut view,
            r#"
        const cache=await caches.open('v1');
        for (const language of ['en','de']) await cache.put(new Request('https://cache.test/doc?v=1',{headers:{'accept-language':language}}),new Response(language,{headers:{vary:'Accept-Language'}}));
        const get=(lang,search='?v=1',method='GET')=>new Request('https://cache.test/doc'+search,{method,headers:{'accept-language':lang}});
        const de=await (await cache.match(get('de'))).text();
        const missing=(await cache.match(get('fr')))===undefined;
        const search=(await cache.match(get('en','?v=2')))===undefined;
        const ignored=await (await cache.match(get('en','?v=2'),{ignoreSearch:true})).text();
        const post=(await cache.match(get('en','?v=1','POST')))===undefined;
        const method=await (await cache.match(get('en','?v=1','POST'),{ignoreMethod:true})).text();
        const all=(await cache.matchAll('/doc?v=1',{ignoreVary:true})).length;
        await caches.open('v2');
        return {de,missing,search,ignored,post,method,all,names:await caches.keys(),named:!!await caches.match(get('en'),{cacheName:'v1'}),absent:await caches.match(get('en'),{cacheName:'absent'})===undefined};
    "#
        ),
        json!({"ok":{"de":"de","missing":true,"search":true,"ignored":"en","post":true,"method":"en","all":2,"names":["v1","v2"],"named":true,"absent":true}})
    );
}
#[test]
fn cache_storage_survives_engine_restart_and_isolates_origins_and_profiles() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut view = page(root.path(), "https://one.test/");
        assert_eq!(
            run(
                &mut view,
                "const c=await caches.open('persist');await c.put('/value',new Response('saved'));return true;"
            ),
            json!({"ok":true})
        );
    }
    for address in [
        "https://one.test/other",
        "https://two.test/",
        "https://one.test:8443/",
    ] {
        let mut view = page(root.path(), address);
        let expected = address == "https://one.test/other";
        assert_eq!(
            run(&mut view, "return await caches.has('persist');"),
            json!({"ok":expected})
        );
        if expected {
            assert_eq!(
                run(
                    &mut view,
                    "return await (await caches.match('/value')).text();"
                ),
                json!({"ok":"saved"})
            );
        }
    }
    let other = tempfile::tempdir().unwrap();
    let mut view = page(other.path(), "https://one.test/");
    assert_eq!(
        run(&mut view, "return await caches.keys();"),
        json!({"ok":[]})
    );
}
#[test]
fn cache_rejects_unsafe_responses_and_enforces_quota_without_losing_old_data() {
    let root = tempfile::tempdir().unwrap();
    let mut view = page(root.path(), "https://cache.test/");
    assert_eq!(
        run(
            &mut view,
            r#"
        const cache=await caches.open('v1');
        await cache.put('/key',new Response('original'));
        const errors=[];
        for (const response of [new Response('partial',{status:206}),new Response('vary',{headers:{vary:'*'}}),new Response(new Uint8Array(1024*1024+1))]) {
            try { await cache.put('/key',response);errors.push('accepted'); } catch(e) { errors.push(e.name); }
        }
        try { await cache.put(new Request('https://cache.test/post',{method:'POST'}),new Response('bad')); } catch(e) { errors.push(e.name); }
        return {errors,value:await (await cache.match('/key')).text(),removed:await cache.delete('/key'),absent:await cache.match('/key')===undefined};
    "#
        ),
        json!({"ok":{"errors":["TypeError","TypeError","QuotaExceededError","TypeError"],"value":"original","removed":true,"absent":true}})
    );
}
#[test]
fn cache_is_exposed_only_in_supported_secure_contexts() {
    let root = tempfile::tempdir().unwrap();
    for (address, available) in [
        ("https://cache.test/", true),
        ("http://localhost/", true),
        ("http://127.0.0.1/", true),
        ("http://[::1]/", true),
        ("http://cache.test/", false),
        ("about:blank", false),
        ("data:text/html,test", false),
    ] {
        let mut view = page(root.path(), address);
        assert_eq!(
            view.evaluate_json("typeof caches === 'object'").unwrap(),
            available.to_string(),
            "{address}"
        );
    }
}

#[test]
fn cache_add_all_fetches_real_responses_atomically_and_reads_without_the_server() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::atomic::{AtomicBool, Ordering},
        thread,
    };
    struct Server {
        address: String,
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let finished = stop.clone();
    let thread = thread::spawn(move || {
        while !finished.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Accepted sockets inherit O_NONBLOCK from the listener on macOS/BSD; without this
                    // the byte-wise read below sees WouldBlock and answers 404 before the request arrives.
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = Vec::new();
                    while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                        let mut byte = [0];
                        if stream.read_exact(&mut byte).is_err() {
                            break;
                        }
                        request.push(byte[0]);
                    }
                    let line = String::from_utf8_lossy(&request);
                    eprintln!(
                        "Cache fixture: {}",
                        line.lines().next().unwrap_or("empty request")
                    );
                    let found = line.split_whitespace().nth(1) == Some("/asset");
                    let (status, body) = if found {
                        ("200 OK", "offline asset")
                    } else {
                        ("404 Not Found", "missing")
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1))
                }
                Err(error) => panic!("Fixture listener failed: {error}"),
            }
        }
    });
    let server = Server {
        address,
        stop,
        thread: Some(thread),
    };
    let root = tempfile::tempdir().unwrap();
    let engine = Engine::new(
        Limits::default(),
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .with_storage_dir(root.path().to_owned());
    let mut view = engine
        .create_view(Url::parse(&format!("{}/", server.address)).unwrap())
        .unwrap();
    view.set_html("<title>Fetch Cache</title>").unwrap();
    assert_eq!(
        run(
            &mut view,
            r#"
        const cache=await caches.open('offline');
        const failures=[];
        try { await cache.addAll(['/asset','/missing']); } catch(e) { failures.push(e.name); }
        const after404=(await cache.keys()).length;
        try { await cache.addAll(['/asset','/asset']); } catch(e) { failures.push(e.name); }
        const afterDuplicate=(await cache.keys()).length;
        await cache.add('/asset');
        return {failures,after404,afterDuplicate,stored:(await cache.keys()).length};
    "#
        ),
        json!({"ok":{"failures":["TypeError","InvalidStateError"],"after404":0,"afterDuplicate":0,"stored":1}})
    );
    let address = server.address.clone();
    drop(server);
    assert_eq!(
        run(
            &mut view,
            "const response=await caches.match('/asset');return {text:await response.text(),url:response.url};"
        ),
        json!({"ok":{"text":"offline asset","url":format!("{address}/asset")}})
    );
}
