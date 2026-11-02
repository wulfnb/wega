//! These regressions check the current fail-closed boundary. They deliberately
//! do not count synthetic documents or an empty iframe box as frame support.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use wega_engine::{Engine, Limits, NetworkPolicy, Url};

fn page() -> wega_engine::View {
    Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {}))
        .unwrap()
        .create_view(Url::parse("https://example.test/index.html").unwrap())
        .unwrap()
}

#[test]
fn unbound_frames_never_expose_synthetic_documents_or_window_proxies() {
    let mut view = page();
    view.set_html(
        r#"<iframe src="about:blank"></iframe>
        <iframe src="https://foreign.test/secret"></iframe>
        <iframe sandbox="allow-scripts" srcdoc="<p>Private</p>"></iframe>"#,
    )
    .unwrap();
    assert_eq!(
        view.evaluate_json(
            r#"(() => {
        const frames = [...document.querySelectorAll('iframe')];
        return frames.length === 3 && window.length === 0 && frames.every(frame =>
            frame.contentWindow === null && frame.contentDocument === null &&
            frame._frameContext === undefined);
    })()"#
        )
        .unwrap(),
        "true"
    );
    view.paint(640, 480, 1.0, 1.0, false).unwrap();
    assert!(
        view.compatibility
            .as_deref()
            .is_some_and(|reason| reason.contains("iframe"))
    );
}

#[test]
fn frame_attributes_remain_native_dom_attributes_without_creating_a_context() {
    let mut view = page();
    view.set_html("<iframe id=f></iframe>").unwrap();
    assert_eq!(
        view.evaluate_json(
            r#"(() => {
        const f = document.getElementById('f');
        f.src = '/child'; f.srcdoc = '<p>Child</p>'; f.name = 'child';
        f.sandbox.add('allow-scripts'); f.sandbox.add('allow-forms');
        f.sandbox.remove('allow-forms');
        const okay = f.src === '/child' && f.name === 'child' &&
            f.sandbox.contains('allow-scripts') && !f.sandbox.contains('allow-forms');
        f.remove();
        return okay && f.contentWindow === null && window.length === 0;
    })()"#
        )
        .unwrap(),
        "true"
    );
}

#[test]
fn window_messages_use_tasks_structured_clone_and_native_transfer() {
    let mut view = page();
    view.set_html("<p>Message target</p>").unwrap();
    let initial = view.evaluate_json(r#"(() => {
        globalThis.received = []; globalThis.order = [];
        addEventListener('message', event => { order.push('message'); received.push({
            data: event.data, origin: event.origin, source: event.source === window
        }); });
        const cyclic = { value: 1 }; cyclic.self = cyclic;
        postMessage(cyclic);
        cyclic.value = 7;
        Promise.resolve().then(() => order.push('microtask'));
        const buffer = new ArrayBuffer(8);
        postMessage({ buffer }, {targetOrigin: 'https://example.test/path', transfer: [buffer]});
        postMessage('drop', 'https://foreign.test');
        let cloneError = false, originError = false;
        try { postMessage(() => {}); } catch(error) { cloneError = error.name === 'DataCloneError'; }
        try { postMessage('bad', 'relative'); } catch(error) { originError = error.name === 'SyntaxError'; }
        return { detached: buffer.byteLength === 0, cloneError, originError, synchronous: received.length };
    })()"#).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&initial).unwrap(),
        serde_json::json!({"cloneError":true,"detached":true,"originError":true,"synchronous":0})
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        view.poll();
        if view.evaluate_json("received.length").unwrap() == "2" {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        view.evaluate_json(
            r#"received.length === 2 && received[0].data.value === 1 &&
        received[0].data.self === received[0].data && received[0].source &&
        received[0].origin === 'https://example.test' && received[1].data.buffer.byteLength === 8 &&
        order[0] === 'microtask'"#
        )
        .unwrap(),
        "true"
    );
}

#[test]
fn failed_or_duplicate_transfers_leave_the_callers_buffers_attached() {
    let mut view = page();
    view.set_html("<p>Clone ownership</p>").unwrap();
    assert_eq!(
        view.evaluate_json(
            r#"(() => {
        const buffer = new ArrayBuffer(16);
        const data = new Uint8Array(buffer); data[0] = 42;
        let failed = false, duplicate = false;
        try { structuredClone({buffer, fn: () => {}}, {transfer: [buffer]}); }
        catch(error) { failed = error.name === 'DataCloneError' && buffer.byteLength === 16; }
        try { structuredClone({buffer}, {transfer: [buffer, buffer]}); }
        catch(error) { duplicate = error.name === 'DataCloneError' && buffer.byteLength === 16; }
        const cloned = structuredClone({data}, {transfer: [buffer]});
        return failed && duplicate && buffer.byteLength === 0 && cloned.data[0] === 42 &&
            typeof __wega_call === 'undefined' && typeof __wega_web === 'undefined' &&
            typeof __wega_assigned_slot === 'undefined';
    })()"#
        )
        .unwrap(),
        "true"
    );
}
