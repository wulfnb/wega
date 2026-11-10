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

#[test]
fn post_frame_message_dispatches_message_event_with_origin_and_target_filtering() {
    let mut view = page();
    view.set_html("<p>Message target</p>").unwrap();
    view.evaluate_json(
        r#"(() => {
        globalThis.inbound = [];
        window.addEventListener('message', e => {
            inbound.push({ data: e.data, origin: e.origin, source: e.source });
        });
    })()"#,
    )
    .unwrap();

    // 1. Valid origin matching targetOrigin
    view.post_frame_message(
        wega_protocol::identity::FrameId(1),
        "https://trusted.test",
        Some("https://example.test"),
        r#"{"greeting":"hello","code":42}"#,
    );
    let count1: u32 = serde_json::from_str(&view.evaluate_json("inbound.length").unwrap()).unwrap();
    assert_eq!(count1, 1);
    let msg1: serde_json::Value =
        serde_json::from_str(&view.evaluate_json("inbound[0]").unwrap()).unwrap();
    assert_eq!(msg1["origin"], "https://trusted.test");
    assert_eq!(msg1["data"]["greeting"], "hello");
    assert_eq!(msg1["data"]["code"], 42);

    // 2. Mismatching targetOrigin -> message dropped
    view.post_frame_message(
        wega_protocol::identity::FrameId(1),
        "https://other.test",
        Some("https://mismatch.test"),
        r#"{"greeting":"dropped"}"#,
    );
    let count2: u32 = serde_json::from_str(&view.evaluate_json("inbound.length").unwrap()).unwrap();
    assert_eq!(count2, 1);

    // 3. Wildcard '*' targetOrigin -> accepted
    view.post_frame_message(
        wega_protocol::identity::FrameId(1),
        "https://wildcard.test",
        Some("*"),
        r#""wildcard-accepted""#,
    );
    let count3: u32 = serde_json::from_str(&view.evaluate_json("inbound.length").unwrap()).unwrap();
    assert_eq!(count3, 2);
    let msg2: serde_json::Value =
        serde_json::from_str(&view.evaluate_json("inbound[1]").unwrap()).unwrap();
    assert_eq!(msg2["origin"], "https://wildcard.test");
    assert_eq!(msg2["data"], "wildcard-accepted");
}

#[test]
fn frame_tree_and_origin_policy_isolation() {
    use wega_protocol::{
        DocumentContext, DocumentId, DocumentRevision,
        authority::is_permitted_cross_origin_property,
        browsing::{ContextKey, ContextOrigin, FrameTree, Placement, Sandbox},
        identity::{
            FrameId, PartitionId, ProcessSessionId, ServiceIdentity, ServiceSessionId,
            SessionIdentity, TabId,
        },
    };

    fn make_key(frame: u64, process: u8, revision: u64) -> ContextKey {
        ContextKey {
            session: SessionIdentity {
                tab: TabId(1),
                frame: FrameId(frame),
                process: ProcessSessionId([process; 16]),
                service: Some(ServiceIdentity {
                    service: ServiceSessionId([1; 16]),
                    partition: PartitionId([2; 16]),
                }),
            },
            document: DocumentContext {
                document: DocumentId(frame),
                revision: DocumentRevision(revision),
            },
        }
    }

    let top_key = make_key(1, 1, 1);
    let mut tree = FrameTree::new(top_key, Url::parse("https://parent.test/app").unwrap()).unwrap();
    assert_eq!(tree.root(), FrameId(1));

    // 1. Same-origin frame (about:blank) inherits parent origin and SameAgent placement
    let c1_id = tree
        .attach(top_key, 10, "about:blank", Sandbox::default())
        .unwrap();
    let c1_ctx = tree.get(c1_id).unwrap();
    assert_eq!(c1_ctx.origin.serialized(), "https://parent.test");
    assert_eq!(c1_ctx.placement, Placement::SameAgent(FrameId(1)));
    tree.bind_process(c1_id, make_key(c1_id.0, 1, 1)).unwrap();
    assert!(tree.can_access(top_key, c1_id).unwrap());

    // 2. Cross-origin frame gets isolated placement
    let c2_id = tree
        .attach(
            top_key,
            20,
            "https://foreign.test/embed",
            Sandbox::default(),
        )
        .unwrap();
    let c2_ctx = tree.get(c2_id).unwrap();
    assert_eq!(c2_ctx.origin.serialized(), "https://foreign.test");
    assert_eq!(c2_ctx.placement, Placement::Isolated);
    // Same process cannot bind an isolated frame
    assert!(tree.bind_process(c2_id, make_key(c2_id.0, 1, 1)).is_err());
    // Independent process binds successfully
    tree.bind_process(c2_id, make_key(c2_id.0, 2, 1)).unwrap();
    assert!(!tree.can_access(top_key, c2_id).unwrap());

    // 3. Sandboxed frame gets opaque origin even with about:srcdoc
    let c3_id = tree
        .attach(
            top_key,
            30,
            "about:srcdoc",
            Sandbox::parse(Some("allow-scripts")),
        )
        .unwrap();
    let c3_ctx = tree.get(c3_id).unwrap();
    assert_eq!(c3_ctx.origin.serialized(), "null");
    assert!(matches!(c3_ctx.origin, ContextOrigin::Opaque { .. }));
    tree.bind_process(c3_id, make_key(c3_id.0, 3, 1)).unwrap();
    assert!(!tree.can_access(top_key, c3_id).unwrap());

    // 4. Cross-origin property access check
    assert!(is_permitted_cross_origin_property("postMessage"));
    assert!(is_permitted_cross_origin_property("location"));
    assert!(is_permitted_cross_origin_property("parent"));
    assert!(is_permitted_cross_origin_property("top"));
    assert!(is_permitted_cross_origin_property("window"));
    assert!(is_permitted_cross_origin_property("frames"));
    assert!(is_permitted_cross_origin_property("closed"));
    assert!(!is_permitted_cross_origin_property("document"));
    assert!(!is_permitted_cross_origin_property("eval"));
    assert!(!is_permitted_cross_origin_property("cookie"));
    assert!(!is_permitted_cross_origin_property("localStorage"));

    // 5. Detach removes frame and revokes access
    let detached = tree.detach(top_key, c1_id).unwrap();
    assert_eq!(detached, vec![c1_id]);
    assert!(tree.get(c1_id).is_none());
    assert!(tree.validate(make_key(c1_id.0, 1, 1)).is_err());
}
