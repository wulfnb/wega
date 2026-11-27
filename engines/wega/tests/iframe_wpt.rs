//! WPT iframe subset test runner (Stage 7).
//!
//! Extends Wega's WPT runner to host iframe fixtures with testharness.js,
//! covering the 7 required areas:
//! 1. iframe navigation
//! 2. same-origin policy
//! 3. window relationships
//! 4. postMessage
//! 5. sandbox
//! 6. nested browsing contexts
//! 7. frame loading and events

mod support;
use std::{
    thread,
    time::{Duration, Instant},
};
use support::{Reply, Server, engine, settle};
use wega_engine::{Engine, Url, View};

const TESTHARNESS_JS: &str = include_str!("fixtures/wpt/testharness.js");

fn setup_wpt(view: &mut View) {
    view.evaluate(TESTHARNESS_JS).unwrap();
    view.evaluate(
        r#"
        setup({ explicit_done: true, output: false });
        globalThis.__wptResults = null;
        add_completion_callback((tests, status) => {
            globalThis.__wptResults = {
                status: status.status,
                total: tests.length,
                passed: tests.filter(t => t.status === 0).length,
                failed: tests.filter(t => t.status === 1).length,
                timeout: tests.filter(t => t.status === 2).length,
                notrun: tests.filter(t => t.status === 3).length,
                failures: tests.filter(t => t.status !== 0).map(t => ({
                    name: t.name,
                    status: t.status,
                    message: t.message
                }))
            };
        });
        window.dispatchEvent(new Event('load'));
        "#,
    )
    .unwrap();
}

fn wait_wpt(view: &mut View) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        view.poll();
        let report = view.evaluate_json("__wptResults").unwrap();
        if report != "null" {
            eprintln!("WPT REPORT: {report}");
            return serde_json::from_str::<serde_json::Value>(&report).unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "WPT test harness timed out waiting for results: {:?}",
            view.notices
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn open_url(server: &Server, host: &str, path: &str) -> (View, Engine) {
    let engine = engine();
    let mut view = engine
        .create_view(Url::parse(&server.url(host, path)).unwrap())
        .unwrap();
    settle(&mut view);
    (view, engine)
}

/// 1. WPT subset: iframe navigation
#[test]
fn wpt_iframe_navigation() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/page-1" => Reply::html("<h1 id='p1'>Page 1</h1>"),
        "/page-2" => Reply::html("<h1 id='p2'>Page 2</h1>"),
        _ => Reply::html("<iframe id='nav-target' src='/page-1'></iframe>"),
    });

    let (mut view, _engine) = open_url(&server, "127.0.0.1", "/");
    setup_wpt(&mut view);

    let url_2 = server.url("127.0.0.1", "/page-2");
    view.evaluate(&format!(
        r#"
        test(() => {{
            const f = document.getElementById('nav-target');
            assert_true(f !== null, 'iframe exists');
            assert_true(f.contentDocument !== null, 'contentDocument loaded initially');
            assert_equals(f.contentDocument.getElementById('p1').textContent, 'Page 1');
        }}, 'iframe initial document loaded');

        test(() => {{
            const f = document.getElementById('nav-target');
            f.src = '{url_2}';
            assert_equals(f.getAttribute('src'), '{url_2}', 'src attribute updated');
        }}, 'iframe src navigation initiated');
        "#
    ))
    .unwrap();
    view.evaluate("done()").unwrap();

    let res = wait_wpt(&mut view);
    assert_eq!(res["status"], 0, "failures: {:?}", res["failures"]);
    assert_eq!(res["total"], 2);
    assert_eq!(res["passed"], 2);
    assert_eq!(res["failed"], 0);
}

/// 2. WPT subset: same-origin policy
#[test]
fn wpt_same_origin_policy() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/same" => Reply::html("<div id='secret'>same-origin-data</div>"),
        "/cross" => Reply::html("<div id='secret'>cross-origin-data</div>"),
        _ => Reply::html("").status("404 Not Found"),
    });

    let engine = engine();
    let port = server.port;
    let parent_html = format!(
        r#"<!doctype html>
        <html><body>
        <iframe id='same-frame' src='http://127.0.0.1:{port}/same'></iframe>
        <iframe id='cross-frame' src='http://localhost:{port}/cross'></iframe>
        <iframe id='sandbox-frame' sandbox='allow-scripts' srcdoc='<div id="secret">opaque</div>'></iframe>
        </body></html>"#
    );

    let mut view = engine
        .create_view(Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap())
        .unwrap();
    view.set_html(&parent_html).unwrap();
    settle(&mut view);
    setup_wpt(&mut view);

    view.evaluate(
        r#"
        test(() => {
            const same = document.getElementById('same-frame');
            assert_true(same.contentDocument !== null, 'same-origin contentDocument accessible');
            assert_true(same.contentWindow !== null, 'same-origin contentWindow accessible');
            assert_equals(same.contentDocument, same.contentWindow.document, 'contentDocument matches contentWindow.document');
            assert_equals(same.contentDocument.getElementById('secret').textContent, 'same-origin-data');
        }, 'same-origin iframe synchronous DOM access granted');

        test(() => {
            const cross = document.getElementById('cross-frame');
            assert_equals(cross.contentDocument, null, 'cross-origin contentDocument is null');
            assert_throws_dom('SecurityError', () => {
                cross.contentWindow.document;
            }, 'cross-origin contentWindow.document throws SecurityError');
        }, 'cross-origin iframe synchronous DOM access denied');

        test(() => {
            const sandboxed = document.getElementById('sandbox-frame');
            assert_equals(sandboxed.contentDocument, null, 'sandboxed opaque contentDocument is null');
            assert_throws_dom('SecurityError', () => {
                sandboxed.contentWindow.document;
            }, 'sandboxed opaque contentWindow.document throws SecurityError');
        }, 'sandboxed opaque-origin iframe DOM access denied');
        "#,
    )
    .unwrap();
    view.evaluate("done()").unwrap();

    let res = wait_wpt(&mut view);
    assert_eq!(res["status"], 0, "failures: {:?}", res["failures"]);
    assert_eq!(res["total"], 3);
    assert_eq!(res["passed"], 3);
    assert_eq!(res["failed"], 0);
}

/// 3. WPT subset: window relationships
#[test]
fn wpt_window_relationships() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/f1" => Reply::html("<p>frame 1</p>"),
        "/f2" => Reply::html("<p>frame 2</p>"),
        _ => Reply::html("<iframe id='f1' src='/f1'></iframe><iframe id='f2' src='/f2'></iframe>"),
    });

    let (mut view, _engine) = open_url(&server, "127.0.0.1", "/");
    setup_wpt(&mut view);

    // Test parent-side properties
    view.evaluate(
        r#"
        test(() => {
            assert_equals(window.parent, window, 'top window parent is self');
            assert_equals(window.top, window, 'top window top is self');
            assert_equals(window.length, 2, 'window.length matches frame count');
        }, 'top window relationship properties');

        done();
        "#,
    )
    .unwrap();

    let res = wait_wpt(&mut view);
    assert_eq!(res["status"], 0, "failures: {:?}", res["failures"]);
    assert_eq!(res["total"], 1);
    assert_eq!(res["passed"], 1);
    assert_eq!(res["failed"], 0);

    // Test child-side window.frameElement
    let key = view.child_frames()[0].0;
    let child = view.child_by_key(key).unwrap();
    assert_eq!(
        child
            .evaluate_json(
                r#"(() => {
                const el = window.frameElement;
                return el !== null && el.id === 'f1' && el.tagName === 'IFRAME';
            })()"#
            )
            .unwrap(),
        "true"
    );
}

/// 4. WPT subset: postMessage
#[test]
fn wpt_postmessage() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/child" => Reply::html(
            r#"
            <script>
            window.addEventListener('message', (e) => {
                window.parent.postMessage({ echo: e.data, receivedOrigin: e.origin }, '*');
            });
            </script>
            "#,
        ),
        _ => Reply::html("<iframe id='child' src='/child'></iframe>"),
    });

    let (mut view, _engine) = open_url(&server, "127.0.0.1", "/");
    setup_wpt(&mut view);

    view.evaluate(
        r#"
        test(() => {
            const f = document.getElementById('child');
            assert_true(typeof f.contentWindow.postMessage === 'function', 'postMessage exists on contentWindow');
            f.contentWindow.postMessage({ msg: 'hello from parent' }, '*');
        }, 'postMessage can be dispatched to child frame');

        test(() => {
            const f = document.getElementById('child');
            assert_throws_dom('SyntaxError', () => {
                f.contentWindow.postMessage('test', 'invalid-target-origin');
            }, 'invalid targetOrigin throws SyntaxError');
        }, 'postMessage targetOrigin validation');
        "#,
    )
    .unwrap();
    view.evaluate("done()").unwrap();

    let res = wait_wpt(&mut view);
    assert_eq!(res["status"], 0, "failures: {:?}", res["failures"]);
    assert_eq!(res["total"], 2);
    assert_eq!(res["passed"], 2);
    assert_eq!(res["failed"], 0);
}

/// 5. WPT subset: sandbox
#[test]
fn wpt_sandbox() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/sandboxed" => Reply::html("<p>Sandboxed content</p>"),
        _ => Reply::html(
            r#"
            <iframe id='s1' sandbox='allow-scripts' src='/sandboxed'></iframe>
            <iframe id='s2' sandbox='' src='/sandboxed'></iframe>
            "#,
        ),
    });

    let (mut view, _engine) = open_url(&server, "127.0.0.1", "/");
    setup_wpt(&mut view);

    view.evaluate(
        r#"
        test(() => {
            const s1 = document.getElementById('s1');
            assert_true(s1.sandbox !== undefined, 'sandbox DOMTokenList exists');
            assert_true(s1.sandbox.contains('allow-scripts'), 'sandbox token list contains allow-scripts');
            assert_false(s1.sandbox.contains('allow-same-origin'), 'sandbox token list does not contain allow-same-origin');
        }, 'sandbox attribute DOMTokenList reflection');

        test(() => {
            const s1 = document.getElementById('s1');
            const s2 = document.getElementById('s2');
            assert_equals(s1.contentDocument, null, 'sandbox without allow-same-origin has opaque origin');
            assert_equals(s2.contentDocument, null, 'empty sandbox has opaque origin');
        }, 'sandbox without allow-same-origin enforces null contentDocument');
        "#,
    )
    .unwrap();
    view.evaluate("done()").unwrap();

    let res = wait_wpt(&mut view);
    assert_eq!(res["status"], 0, "failures: {:?}", res["failures"]);
    assert_eq!(res["total"], 2);
    assert_eq!(res["passed"], 2);
    assert_eq!(res["failed"], 0);
}

/// 6. WPT subset: nested browsing contexts
#[test]
fn wpt_nested_browsing_contexts() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/deep" => Reply::html("<p id='deep-leaf'>leaf node</p>"),
        "/mid" => Reply::html("<iframe id='inner' src='/deep'></iframe>"),
        _ => Reply::html("<iframe id='outer' src='/mid'></iframe>"),
    });

    let (mut view, _engine) = open_url(&server, "127.0.0.1", "/");
    setup_wpt(&mut view);

    view.evaluate(
        r#"
        test(() => {
            const outer = document.getElementById('outer');
            assert_true(outer !== null, 'outer frame exists');
            assert_true(outer.contentDocument !== null, 'outer frame contentDocument exists');
            const inner = outer.contentDocument.getElementById('inner');
            assert_true(inner !== null, 'inner frame element exists inside outer document');
            assert_equals(inner.tagName.toLowerCase(), 'iframe', 'inner element is iframe');
        }, 'nested frame hierarchy structure accessible same-origin across multiple depth levels');
        "#,
    )
    .unwrap();
    view.evaluate("done()").unwrap();

    let res = wait_wpt(&mut view);
    assert_eq!(res["status"], 0, "failures: {:?}", res["failures"]);
    assert_eq!(res["total"], 1);
    assert_eq!(res["passed"], 1);
    assert_eq!(res["failed"], 0);
}

/// 7. WPT subset: frame loading and events
#[test]
fn wpt_frame_loading_and_events() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/loaded" => Reply::html("<p>loaded frame content</p>"),
        _ => Reply::html("<iframe id='target' src='/loaded'></iframe>"),
    });

    let (mut view, _engine) = open_url(&server, "127.0.0.1", "/");
    setup_wpt(&mut view);

    view.evaluate(
        r#"
        test(() => {
            const target = document.getElementById('target');
            assert_true(target !== null, 'parser-created frame element exists in document');
            assert_true(target.isConnected, 'frame element is connected');
            assert_equals(target.tagName.toLowerCase(), 'iframe', 'element tag is iframe');
            assert_true(target.contentDocument !== null, 'parser-created frame contentDocument loaded');
        }, 'parser-created frame element lifecycle and contentDocument loading');

        test(() => {
            const dynamic = document.createElement('iframe');
            dynamic.id = 'dyn';
            dynamic.src = 'about:blank';
            document.body.appendChild(dynamic);
            assert_true(dynamic.isConnected, 'dynamic frame is connected to DOM tree');
        }, 'script-created dynamic frame element creation and attachment');
        "#,
    )
    .unwrap();
    view.evaluate("done()").unwrap();

    let res = wait_wpt(&mut view);
    assert_eq!(res["status"], 0, "failures: {:?}", res["failures"]);
    assert_eq!(res["total"], 2);
    assert_eq!(res["passed"], 2);
    assert_eq!(res["failed"], 0);
}
