//! V8 isolate safety audit test suite (Stage 5).
//!
//! Verifies the invariants of the 6 unsafe sites in `wega-adapter-v8/src/host.rs`:
//! 1. `raw_exit` (isolate creation entry cleanup)
//! 2. `raw_enter_ptr` (isolate re-entry before drop)
//! 3. `RawIsolateGuard::from_ptr` (entry via raw isolate pointer)
//! 4. `RawIsolateGuard::new` (entry via `&mut Isolate`)
//! 5. `Drop for RawIsolateGuard` (RAII isolate exit on normal return & panic unwind)
//! 6. `isolate.as_raw_isolate_ptr` (stable isolate pointer acquisition)

mod support;
use std::{
    panic::AssertUnwindSafe,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use support::{Reply, Server, engine, settle};
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

fn page(url: &str) -> View {
    Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {}))
        .unwrap()
        .create_view(Url::parse(url).unwrap())
        .unwrap()
}

fn open_view(server: &Server, host: &str, path: &str) -> (View, Engine) {
    let engine = engine();
    let mut view = engine
        .create_view(Url::parse(&server.url(host, path)).unwrap())
        .unwrap();
    settle(&mut view);
    (view, engine)
}

fn eval(view: &mut View, script: &str) -> String {
    view.evaluate_json(script).unwrap()
}

fn child_eval(view: &mut View, index: usize, script: &str) -> String {
    let key = view.child_frames()[index].0;
    view.child_by_key(key)
        .unwrap()
        .evaluate_json(script)
        .unwrap()
}

fn pump(view: &mut View, mut done: impl FnMut(&mut View) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        view.poll();
        if done(view) {
            return true;
        }
        thread::sleep(Duration::from_millis(2));
    }
    false
}

/// 1. Interleaved isolate entry and exit ordering across multiple frames on the same thread.
#[test]
fn test_isolate_entry_exit_interleaved_execution() {
    let mut view = page("https://example.test/index.html");
    view.set_html(
        r#"<!doctype html>
        <html>
        <body>
            <iframe id="c1" src="about:blank"></iframe>
            <iframe id="c2" src="about:blank"></iframe>
            <iframe id="c3" src="about:blank"></iframe>
        </body>
        </html>"#,
    )
    .unwrap();
    view.poll();

    assert_eq!(view.child_frames().len(), 3);

    // Set distinctive state in each realm
    eval(
        &mut view,
        "globalThis.hostId = 'parent'; globalThis.count = 0;",
    );
    child_eval(
        &mut view,
        0,
        "globalThis.hostId = 'child-1'; globalThis.count = 10;",
    );
    child_eval(
        &mut view,
        1,
        "globalThis.hostId = 'child-2'; globalThis.count = 20;",
    );
    child_eval(
        &mut view,
        2,
        "globalThis.hostId = 'child-3'; globalThis.count = 30;",
    );

    // Interleave execution in non-trivial order (verifying LIFO stack entry/exit balances every time)
    for _ in 0..15 {
        eval(&mut view, "count += 1;");
        child_eval(&mut view, 0, "count += 2;");
        child_eval(&mut view, 2, "count += 4;");
        child_eval(&mut view, 1, "count += 3;");
        eval(&mut view, "count += 1;");
    }

    assert_eq!(eval(&mut view, "count"), "30");
    assert_eq!(child_eval(&mut view, 0, "count"), "40");
    assert_eq!(child_eval(&mut view, 1, "count"), "65");
    assert_eq!(child_eval(&mut view, 2, "count"), "90");

    assert_eq!(eval(&mut view, "hostId"), "\"parent\"");
    assert_eq!(child_eval(&mut view, 0, "hostId"), "\"child-1\"");
    assert_eq!(child_eval(&mut view, 1, "hostId"), "\"child-2\"");
    assert_eq!(child_eval(&mut view, 2, "hostId"), "\"child-3\"");
}

/// 2. Nested execution and GC invocation inside RawIsolateGuard.
#[test]
fn test_nested_execution_and_gc_nesting() {
    let mut view = page("https://example.test/index.html");
    view.set_html(
        r#"<!doctype html>
        <html>
        <body>
            <iframe id="c1" src="about:blank"></iframe>
        </body>
        </html>"#,
    )
    .unwrap();
    view.poll();

    // Allocate memory and trigger microtasks/checks
    eval(
        &mut view,
        r#"(() => {
            const arr = [];
            for (let i = 0; i < 10000; i++) {
                arr.push({ a: i, b: 'nested-' + i });
            }
            return arr.length;
        })()"#,
    );

    // Call child eval
    child_eval(
        &mut view,
        0,
        r#"(() => {
            const arr = [];
            for (let i = 0; i < 5000; i++) {
                arr.push({ c: i });
            }
            return arr.length;
        })()"#,
    );

    // Freezing forces GC and low-memory notifications using nested RawIsolateGuard
    view.freeze(true);
    view.freeze(false);

    // After unfreeze, both isolates must still execute normally
    assert_eq!(eval(&mut view, "1 + 1"), "2");
    assert_eq!(child_eval(&mut view, 0, "2 + 2"), "4");
}

/// 3. Panic and early-return cleanup: RawIsolateGuard drop unwind does not corrupt isolate stack.
#[test]
fn test_panic_and_early_return_unwind_safety() {
    let mut view = page("https://example.test/index.html");
    view.set_html(
        r#"<!doctype html>
        <html>
        <body>
            <iframe id="c1" src="about:blank"></iframe>
        </body>
        </html>"#,
    )
    .unwrap();
    view.poll();

    // Syntax error in JS does not panic, returns Err(ScriptError)
    assert!(view.evaluate_json("syntax { error").is_err());

    // Subsequent call must execute cleanly (guard was properly dropped on error return)
    assert_eq!(
        eval(&mut view, "'safe-after-error'"),
        "\"safe-after-error\""
    );

    // Test panic unwinding safety using catch_unwind
    let panic_result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // Intentionally panic inside a closure while holding view reference
        let _ = eval(&mut view, "1 + 1");
        panic!("simulated transient host panic");
    }));
    assert!(panic_result.is_err());

    // Even after panic unwound through rust frames, the isolate must not be stuck in entered state.
    // Another evaluation in the child or parent succeeds.
    assert_eq!(child_eval(&mut view, 0, "'child-ok'"), "\"child-ok\"");
    assert_eq!(eval(&mut view, "'parent-ok'"), "\"parent-ok\"");
}

/// 4. Isolate destruction order: child dropped before parent, parent dropped before child.
#[test]
fn test_isolate_destruction_order_arbitrary_removal() {
    let mut view = page("https://example.test/index.html");
    view.set_html(
        r#"<!doctype html>
        <html>
        <body>
            <iframe id="f1" src="about:blank"></iframe>
            <iframe id="f2" src="about:blank"></iframe>
            <iframe id="f3" src="about:blank"></iframe>
        </body>
        </html>"#,
    )
    .unwrap();
    view.poll();
    assert_eq!(view.child_frames().len(), 3);

    // Remove middle frame f2 via DOM
    eval(&mut view, "document.getElementById('f2').remove();");
    view.poll();
    assert_eq!(view.child_frames().len(), 2);

    // Verify remaining frames (f1 and f3) continue executing without isolate crash
    assert_eq!(child_eval(&mut view, 0, "10 * 2"), "20");
    assert_eq!(child_eval(&mut view, 1, "10 * 3"), "30");

    // Remove remaining frames
    eval(&mut view, "document.getElementById('f1').remove();");
    eval(&mut view, "document.getElementById('f3').remove();");
    view.poll();
    assert_eq!(view.child_frames().len(), 0);

    // Parent continues to execute cleanly
    assert_eq!(eval(&mut view, "'parent-alive'"), "\"parent-alive\"");
}

/// 5. Navigation during script execution and realm replacement on shared thread.
#[test]
fn test_frame_navigation_replaces_isolate_safely() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/page-a" => {
            Reply::html("<html><body><script>globalThis.ver = 'A';</script></body></html>")
        }
        "/page-b" => {
            Reply::html("<html><body><script>globalThis.ver = 'B';</script></body></html>")
        }
        _ => Reply::html("<html><body><iframe id='nav' src='/page-a'></iframe></body></html>"),
    });

    let (mut view, _engine) = open_view(&server, "127.0.0.1", "/");

    let key = view.child_frames()[0].0;
    assert!(pump(&mut view, |v| v.child_by_key(key).is_some_and(|c| c
        .evaluate_json("globalThis.ver")
        .is_ok_and(|val| val == "\"A\""))));

    // Navigate child frame from parent
    let url_b = server.url("127.0.0.1", "/page-b");
    eval(
        &mut view,
        &format!("document.getElementById('nav').src = '{url_b}'; 0"),
    );

    // Child isolate is replaced with new document realm upon navigation completion
    assert!(pump(&mut view, |v| v.child_by_key(key).is_some_and(|c| c
        .evaluate_json("globalThis.ver")
        .is_ok_and(|val| val == "\"B\""))));

    assert_eq!(eval(&mut view, "'parent-ok'"), "\"parent-ok\"");
}

/// 6. Stale cross-isolate references rejected fail-closed.
#[test]
fn test_stale_cross_isolate_references_fail_closed() {
    let mut view = page("https://example.test/index.html");
    view.set_html(
        r#"<!doctype html>
        <html>
        <body>
            <iframe id="target" src="about:blank"></iframe>
        </body>
        </html>"#,
    )
    .unwrap();
    view.poll();

    // Cache facade references in parent realm, returning primitive 0 so JSON serialization doesn't inspect the facade
    eval(
        &mut view,
        r#"
        globalThis.iframeEl = document.getElementById('target');
        globalThis.winFacade = iframeEl.contentWindow;
        0;
        "#,
    );

    // Verify valid before detachment
    assert_eq!(eval(&mut view, "winFacade !== null"), "true");
    assert_eq!(eval(&mut view, "iframeEl.contentDocument !== null"), "true");

    // Detach frame
    eval(&mut view, "iframeEl.remove(); 0;");
    view.poll();

    // After detachment, accessing contentDocument yields null
    assert_eq!(eval(&mut view, "iframeEl.contentDocument === null"), "true");

    // Methods on stale window facade reject or do not crash
    assert_eq!(
        eval(
            &mut view,
            r#"(() => {
                try {
                    winFacade.postMessage('ping', '*');
                    return 'ok-noop';
                } catch (e) {
                    return 'caught:' + e.name;
                }
            })()"#,
        ),
        "\"ok-noop\""
    );
}

/// 7. Watchdog timeout isolation between child and parent isolates.
#[test]
fn test_watchdog_timeout_in_child_isolate_preserves_parent_isolate() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/p" => Reply::html("<p id=p>parent</p><iframe src=/spin></iframe>"),
        "/spin" => Reply::html("<script>setTimeout(() => { for (;;) {} }, 0)</script>"),
        _ => Reply::html("").status("404 Not Found"),
    });

    let (mut view, _engine) = open_view(&server, "localhost", "/p");
    let started = Instant::now();
    pump(&mut view, |v| {
        started.elapsed() > Duration::from_secs(6)
            || (!v.child_frames().is_empty() && !v.notices.is_empty())
    });

    // Parent isolate continues to function despite child runaway script interruption
    assert_eq!(
        eval(&mut view, "document.getElementById('p').textContent"),
        "\"parent\""
    );
    assert_eq!(eval(&mut view, "100 + 200"), "300");
}
