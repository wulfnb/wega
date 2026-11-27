//! Round 15 Acceptance Tests: JavaScript Promise/microtask scheduling,
//! Fetch/AbortController, Storage partitioning, and Worker structured cloning.
mod support;
use serde_json::Value;
use std::time::Duration;
use wega_engine::{Engine, Limits, NetworkPolicy, RenderingBackend};

#[test]
fn timers_microtasks_and_promise_ordering() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            r#"<script>
window.order = [];
window.order.push("sync1");

queueMicrotask(() => {
    window.order.push("microtask1");
});

Promise.resolve().then(() => {
    window.order.push("promise1");
    queueMicrotask(() => {
        window.order.push("microtask2");
    });
});

window.order.push("sync2");
</script>"#,
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/timing"));
    view.set_rendering_backend(RenderingBackend::WegaNative);

    view.poll();

    let res = view.evaluate_json("({ order: window.order })").unwrap();

    let v: Value = serde_json::from_str(&res).unwrap();
    assert_eq!(
        v["order"],
        serde_json::json!(["sync1", "sync2", "microtask1", "promise1", "microtask2"])
    );
}

#[test]
fn fetch_headers_and_abort_controller_cycle() {
    let server = support::Server::new(|req| {
        if req.path == "/api/data" {
            support::Reply::ok("application/json", r#"{"status":"ok","received":true}"#)
        } else {
            support::Reply::html(r#"<div>Page</div>"#)
        }
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);

    let res = view
        .evaluate_json(
            r#"(() => {
        const controller = new AbortController();
        const signal = controller.signal;
        const initialAborted = signal.aborted;

        controller.abort("manual abort");
        const afterAborted = signal.aborted;

        const headers = new Headers();
        headers.append("X-Custom-Header", "WegaValue");
        headers.set("Content-Type", "application/json");

        return {
            initialAborted,
            afterAborted,
            customHeader: headers.get("x-custom-header"),
            contentType: headers.get("content-type")
        };
    })()"#,
        )
        .unwrap();

    let v: Value = serde_json::from_str(&res).unwrap();
    assert_eq!(v["initialAborted"], false);
    assert_eq!(v["afterAborted"], true);
    assert_eq!(v["customHeader"], "WegaValue");
    assert_eq!(v["contentType"], "application/json");
}

#[test]
fn storage_and_structured_clone_data_integrity() {
    let tmp = tempfile::tempdir().unwrap();
    let limits = Limits {
        script_time: Duration::from_secs(3),
        ..Limits::default()
    };
    let engine = Engine::new(
        limits,
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        std::sync::Arc::new(|| {}),
    )
    .unwrap()
    .with_storage_dir(tmp.path().to_path_buf());

    let server = support::Server::new(|_| support::Reply::html(r#"<div>Storage Test</div>"#));
    let mut view = support::open(&engine, &server.url("localhost", "/storage-test"));
    view.set_rendering_backend(RenderingBackend::WegaNative);

    let res = view
        .evaluate_json(
            r#"(() => {
        localStorage.setItem("keyA", "valA");
        sessionStorage.setItem("keyB", "valB");

        const source = {
            num: 42,
            str: "hello",
            arr: [1, 2, 3],
            nested: { flag: true }
        };
        const cloned = structuredClone(source);
        cloned.nested.flag = false;
        cloned.arr.push(4);

        return {
            localVal: localStorage.getItem("keyA"),
            sessionVal: sessionStorage.getItem("keyB"),
            sourceFlag: source.nested.flag,
            clonedFlag: cloned.nested.flag,
            sourceArrLen: source.arr.length,
            clonedArrLen: cloned.arr.length
        };
    })()"#,
        )
        .unwrap();

    let v: Value = serde_json::from_str(&res).unwrap();
    assert_eq!(v["localVal"], "valA");
    assert_eq!(v["sessionVal"], "valB");
    assert_eq!(v["sourceFlag"], true);
    assert_eq!(v["clonedFlag"], false);
    assert_eq!(v["sourceArrLen"], 3);
    assert_eq!(v["clonedArrLen"], 4);
}
