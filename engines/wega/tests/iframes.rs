//! Iframe browsing contexts: nested views with their own documents, realms and principals.
//! Every fixture is a deterministic loopback HTTP server; hosts `localhost` and `127.0.0.1`
//! are different origins on the same port.
mod support;
use std::{thread, time::{Duration, Instant}};
use support::{Reply, Server, engine, settle};
use wega_engine::{Url, View};

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
fn child_eval(view: &mut View, index: usize, script: &str) -> String {
    let key = view.child_frames()[index].0;
    view.child_by_key(key).unwrap().evaluate_json(script).unwrap()
}
fn loaded(view: &mut View, count: usize) {
    assert!(
        pump(view, |v| v.child_frames().len() == count
            && (0..count).all(|i| {
                let key = v.child_frames()[i].0;
                let c = v.child_by_key(key).unwrap();
                !c.loading
            })),
        "frames did not load: {:?}",
        view.notices
    );
}
fn open(server: &Server, host: &str, path: &str) -> (View, wega_engine::Engine) {
    let engine = engine();
    let mut view = engine.create_view(Url::parse(&server.url(host, path)).unwrap()).unwrap();
    settle(&mut view);
    (view, engine)
}

#[test]
fn parser_created_same_origin_iframe_loads_with_its_own_document_and_realm() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/parent" => Reply::html("<title>p</title><p id=parent>parent</p><iframe id=f src=/child></iframe>"),
        "/child" => Reply::html("<title>c</title><p id=who>child</p><script>globalThis.mark='child';</script>"),
        _ => Reply::html("").status("404 Not Found"),
    });
    let (mut view, _engine) = open(&server, "localhost", "/parent");
    loaded(&mut view, 1);
    assert_eq!(child_eval(&mut view, 0, "document.getElementById('who').textContent"), "\"child\"");
    assert_eq!(child_eval(&mut view, 0, "globalThis.mark"), "\"child\"");
    assert_eq!(view.evaluate_json("typeof globalThis.mark").unwrap(), "\"undefined\"");
    assert_eq!(view.evaluate_json("document.getElementById('who')").unwrap(), "null");
    assert_eq!(view.evaluate_json("window.length").unwrap(), "1");
}

fn eval(view: &mut View, script: &str) -> String {
    view.evaluate_json(script).unwrap()
}
fn wait_for(view: &mut View, script: &str, expected: &str) {
    let ok = pump(view, |v| v.evaluate_json(script).map_or(false, |r| r == expected));
    assert!(ok, "`{script}` never became {expected}: now {:?}; notices {:?}",
        view.evaluate_json(script), view.notices);
}
fn routes(seen: &support::Seen) -> Reply {
    match seen.path.as_str() {
        "/parent" => Reply::html("<p id=p>parent</p><iframe id=f src=/child></iframe>"),
        "/child" => Reply::html("<p id=who>child</p><script>globalThis.mark='c'; addEventListener('message', e => { globalThis.got = {d: e.data, o: e.origin, s: e.source === parent}; parent.postMessage({echo: e.data}, '*'); });</script>"),
        "/deny" => Reply::html("<p>deny</p>").header("X-Frame-Options", "DENY"),
        "/same" => Reply::html("<p>same</p>").header("X-Frame-Options", "SAMEORIGIN"),
        "/anc-none" => Reply::html("<p>x</p>").header("Content-Security-Policy", "frame-ancestors 'none'"),
        "/redir" => Reply::html("").status("302 Found").header("Location", "http://127.0.0.1:__PORT__/child"),
        "/csp-parent" => Reply::html("<p>x</p>").header("Content-Security-Policy", "frame-src 'none'"),
        "/cookie-page" => Reply::html("<p>c</p>").header("Set-Cookie", "sid=top; Path=/"),
        "/echo" => Reply::html(&format!("<p id=ck>{}</p>", seen.header("cookie").unwrap_or("none"))),
        _ => Reply::html("").status("404 Not Found"),
    }
}
fn server() -> Server {
    // The redirect target needs the port: resolve it lazily from the Host header.
    Server::new(|seen| {
        let mut reply = routes(seen);
        if seen.path == "/redir" {
            let host = seen.header("host").unwrap_or("localhost:0");
            let port = host.rsplit(':').next().unwrap_or("0");
            reply.headers.retain(|(n, _)| n != "Location");
            reply = reply.header("Location", &format!("http://127.0.0.1:{port}/child"));
        }
        reply
    })
}

#[test]
fn cross_origin_frame_loads_but_its_window_is_a_restricted_facade() {
    let server = server();
    let (mut view, _e) = open(&server, "localhost", "/blank");
    let url = server.url("127.0.0.1", "/child");
    eval(&mut view, &format!("document.body.innerHTML = '<iframe id=f src=\"{url}\"></iframe>'; 0"));
    loaded(&mut view, 1);
    assert_eq!(child_eval(&mut view, 0, "location.origin"), format!("\"http://127.0.0.1:{}\"", server.port));
    assert_eq!(eval(&mut view, "document.getElementById('f').contentDocument"), "null");
    assert_eq!(eval(&mut view, "(() => { const w = document.getElementById('f').contentWindow;
        try { w.document; return 'leak'; } catch (e) { return e.name; } })()"), "\"SecurityError\"");
    assert_eq!(eval(&mut view, "(() => { const w = document.getElementById('f').contentWindow;
        try { w.eval; return 'leak'; } catch (e) { return e.name; } })()"), "\"SecurityError\"");
    assert_eq!(eval(&mut view, "(() => { const w = document.getElementById('f').contentWindow;
        try { w.localStorage; return 'leak'; } catch (e) { return e.name; } })()"), "\"SecurityError\"");
    assert_eq!(eval(&mut view, "typeof document.getElementById('f').contentWindow.postMessage"), "\"function\"");
}

#[test]
fn post_message_checks_target_origin_and_reports_the_host_computed_origin() {
    let server = server();
    let (mut view, _e) = open(&server, "localhost", "/parent");
    loaded(&mut view, 1);
    eval(&mut view, "globalThis.replies = []; addEventListener('message', e => replies.push({d: e.data, o: e.origin, child: e.source === document.getElementById('f').contentWindow})); 0");
    let origin = format!("http://localhost:{}", server.port);
    // Wrong target origin: never delivered.
    eval(&mut view, "document.getElementById('f').contentWindow.postMessage('nope', 'http://evil.test'); 0");
    eval(&mut view, "document.getElementById('f').contentWindow.postMessage({n: 1}, '*'); 0");
    pump(&mut view, |_| false);
    wait_for(&mut view, "replies.length", "1");
    assert_eq!(eval(&mut view, "JSON.stringify(replies[0])"), format!("\"{{\\\"d\\\":{{\\\"echo\\\":{{\\\"n\\\":1}}}},\\\"o\\\":\\\"{origin}\\\",\\\"child\\\":true}}\""));
    assert_eq!(child_eval(&mut view, 0, "JSON.stringify(got)"), format!("\"{{\\\"d\\\":{{\\\"n\\\":1}},\\\"o\\\":\\\"{origin}\\\",\\\"s\\\":true}}\""));
    // Uncloneable data fails closed.
    assert_eq!(eval(&mut view, "(() => { try { document.getElementById('f').contentWindow.postMessage(() => {}, '*'); } catch (e) { return e.name; } })()"), "\"DataCloneError\"");
    assert_eq!(eval(&mut view, "(() => { try { const b = new ArrayBuffer(1); document.getElementById('f').contentWindow.postMessage(b, '*', [b]); } catch (e) { return e.name; } })()"), "\"DataCloneError\"");
}

#[test]
fn siblings_and_nested_frames_are_independent_contexts() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/top" => Reply::html("<iframe src=/a></iframe><iframe src=/b></iframe>"),
        "/a" => Reply::html("<script>globalThis.id='a'</script><iframe src=/leaf></iframe>"),
        "/b" => Reply::html("<script>globalThis.id='b'</script>"),
        "/leaf" => Reply::html("<script>globalThis.id='leaf'</script>"),
        _ => Reply::html("").status("404 Not Found"),
    });
    let (mut view, _e) = open(&server, "localhost", "/top");
    loaded(&mut view, 2);
    assert_eq!(child_eval(&mut view, 0, "id"), "\"a\"");
    assert_eq!(child_eval(&mut view, 1, "id"), "\"b\"");
    assert_eq!(eval(&mut view, "window.length"), "2");
    wait_for(&mut view, "document.querySelectorAll('iframe').length", "2");
    let a = view.child_frames()[0].0;
    wait_for_child(&mut view, a, "window.length", "1");
    assert_eq!(view.live_frames(), 3);
}
fn wait_for_child(view: &mut View, key: u64, script: &str, expected: &str) {
    let ok = pump(view, |v| v.child_by_key(key).is_some_and(|c| c.evaluate_json(script).map_or(false, |r| r == expected)));
    assert!(ok, "child `{script}` never became {expected}");
}

#[test]
fn srcdoc_and_about_blank_inherit_the_creators_origin_unless_sandboxed() {
    let server = server();
    let (mut view, _e) = open(&server, "localhost", "/blank");
    eval(&mut view, "document.body.innerHTML = `<iframe srcdoc=\"<script>globalThis.o = location.origin</script>\"></iframe><iframe></iframe><iframe sandbox=allow-scripts srcdoc=\"<script>globalThis.o = 'ran'; globalThis.err = (()=>{try{localStorage;return 'leak'}catch(e){return e.name}})()</script>\"></iframe><iframe sandbox srcdoc=\"<script>globalThis.o = 'ran'</script>\"></iframe>`; 0");
    loaded(&mut view, 4);
    let origin = format!("\"http://localhost:{}\"", server.port);
    assert_eq!(child_eval(&mut view, 0, "globalThis.o"), origin);
    assert_eq!(child_eval(&mut view, 1, "location.origin"), origin);
    // Sandboxed with scripts but without same-origin: opaque; storage is denied.
    assert_eq!(child_eval(&mut view, 2, "globalThis.o"), "\"ran\"");
    assert_eq!(child_eval(&mut view, 2, "globalThis.err"), "\"SecurityError\"");
    // Sandboxed without allow-scripts: there is no realm at all.
    let key = view.child_frames()[3].0;
    assert!(view.child_by_key(key).unwrap().evaluate_json("1").is_err());
}

#[test]
fn opaque_frames_send_no_cookies_and_post_null_origin() {
    let server = server();
    let (mut view, _e) = open(&server, "localhost", "/cookie-page");
    let echo = server.url("localhost", "/echo");
    eval(&mut view, &format!("document.body.innerHTML = '<iframe src=\"{echo}\"></iframe><iframe sandbox=allow-scripts src=\"{echo}\"></iframe>'; 0"));
    loaded(&mut view, 2);
    assert_eq!(child_eval(&mut view, 0, "document.getElementById('ck').textContent"), "\"sid=top\"");
    assert_eq!(child_eval(&mut view, 1, "document.getElementById('ck').textContent"), "\"none\"");
}

#[test]
fn embedding_policies_block_frames_and_keep_them_unscriptable() {
    let server = server();
    let (mut view, _e) = open(&server, "localhost", "/blank");
    let urls: Vec<String> = ["/deny", "/same", "/anc-none"].iter().map(|p| server.url("127.0.0.1", p)).collect();
    eval(&mut view, &format!("document.body.innerHTML = '{}'; 0",
        urls.iter().map(|u| format!("<iframe src=\"{u}\"></iframe>")).collect::<String>()));
    loaded(&mut view, 3);
    for i in 0..3 {
        // Cross-origin SAMEORIGIN is blocked too; every blocked frame shows the blocked page.
        assert_eq!(child_eval(&mut view, i, "document.title"), "\"Blocked\"", "frame {i}");
    }
    // A parent CSP frame-src refuses the navigation before any request.
    let (mut csp_view, _e2) = open(&server, "localhost", "/csp-parent");
    let before = server.requests("/child").len();
    eval(&mut csp_view, &format!("document.body.innerHTML = '<iframe src=\"{}\"></iframe>'; 0", server.url("localhost", "/child")));
    pump(&mut csp_view, |_| false);
    assert!(server.requests("/child").len() == before);
    assert!(csp_view.child_frames().is_empty());
}

#[test]
fn redirects_commit_the_final_principal() {
    let server = server();
    let (mut view, _e) = open(&server, "localhost", "/blank");
    eval(&mut view, &format!("document.body.innerHTML = '<iframe src=\"{}\"></iframe>'; 0", server.url("localhost", "/redir")));
    loaded(&mut view, 1);
    assert_eq!(child_eval(&mut view, 0, "location.origin"), format!("\"http://127.0.0.1:{}\"", server.port));
    assert_eq!(eval(&mut view, "(() => { try { document.querySelector('iframe').contentWindow.document; return 'leak'; } catch (e) { return e.name; } })()"), "\"SecurityError\"");
}

#[test]
fn script_created_frames_navigate_and_detach_independently() {
    let server = server();
    let (mut view, _e) = open(&server, "localhost", "/parent");
    loaded(&mut view, 1);
    let first = view.child_frames()[0].0;
    eval(&mut view, "globalThis.loads = 0; const g = document.createElement('iframe'); g.id = 'g'; g.onload = () => loads++; g.srcdoc = '<script>globalThis.v = 1</script>'; document.body.appendChild(g); 0");
    wait_for(&mut view, "loads", "1");
    assert_eq!(view.child_frames().len(), 2);
    let second = view.child_frames()[1].0;
    assert_ne!(first, second);
    // Changing srcdoc navigates only that frame: a new document and realm.
    eval(&mut view, "document.getElementById('g').srcdoc = '<script>globalThis.v = 2</script>'; 0");
    wait_for(&mut view, "loads", "2");
    assert_eq!(child_eval(&mut view, 1, "v"), "2");
    assert_eq!(child_eval(&mut view, 0, "globalThis.mark"), "\"c\"");
    // Removal tears the frame down; keys are never reused.
    eval(&mut view, "document.getElementById('g').remove(); 0");
    assert!(pump(&mut view, |v| v.child_frames().len() == 1));
    assert!(view.child_by_key(second).is_none());
    eval(&mut view, "const h = document.createElement('iframe'); document.body.appendChild(h); 0");
    assert!(pump(&mut view, |v| v.child_frames().len() == 2));
    let third = view.child_frames()[1].0;
    assert!(third > second);
    // Parent navigation drops every child.
    view.set_html("<p>new</p>").unwrap();
    assert!(view.child_frames().is_empty());
    assert_eq!(view.live_frames(), 0);
}

#[test]
fn frame_limits_bound_nesting_count_and_srcdoc_size() {
    let server = Server::new(|seen| match seen.path.as_str() {
        // A document that frames itself forever.
        "/loop" => Reply::html("<iframe src=/loop></iframe>"),
        _ => Reply::html("").status("404 Not Found"),
    });
    let (mut view, _e) = open(&server, "localhost", "/loop");
    pump(&mut view, |v| v.live_frames() >= 8);
    pump(&mut view, |_| false);
    assert!(view.live_frames() <= 32, "live frames {}", view.live_frames());
    assert!(view.live_frames() >= 1);
    let (mut flood, _e2) = open(&server, "localhost", "/blank");
    eval(&mut flood, "for (let i = 0; i < 200; i++) document.body.appendChild(document.createElement('iframe')); 0");
    pump(&mut flood, |_| false);
    assert_eq!(flood.live_frames(), 32);
    let (mut big, _e3) = open(&server, "localhost", "/blank");
    // The script layer's attribute budget refuses the oversized srcdoc before the engine does.
    assert!(big.evaluate_json("const f = document.createElement('iframe'); f.srcdoc = 'x'.repeat(5 * 1024 * 1024); document.body.appendChild(f); 0").is_err());
    pump(&mut big, |_| false);
    assert!(big.child_frames().is_empty());
}

#[test]
fn frames_composite_into_the_parent_and_receive_input_instead_of_what_is_behind() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/p" => Reply::html("<body style='margin:0;background:#fff'><div id=behind style='position:absolute;left:0;top:0;width:300px;height:200px;background:#00f' onclick='globalThis.behindClicks=(globalThis.behindClicks||0)+1'></div><iframe id=f src=/c style='position:absolute;left:20px;top:20px;width:100px;height:80px;border:0'></iframe></body>"),
        "/c" => Reply::html("<body style='margin:0;background:#f00;height:80px' onclick='globalThis.clicks=(globalThis.clicks||0)+1'><input id=i style='width:90px'></body>"),
        _ => Reply::html("").status("404 Not Found"),
    });
    let (mut view, _e) = open(&server, "localhost", "/p");
    loaded(&mut view, 1);
    let frame = view.paint(320, 240, 1.0, 1.0, false).unwrap();
    let px = |x: usize, y: usize| { let i = (y * 320 + x) * 4; [frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]] };
    assert_eq!(px(110, 90), [255, 0, 0], "inside the frame shows the child's background");
    assert_eq!(px(10, 10), [0, 0, 255], "outside shows the parent's content behind");
    assert_eq!(px(150, 150), [0, 0, 255], "clipped at the frame boundary");
    view.click(60.0, 60.0);
    view.poll();
    assert_eq!(child_eval(&mut view, 0, "globalThis.clicks"), "1");
    assert_eq!(eval(&mut view, "globalThis.behindClicks === undefined"), "true");
    view.click(200.0, 150.0);
    view.poll();
    assert_eq!(eval(&mut view, "globalThis.behindClicks"), "1");
    assert_eq!(child_eval(&mut view, 0, "globalThis.clicks"), "1");
}

#[test]
fn keyboard_input_goes_only_to_the_focused_frame() {
    use wega_engine::events::{BlitzKeyEvent, KeyState};
    let server = Server::new(|seen| match seen.path.as_str() {
        "/p" => Reply::html("<body style='margin:0'><input id=top style='position:absolute;left:0;top:150px;width:100px'><iframe src=/c style='position:absolute;left:0;top:0;width:200px;height:100px;border:0'></iframe></body>"),
        "/c" => Reply::html("<body style='margin:0'><input id=i style='width:150px'><script>globalThis.keys=[];addEventListener('keydown',e=>keys.push(e.key))</script></body>"),
        _ => Reply::html("").status("404 Not Found"),
    });
    let (mut view, _e) = open(&server, "localhost", "/p");
    loaded(&mut view, 1);
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    eval(&mut view, "globalThis.pk=[];addEventListener('keydown',e=>pk.push(e.key)); 0");
    let key = |c: &str| BlitzKeyEvent {
        key: keyboard_types::Key::Character(c.into()),
        code: keyboard_types::Code::KeyA,
        modifiers: keyboard_types::Modifiers::empty(),
        location: keyboard_types::Location::Standard,
        is_auto_repeating: false,
        is_composing: false,
        state: KeyState::Pressed,
        text: Some(c.into()),
    };
    view.click(10.0, 10.0);
    view.key(key("a"));
    assert_eq!(child_eval(&mut view, 0, "keys.length"), "1");
    assert_eq!(eval(&mut view, "pk.length"), "0", "the parent never sees keys typed into a frame");
    // Activating the parent returns focus to it.
    view.click(10.0, 160.0);
    view.key(key("b"));
    assert_eq!(child_eval(&mut view, 0, "keys.length"), "1");
}

#[test]
fn wheel_scrolls_the_frame_under_the_pointer_and_not_the_parent() {
    use wega_engine::{Pointer, PointerKind};
    let server = Server::new(|seen| match seen.path.as_str() {
        "/p" => Reply::html("<body style='margin:0;height:2000px'><iframe src=/c style='position:absolute;left:0;top:0;width:200px;height:100px;border:0'></iframe></body>"),
        "/c" => Reply::html("<body style='margin:0;height:3000px'><script>globalThis.sy=()=>scrollY</script></body>"),
        _ => Reply::html("").status("404 Not Found"),
    });
    let (mut view, _e) = open(&server, "localhost", "/p");
    loaded(&mut view, 1);
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    let mut wheel = Pointer::new(PointerKind::Wheel, 50.0, 50.0);
    wheel.delta_y = 120.0;
    view.pointer(wheel);
    view.poll();
    assert_ne!(child_eval(&mut view, 0, "sy()"), "0", "the frame scrolled");
    assert_eq!(eval(&mut view, "scrollY"), "0", "the parent did not");
}

#[test]
fn sandbox_denies_forms_popups_modals_and_top_navigation() {
    let server = server();
    let (mut view, _e) = open(&server, "localhost", "/blank");
    eval(&mut view, "document.body.innerHTML = `<iframe sandbox=allow-scripts srcdoc=\"<form id=f method=get action=/echo><button id=b>go</button></form>\"></iframe>`; 0");
    loaded(&mut view, 1);
    let probe = |view: &mut View, code: &str| child_eval(view, 0, &format!("(() => {{ try {{ return String({code}); }} catch (e) {{ return e.name; }} }})()"));
    assert_eq!(probe(&mut view, "typeof __wega_call"), "\"undefined\"");
    assert_eq!(probe(&mut view, "typeof Deno"), "\"undefined\"");
    assert_eq!(probe(&mut view, "window.open('/x')"), "\"null\"", "no popup without allow-popups");
    assert_eq!(probe(&mut view, "confirm('?')"), "\"false\"", "no modal without allow-modals");
    assert_eq!(probe(&mut view, "parent.document"), "\"SecurityError\"");
    child_eval(&mut view, 0, "parent.location.href = '/hijack'; 0");
    // Submitting the form from a sandbox without allow-forms never navigates.
    let before = server.requests("/echo").len();
    child_eval(&mut view, 0, "document.getElementById('f').submit(); 0");
    pump(&mut view, |_| false);
    assert_eq!(server.requests("/echo").len(), before);
    assert!(server.requests("/hijack").is_empty());
}

#[test]
fn window_relationships_names_and_nested_posting() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/top" => Reply::html("<iframe name=mid src=/mid></iframe>"),
        "/mid" => Reply::html("<iframe src=/leaf></iframe><script>addEventListener('message',e=>{globalThis.fromLeaf=e.data})</script>"),
        "/leaf" => Reply::html("<script>globalThis.facts=[parent!==window, top!==window, window.frameElement===null].join();top.postMessage('hello-top','*');parent.postMessage('hello-mid','*')</script>"),
        _ => Reply::html("").status("404 Not Found"),
    });
    let (mut view, _e) = open(&server, "localhost", "/top");
    loaded(&mut view, 1);
    eval(&mut view, "globalThis.tops=[];addEventListener('message',e=>tops.push([e.data,e.source])); 0");
    assert_eq!(eval(&mut view, "frames.mid === document.querySelector('iframe').contentWindow && frames[0] === frames.mid && window.length === 1"), "true");
    let mid = view.child_frames()[0].0;
    let ok = pump(&mut view, |v| v.child_by_key(mid).is_some_and(|m| m.child_frames().len() == 1));
    assert!(ok);
    let leaf = view.child_by_key(mid).unwrap().child_frames()[0].0;
    // Re-post after the leaf exists and loaded, from inside the leaf.
    pump(&mut view, |_| false);
    let m = view.child_by_key(mid).unwrap();
    m.child_by_key(leaf).unwrap().evaluate_json("top.postMessage('again-top','*'); parent.postMessage('again-mid','*'); 0").unwrap();
    pump(&mut view, |v| v.evaluate_json("tops.length").map_or(false, |r| r != "0"));
    assert_eq!(eval(&mut view, "tops.some(t => t[0] === 'again-top') && tops.every(t => t[1] === null)"), "true");
    let m = view.child_by_key(mid).unwrap();
    let ok = pump_child(m, "fromLeaf", "\"again-mid\"");
    assert!(ok);
    let facts = m.child_by_key(leaf).unwrap().evaluate_json("facts").unwrap();
    assert_eq!(facts, "\"true,true,true\"");
}
fn pump_child(view: &mut View, script: &str, expected: &str) -> bool {
    pump(view, |v| v.evaluate_json(script).map_or(false, |r| r == expected))
}

#[test]
fn child_navigation_reload_and_stale_references() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/p" => Reply::html("<iframe id=f src=/one></iframe>"),
        "/one" => Reply::html("<script>globalThis.page='one'; setTimeout(()=>{location.href='/two'},10)</script>"),
        "/two" => Reply::html("<script>globalThis.page='two'</script>"),
        "/slow" => Reply::html("<script>globalThis.page='slow'</script>").delay(Duration::from_millis(400)),
        _ => Reply::html("").status("404 Not Found"),
    });
    let (mut view, _e) = open(&server, "localhost", "/p");
    loaded(&mut view, 1);
    let key = view.child_frames()[0].0;
    assert!(pump(&mut view, |v| v.child_by_key(key).is_some_and(|c| c.evaluate_json("globalThis.page").map_or(false, |r| r == "\"two\""))));
    // The frame keeps its identity; the parent document is untouched.
    assert_eq!(view.child_frames()[0].0, key);
    assert_eq!(eval(&mut view, "document.getElementById('f') !== null"), "true");
    // A held facade becomes "closed" after removal, and messages to it vanish.
    eval(&mut view, "globalThis.w = document.getElementById('f').contentWindow; 0");
    assert_eq!(eval(&mut view, "w.closed"), "false");
    // Removal during a pending navigation: the response is dropped, nothing panics.
    eval(&mut view, &format!("document.getElementById('f').src = '{}'", server.url("localhost", "/slow")));
    pump(&mut view, |v| v.evaluate_json("0").is_ok());
    eval(&mut view, "document.getElementById('f').remove(); 0");
    assert!(pump(&mut view, |v| v.child_frames().is_empty()));
    assert_eq!(eval(&mut view, "w.closed"), "true");
    assert_eq!(eval(&mut view, "w.postMessage('x', '*'); 'ok'"), "\"ok\"");
    pump(&mut view, |_| false);
    assert!(view.child_frames().is_empty());
}
