use std::sync::Arc;
use wega_engine::{Engine, Limits, NetworkPolicy, Url};

fn page(limits: Limits) -> wega_engine::View {
    Engine::new(limits, NetworkPolicy::default(), Arc::new(|| {}))
        .unwrap()
        .create_view(Url::parse("https://example.test/index.html").unwrap())
        .unwrap()
}

#[test]
fn same_origin_iframe_dom_and_script_access() {
    let mut view = page(Limits::default());
    view.set_html(r#"<!doctype html>
      <title>Parent</title>
      <iframe id="child" src="about:blank"></iframe>
    "#)
    .unwrap();

    // Verify contentDocument is non-null for same-origin
    let doc_check = view
        .evaluate_json("document.getElementById('child').contentDocument !== null")
        .unwrap();
    assert_eq!(doc_check, "true");

    // Verify contentWindow.document === contentDocument
    let eq_check = view
        .evaluate_json(
            "document.getElementById('child').contentWindow.document === document.getElementById('child').contentDocument",
        )
        .unwrap();
    assert_eq!(eq_check, "true");

    // Parent can synchronously manipulate iframe DOM
    view.evaluate(
        "document.getElementById('child').contentDocument.body.innerHTML = '<h1 id=\"heading\">Frame Title</h1>'",
    )
    .unwrap();
    let text = view
        .evaluate_json(
            "document.getElementById('child').contentDocument.getElementById('heading').textContent",
        )
        .unwrap();
    assert_eq!(text, "\"Frame Title\"");

    // Verify frame indexing on window
    let len = view.evaluate_json("window.length").unwrap();
    assert_eq!(len, "1");

    let win0 = view
        .evaluate_json("window[0] === document.getElementById('child').contentWindow")
        .unwrap();
    assert_eq!(win0, "true");

    // Verify parent/top relationships
    let parent_ref = view
        .evaluate_json("document.getElementById('child').contentWindow.parent === window")
        .unwrap();
    assert_eq!(parent_ref, "true");

    let top_ref = view
        .evaluate_json("document.getElementById('child').contentWindow.top === window")
        .unwrap();
    assert_eq!(top_ref, "true");
}

#[test]
fn cross_origin_iframe_security_isolation() {
    let mut view = page(Limits::default());
    // Parent origin is https://example.test, frame is https://foreign.test
    view.set_html(r#"<!doctype html>
      <title>Parent</title>
      <iframe id="cross" src="https://foreign.test/embed.html"></iframe>
    "#)
    .unwrap();

    // Cross-origin contentDocument must be null
    let doc_null = view
        .evaluate_json("document.getElementById('cross').contentDocument === null")
        .unwrap();
    assert_eq!(doc_null, "true");

    // Reading document property from cross-origin contentWindow must throw SecurityError
    let threw_doc = view
        .evaluate_json(r#"(() => {
            try {
                const d = document.getElementById('cross').contentWindow.document;
                return false;
            } catch (e) {
                return e.name === "SecurityError";
            }
        })()"#)
        .unwrap();
    assert_eq!(threw_doc, "true");

    // Reading custom expando or eval on cross-origin contentWindow must throw SecurityError
    let threw_eval = view
        .evaluate_json(r#"(() => {
            try {
                document.getElementById('cross').contentWindow.eval("alert(1)");
                return false;
            } catch (e) {
                return e.name === "SecurityError";
            }
        })()"#)
        .unwrap();
    assert_eq!(threw_eval, "true");

    // Allowed cross-origin properties work without throwing
    let allowed_checks = view
        .evaluate_json(r#"(() => {
            const win = document.getElementById('cross').contentWindow;
            return typeof win.postMessage === "function" &&
                   win.closed === false &&
                   win.parent === window &&
                   win.top === window;
        })()"#)
        .unwrap();
    assert_eq!(allowed_checks, "true");

    // Setting location.href is permitted for cross-origin navigation, but reading throws SecurityError
    let threw_read_loc = view
        .evaluate_json(r#"(() => {
            try {
                const href = document.getElementById('cross').contentWindow.location.href;
                return false;
            } catch (e) {
                return e.name === "SecurityError";
            }
        })()"#)
        .unwrap();
    assert_eq!(threw_read_loc, "true");
}

#[test]
fn sandboxed_iframe_opaque_origin_isolation() {
    let mut view = page(Limits::default());
    // Sandboxed without allow-same-origin forces unique opaque origin "null"
    view.set_html(r#"<!doctype html>
      <iframe id="sandboxed" sandbox="allow-scripts" src="about:blank"></iframe>
      <iframe id="sandboxed_same" sandbox="allow-scripts allow-same-origin" src="about:blank"></iframe>
    "#)
    .unwrap();

    // Sandboxed frame without allow-same-origin has contentDocument === null
    let sandboxed_doc_null = view
        .evaluate_json("document.getElementById('sandboxed').contentDocument === null")
        .unwrap();
    assert_eq!(sandboxed_doc_null, "true");

    // Accessing sandboxed contentWindow.document throws SecurityError
    let sandboxed_threw = view
        .evaluate_json(r#"(() => {
            try {
                const d = document.getElementById('sandboxed').contentWindow.document;
                return false;
            } catch (e) {
                return e.name === "SecurityError";
            }
        })()"#)
        .unwrap();
    assert_eq!(sandboxed_threw, "true");

    // Sandboxed frame WITH allow-same-origin permits contentDocument access
    let same_origin_allowed = view
        .evaluate_json("document.getElementById('sandboxed_same').contentDocument !== null")
        .unwrap();
    assert_eq!(same_origin_allowed, "true");

    // TokenList manipulation on sandbox
    let token_test = view
        .evaluate_json(r#"(() => {
            const frame = document.getElementById('sandboxed');
            const hasScripts = frame.sandbox.contains('allow-scripts');
            const hasForms = frame.sandbox.contains('allow-forms');
            frame.sandbox.add('allow-forms');
            return hasScripts && !hasForms && frame.sandbox.contains('allow-forms');
        })()"#)
        .unwrap();
    assert_eq!(token_test, "true");
}

#[test]
fn post_message_cross_frame_communication_and_target_origin_filtering() {
    let mut view = page(Limits::default());
    view.set_html(r#"<!doctype html>
      <iframe id="child" src="about:blank"></iframe>
    "#)
    .unwrap();

    // Register message receiver on child frame
    view.evaluate(r#"
        window.receivedMessages = [];
        document.getElementById('child').contentWindow.addEventListener('message', (event) => {
            window.receivedMessages.push({
                data: event.data,
                origin: event.origin,
            });
        });
    "#)
    .unwrap();

    // Post message with wildcard target origin "*"
    view.evaluate("document.getElementById('child').contentWindow.postMessage('hello from parent', '*')")
        .unwrap();

    // Post message with matching target origin
    view.evaluate("document.getElementById('child').contentWindow.postMessage('matching message', 'https://example.test')")
        .unwrap();

    // Post message with mismatched target origin (must be dropped and not delivered)
    view.evaluate("document.getElementById('child').contentWindow.postMessage('secret dropped', 'https://mismatched.test')")
        .unwrap();

    // Verify messages received
    let result = view
        .evaluate_json(r#"(() => {
            const msgs = window.receivedMessages;
            if (msgs.length !== 2) return false;
            return msgs[0].data === "hello from parent" &&
                   msgs[0].origin === "https://example.test" &&
                   msgs[1].data === "matching message";
        })()"#)
        .unwrap();
    assert_eq!(result, "true");

    // Invalid target origin throws SyntaxError per HTML spec
    let syntax_err = view
        .evaluate_json(r#"(() => {
            try {
                document.getElementById('child').contentWindow.postMessage('test', 'not a valid uri');
                return false;
            } catch (e) {
                return e.name === "SyntaxError";
            }
        })()"#)
        .unwrap();
    assert_eq!(syntax_err, "true");
}

#[test]
fn srcdoc_iframe_inherits_parent_origin_unless_sandboxed() {
    let mut view = page(Limits::default());
    view.set_html(r#"<!doctype html>
      <iframe id="srcdoc_frame" srcdoc="<h1>Embedded Content</h1>"></iframe>
      <iframe id="srcdoc_sandboxed" sandbox="allow-scripts" srcdoc="<h1>Sandboxed Content</h1>"></iframe>
    "#)
    .unwrap();

    // Non-sandboxed srcdoc inherits parent origin and has accessible contentDocument
    let doc_accessible = view
        .evaluate_json(r#"(() => {
            const frame = document.getElementById('srcdoc_frame');
            return frame.contentDocument !== null &&
                   frame.contentDocument.body.querySelector('h1').textContent === "Embedded Content";
        })()"#)
        .unwrap();
    assert_eq!(doc_accessible, "true");

    // Sandboxed srcdoc without allow-same-origin gets opaque "null" origin
    let sandboxed_inaccessible = view
        .evaluate_json("document.getElementById('srcdoc_sandboxed').contentDocument === null")
        .unwrap();
    assert_eq!(sandboxed_inaccessible, "true");
}

#[test]
fn iframe_detachment_cleans_up_frame_and_marks_window_closed() {
    let mut view = page(Limits::default());
    view.set_html(r#"<!doctype html>
      <div id="container">
        <iframe id="temp_frame" src="about:blank"></iframe>
      </div>
    "#)
    .unwrap();

    let initial_len = view.evaluate_json("window.length").unwrap();
    assert_eq!(initial_len, "1");

    let is_open = view
        .evaluate_json("document.getElementById('temp_frame').contentWindow.closed")
        .unwrap();
    assert_eq!(is_open, "false");

    // Remove frame from DOM
    view.evaluate(r#"
        const frame = document.getElementById('temp_frame');
        window.detachedWindow = frame.contentWindow;
        frame.remove();
    "#)
    .unwrap();

    let after_len = view.evaluate_json("window.length").unwrap();
    assert_eq!(after_len, "0");

    let is_closed = view
        .evaluate_json("window.detachedWindow.closed")
        .unwrap();
    assert_eq!(is_closed, "true");
}

