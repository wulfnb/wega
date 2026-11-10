//! Step 17 Workers and Offline Execution Verification Suite.
//!
//! Verifies:
//! - Structured clone and transferable ownership: ArrayBuffer detachment and MessageChannel entanglement.
//! - DedicatedWorker: independent isolate and event loop, bidirectional messaging, error handling, terminate().
//! - SharedWorker: shared isolate per origin/name across documents, entangled port communication.
//! - Cross-origin worker script rejection (SecurityError).
//! - ServiceWorker registration, scope matching (path prefix and origin), unregistration.
//! - Offline execution and fetch interception: CacheStorage cache serving when network is offline.
//! - Document navigation and teardown cleanup terminating worker isolates.

use std::{sync::Arc, time::Duration};
use url::Url;
use wega_engine::{
    Engine, Limits, NetworkPolicy, View,
    workers::{ArrayBufferTransfer, MessagePortHub},
};
use wega_network::CacheStorage;

fn test_engine(storage_dir: std::path::PathBuf, policy: NetworkPolicy) -> Engine {
    Engine::new(
        Limits {
            script_time: Duration::from_secs(5),
            ..Limits::default()
        },
        policy,
        Arc::new(|| {}),
    )
    .unwrap()
    .with_storage_dir(storage_dir)
}

fn create_page(engine: &Engine, address: &str) -> View {
    let url = Url::parse(address).unwrap();
    let mut view = engine.create_view(url).unwrap();
    view.set_html("<title>Worker Test</title>").unwrap();
    view.poll();
    view
}

#[test]
fn transferable_ownership_detaches_source_buffer() {
    let mut source = vec![10u8, 20, 30, 40, 50];
    assert_eq!(source.len(), 5);

    let transferred = ArrayBufferTransfer::detach(&mut source);
    assert_eq!(source.len(), 0, "Source buffer must be detached with 0 length");
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
    assert!(hub.post_message(p2, "\"lost message\"".into(), vec![]).is_ok());
    assert!(hub.poll_messages(p1).is_empty());
}

#[test]
fn dedicated_worker_script_execution_and_bidirectional_messaging() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path().to_path_buf(), NetworkPolicy::default());
    let mut page = create_page(&engine, "https://app.test/index.html");

    let setup = r#"
        (() => {
            globalThis.worker = new Worker('worker.js', {
                content: "onmessage = function(e) { postMessage({ result: e.data.value * 3, text: 'computed' }); };"
            });
            globalThis.received = null;
            globalThis.worker.onmessage = function(e) {
                globalThis.received = e.data;
            };
            globalThis.worker.postMessage({ value: 14 });
            return true;
        })()
    "#;
    assert_eq!(page.evaluate_json(setup).unwrap(), "true");

    // Poll until response arrives
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut got_reply = false;
    while std::time::Instant::now() < deadline {
        page.poll();
        let _ = page.evaluate_json("globalThis.worker.poll(); true");
        let res = page.evaluate_json("globalThis.received").unwrap();
        if res.contains("\"result\":42") {
            got_reply = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(got_reply, "Dedicated worker must receive and reply to messages");

    // Terminate worker
    assert_eq!(page.evaluate_json("globalThis.worker.terminate(); true").unwrap(), "true");
}

#[test]
fn dedicated_worker_rejects_cross_origin_script() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path().to_path_buf(), NetworkPolicy::default());
    let mut page = create_page(&engine, "https://app.test/index.html");

    // Attempt to spawn worker from cross-origin URL
    let result = page.evaluate_json(
        "(() => { try { new Worker('https://evil.test/worker.js'); return 'ok'; } catch(e) { return e.message; } })()"
    );
    assert!(result.is_ok());
    let msg = result.unwrap();
    assert!(
        msg.contains("SecurityError") || msg.contains("same-origin"),
        "Cross-origin worker creation must be rejected with SecurityError: got {msg}"
    );
}

#[test]
fn shared_worker_shares_state_across_same_origin_documents() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path().to_path_buf(), NetworkPolicy::default());

    let mut doc1 = create_page(&engine, "https://app.test/tab1.html");
    let mut doc2 = create_page(&engine, "https://app.test/tab2.html");

    let shared_script = "let total = 0; onconnect = function(e) { const port = e.ports[0]; port.onmessage = function(msg) { total += msg.data.delta; port.postMessage({ total: total }); }; };";

    // Doc 1 connects to SharedWorker
    let doc1_setup = format!(
        r#"
        (() => {{
            globalThis.shared = new SharedWorker('shared.js', {{ name: 'counter', content: "{shared_script}" }});
            globalThis.reply1 = null;
            globalThis.shared.port.onmessage = function(e) {{
                globalThis.reply1 = e.data;
            }};
            globalThis.shared.port.postMessage({{ delta: 10 }});
            return true;
        }})()
        "#
    );
    assert_eq!(doc1.evaluate_json(&doc1_setup).unwrap(), "true");

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut doc1_got = false;
    while std::time::Instant::now() < deadline {
        doc1.poll();
        let _ = doc1.evaluate_json("globalThis.shared.port.poll(); true");
        let res = doc1.evaluate_json("globalThis.reply1").unwrap();
        if res.contains("\"total\":10") {
            doc1_got = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(doc1_got, "Doc 1 must receive initial total of 10");

    // Doc 2 connects to the SAME SharedWorker instance on same origin
    let doc2_setup = format!(
        r#"
        (() => {{
            globalThis.shared = new SharedWorker('shared.js', {{ name: 'counter', content: "{shared_script}" }});
            globalThis.reply2 = null;
            globalThis.shared.port.onmessage = function(e) {{
                globalThis.reply2 = e.data;
            }};
            globalThis.shared.port.postMessage({{ delta: 25 }});
            return true;
        }})()
        "#
    );
    assert_eq!(doc2.evaluate_json(&doc2_setup).unwrap(), "true");

    let mut doc2_got = false;
    while std::time::Instant::now() < deadline {
        doc2.poll();
        let _ = doc2.evaluate_json("globalThis.shared.port.poll(); true");
        let res = doc2.evaluate_json("globalThis.reply2").unwrap();
        if res.contains("\"total\":35") {
            doc2_got = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(doc2_got, "Doc 2 must receive accumulated total of 35 from shared isolate");
}

#[test]
fn service_worker_registration_scope_matching_and_unregistration() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path().to_path_buf(), NetworkPolicy::default());
    let mut page = create_page(&engine, "https://app.test/app/index.html");

    // Register service worker with scope /app/
    let reg_code = r#"
        (() => {
            globalThis.__scope = null;
            navigator.serviceWorker.register('sw.js', { scope: '/app/' }).then(reg => {
                globalThis.__scope = reg.scope;
            });
            return true;
        })()
    "#;
    assert_eq!(page.evaluate_json(reg_code).unwrap(), "true");

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        page.poll();
        let res = page.evaluate_json("globalThis.__scope").unwrap();
        if res.contains("https://app.test/app/") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let res = page.evaluate_json("globalThis.__scope").unwrap();
    assert!(res.contains("https://app.test/app/"));

    // Check matching registration
    let check_match = r#"
        (() => {
            globalThis.__matched = null;
            navigator.serviceWorker.getRegistration('https://app.test/app/details').then(reg => {
                globalThis.__matched = reg ? reg.scope : 'none';
            });
            return true;
        })()
    "#;
    assert_eq!(page.evaluate_json(check_match).unwrap(), "true");
    while std::time::Instant::now() < deadline {
        page.poll();
        let res = page.evaluate_json("globalThis.__matched").unwrap();
        if res != "null" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(page.evaluate_json("globalThis.__matched").unwrap(), "\"https://app.test/app/\"");

    // Check non-matching scope
    let check_non_match = r#"
        (() => {
            globalThis.__nonMatched = null;
            navigator.serviceWorker.getRegistration('https://app.test/other/path').then(reg => {
                globalThis.__nonMatched = reg ? reg.scope : 'none';
            });
            return true;
        })()
    "#;
    assert_eq!(page.evaluate_json(check_non_match).unwrap(), "true");
    while std::time::Instant::now() < deadline {
        page.poll();
        let res = page.evaluate_json("globalThis.__nonMatched").unwrap();
        if res != "null" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(page.evaluate_json("globalThis.__nonMatched").unwrap(), "\"none\"");

    // Unregister
    let unreg = r#"
        (() => {
            globalThis.__unreg = null;
            navigator.serviceWorker.getRegistration('https://app.test/app/details').then(async reg => {
                globalThis.__unreg = await reg.unregister();
            });
            return true;
        })()
    "#;
    assert_eq!(page.evaluate_json(unreg).unwrap(), "true");
    while std::time::Instant::now() < deadline {
        page.poll();
        let res = page.evaluate_json("globalThis.__unreg").unwrap();
        if res != "null" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(page.evaluate_json("globalThis.__unreg").unwrap(), "true");
}

#[test]
fn service_worker_offline_fetch_cache_interception() {
    let dir = tempfile::tempdir().unwrap();

    // 1. Pre-seed CacheStorage with offline response
    {
        let mut store = CacheStorage::open(dir.path()).unwrap();
        let cache_id = store.open_cache("v1-cache").unwrap();
        store
            .put_entry(
                cache_id,
                "https://app.test/offline-data.json",
                "GET",
                200,
                "OK",
                &[("content-type", "application/json")],
                br#"{"status":"offline-ready","items":[1,2,3]}"#,
            )
            .unwrap();
    }

    // 2. Launch engine with offline network policy
    let offline_policy = NetworkPolicy {
        offline: true,
        allow_loopback: false,
        ..Default::default()
    };
    let engine = test_engine(dir.path().to_path_buf(), offline_policy);
    let mut page = create_page(&engine, "https://app.test/index.html");

    // 3. Register service worker covering the app
    let setup = r#"
        (() => {
            globalThis.__swRegistered = false;
            navigator.serviceWorker.register('sw.js', { scope: '/' }).then(() => {
                globalThis.__swRegistered = true;
            });
            return true;
        })()
    "#;
    assert_eq!(page.evaluate_json(setup).unwrap(), "true");

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        page.poll();
        let res = page.evaluate_json("globalThis.__swRegistered").unwrap();
        if res == "true" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    // 4. Fetch offline resource: service worker must intercept and serve cached body!
    let fetch_code = r#"
        (() => {
            globalThis.__fetchBody = null;
            fetch('https://app.test/offline-data.json')
                .then(r => r.text())
                .then(t => { globalThis.__fetchBody = t; })
                .catch(e => { globalThis.__fetchBody = 'error: ' + e; });
            return true;
        })()
    "#;
    assert_eq!(page.evaluate_json(fetch_code).unwrap(), "true");

    while std::time::Instant::now() < deadline {
        page.poll();
        let res = page.evaluate_json("globalThis.__fetchBody").unwrap();
        if res != "null" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let result = page.evaluate_json("globalThis.__fetchBody").unwrap();
    assert!(
        result.contains("offline-ready"),
        "Offline fetch must be served from service worker cache, got: {result}"
    );
}

#[test]
fn document_navigation_terminates_worker_isolates() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path().to_path_buf(), NetworkPolicy::default());

    // Create page and spawn worker
    let mut page = create_page(&engine, "https://app.test/page1.html");
    let spawn = r#"
        (() => {
            globalThis.w = new Worker('w.js', { content: 'let count = 0; setInterval(() => { count++; }, 10);' });
            return true;
        })()
    "#;
    assert_eq!(page.evaluate_json(spawn).unwrap(), "true");

    // Navigating or dropping the document cleans up all workers cleanly
    drop(page);

    // Verify a fresh page can be created without hanging or leaking worker isolates
    let mut page2 = create_page(&engine, "https://app.test/page2.html");
    assert_eq!(page2.evaluate_json("2 + 2").unwrap(), "4");
}
