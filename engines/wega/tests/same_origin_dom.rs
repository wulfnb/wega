//! Same-origin DOM access and cross-origin security boundaries.
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

fn page(url: &str) -> View {
    Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {}))
        .unwrap()
        .create_view(Url::parse(url).unwrap())
        .unwrap()
}

fn eval(view: &mut View, script: &str) -> String {
    view.evaluate_json(script).unwrap()
}

/// Evaluate in a child frame, polling while that frame is still mid-navigation.
/// A single parent `poll()` is not always enough on slower runners.
fn child_eval(view: &mut View, index: usize, script: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let key = view.child_frames()[index].0;
        match view.child_by_key(key).unwrap().evaluate_json(script) {
            Ok(value) => return value,
            Err(error) if error == "Document is suspended for navigation" => {
                assert!(
                    Instant::now() < deadline,
                    "child frame {index} never finished navigating"
                );
                view.poll();
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("child frame {index} evaluate failed: {error}"),
        }
    }
}

#[test]
fn same_origin_dom_access_content_document_and_window() {
    let mut view = page("https://example.test/app");
    view.set_html(
        r#"<!doctype html>
        <html>
        <body>
            <iframe id="same" src="about:blank"></iframe>
            <iframe id="cross" src="https://foreign.test/embed"></iframe>
            <iframe id="sandboxed" sandbox="allow-scripts" srcdoc="<p>Opaque</p>"></iframe>
        </body>
        </html>"#,
    )
    .unwrap();
    view.poll();

    // 1. Same-origin iframe.contentDocument and iframe.contentWindow.document
    assert_eq!(
        eval(
            &mut view,
            r#"(() => {
                const f = document.getElementById('same');
                return f.contentDocument !== null &&
                       f.contentWindow !== null &&
                       f.contentDocument === f.contentWindow.document &&
                       f.contentDocument.nodeType === 9;
            })()"#
        ),
        "true"
    );

    // 2. Cross-origin iframe: contentDocument is null, contentWindow.document throws SecurityError
    assert_eq!(
        eval(
            &mut view,
            r#"(() => {
                const f = document.getElementById('cross');
                let threw = false;
                try { f.contentWindow.document; } catch (e) { threw = e.name === 'SecurityError'; }
                return f.contentDocument === null && threw;
            })()"#
        ),
        "true"
    );

    // 3. Sandboxed opaque iframe: contentDocument is null, contentWindow.document throws SecurityError
    assert_eq!(
        eval(
            &mut view,
            r#"(() => {
                const f = document.getElementById('sandboxed');
                let threw = false;
                try { f.contentWindow.document; } catch (e) { threw = e.name === 'SecurityError'; }
                return f.contentDocument === null && threw;
            })()"#
        ),
        "true"
    );

    // 4. Parent reads and mutates same-origin child DOM
    assert_eq!(
        eval(
            &mut view,
            r#"(() => {
                const doc = document.getElementById('same').contentDocument;
                const p = doc.createElement('p');
                p.textContent = 'Written by parent';
                p.setAttribute('id', 'child-p');
                doc.body.appendChild(p);
                return doc.getElementById('child-p').textContent;
            })()"#
        ),
        "\"Written by parent\""
    );

    // 5. Child can read its own mutated DOM
    assert_eq!(
        child_eval(
            &mut view,
            0,
            "document.getElementById('child-p').textContent"
        ),
        "\"Written by parent\""
    );

    // 6. Child access to window.frameElement and window.parent.document
    assert_eq!(
        child_eval(
            &mut view,
            0,
            r#"(() => {
                const fe = window.frameElement;
                if (!fe) return 'no-frameElement';
                const parentDoc = window.parent.document;
                if (!parentDoc) return 'no-parentDoc';
                const heading = parentDoc.createElement('h1');
                heading.id = 'from-child';
                heading.textContent = 'Written by child';
                parentDoc.body.appendChild(heading);
                return fe.id;
            })()"#
        ),
        "\"same\""
    );

    // 7. Parent sees the heading inserted by child
    assert_eq!(
        eval(
            &mut view,
            "document.getElementById('from-child').textContent"
        ),
        "\"Written by child\""
    );

    // 8. Top-level window.frameElement is null
    assert_eq!(eval(&mut view, "window.frameElement === null"), "true");

    // 9. Cross-origin child window.frameElement is null
    assert_eq!(
        child_eval(&mut view, 1, "window.frameElement === null"),
        "true"
    );

    // 10. Sandboxed opaque child window.frameElement is null
    assert_eq!(
        child_eval(&mut view, 2, "window.frameElement === null"),
        "true"
    );
}

#[test]
fn same_origin_dom_navigation_and_stale_reference_safety() {
    let mut view = page("https://example.test/main");
    view.set_html(
        r#"<!doctype html>
        <html>
        <body>
            <iframe id="f" src="about:blank"></iframe>
        </body>
        </html>"#,
    )
    .unwrap();
    view.poll();

    // Grab a reference to a node in the initial document
    eval(
        &mut view,
        r#"
        globalThis.oldDoc = document.getElementById('f').contentDocument;
        globalThis.oldBody = oldDoc.body;
        globalThis.oldBody.textContent = 'Initial';
        0
        "#,
    );

    assert_eq!(
        eval(&mut view, "globalThis.oldBody.textContent"),
        "\"Initial\""
    );

    // Navigate child iframe to another same-origin page via srcdoc
    eval(
        &mut view,
        r#"
        document.getElementById('f').srcdoc = '<h1>Navigated</h1>';
        0
        "#,
    );
    view.poll();

    // Check that new contentDocument has the new content
    assert_eq!(
        eval(
            &mut view,
            "document.getElementById('f').contentDocument.querySelector('h1').textContent"
        ),
        "\"Navigated\""
    );

    // Accessing stale node from the old generation fails safely (fails or returns null/throws)
    let stale_attempt = eval(
        &mut view,
        r#"(() => {
            try {
                globalThis.oldBody.textContent = 'Zombie';
                return 'mutated';
            } catch (e) {
                return e.name;
            }
        })()"#,
    );
    // Either InvalidStateError or cannot mutate detached old node
    assert_ne!(stale_attempt, "\"mutated\"");
}
