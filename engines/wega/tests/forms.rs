//! Form submission: POST bodies, submit events, FormData, implicit submission.
mod support;
use keyboard_types::{Code, Key, Location, Modifiers};
use support::{Reply, Server, engine, open, settle, wait};
use wega_engine::events::{BlitzKeyEvent, KeyState};

fn forms() -> Server {
    Server::new(|seen| match (seen.method.as_str(), seen.path.as_str()) {
        ("POST", "/submit") => Reply::html(&format!(
            "<title>posted</title><pre id=body>{}</pre><pre id=type>{}</pre>",
            String::from_utf8_lossy(&seen.body).replace('<', "&lt;"),
            seen.header("content-type").unwrap_or_default()
        )),
        ("GET", path) if path.starts_with("/search") => {
            Reply::html(&format!("<title>searched</title><p id=query>{path}</p>"))
        }
        _ => Reply::html(
            "<title>form</title><style>body{margin:0}button{display:block;width:100px;height:40px}</style>\
             <form id=post method=post action=/submit><input name=q value=hello><input type=checkbox name=c value=yes checked>\
             <input type=checkbox name=unchecked><button id=go name=via value=button>Send</button></form>\
             <form id=multi method=post action=/submit enctype=multipart/form-data><input name=file value=x></form>\
             <form id=cancel method=post action=/submit onsubmit='event.preventDefault();globalThis.cancelled=true'><button id=nope>No</button></form>\
             <form id=search action=/search><input id=field name=q value=wega></form>",
        ),
    })
}

#[test]
fn clicking_submit_posts_an_urlencoded_body_and_shows_the_response() {
    let server = forms();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.paint(300, 300, 1.0, 1.0, false).unwrap();
    view.evaluate("document.getElementById('post').addEventListener('submit',e=>globalThis.submitter=e.submitter.id)").unwrap();
    view.click(20.0, 60.0);
    assert_eq!(
        view.evaluate_json("globalThis.submitter??null").unwrap(),
        "\"go\""
    );
    settle(&mut view);
    assert_eq!(view.title(), "posted", "{:?}", view.notices);
    assert_eq!(
        view.text("#body").as_deref(),
        Some("q=hello&c=yes&via=button")
    );
    assert_eq!(
        view.text("#type").as_deref(),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(server.requests("/submit")[0].method, "POST");
}

#[test]
fn submit_events_can_cancel_and_submit_skips_them() {
    let server = forms();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.paint(300, 300, 1.0, 1.0, false).unwrap();
    let y: f32 = view
        .evaluate_json(
            "(r=>r.top+r.height/2)(document.getElementById('nope').getBoundingClientRect())",
        )
        .unwrap()
        .parse()
        .unwrap();
    view.click(20.0, y);
    settle(&mut view);
    assert_eq!(
        view.evaluate_json("globalThis.cancelled??false").unwrap(),
        "true"
    );
    assert!(
        server.requests("/submit").is_empty(),
        "a cancelled pointer click does not submit"
    );
    view.evaluate("globalThis.cancelled=false;document.getElementById('nope').click()")
        .unwrap();
    view.poll();
    assert_eq!(
        view.evaluate_json("globalThis.cancelled??false").unwrap(),
        "true"
    );
    assert!(server.requests("/submit").is_empty());
    view.evaluate("document.getElementById('cancel').requestSubmit()")
        .unwrap();
    view.poll();
    assert!(
        server.requests("/submit").is_empty(),
        "requestSubmit fires the cancelable event"
    );
    view.evaluate("globalThis.cancelled=false;document.getElementById('cancel').submit()")
        .unwrap();
    settle(&mut view);
    assert_eq!(
        view.evaluate_json("globalThis.cancelled??false").unwrap(),
        "false"
    );
    assert_eq!(view.title(), "posted");
}

#[test]
fn multipart_and_formdata_use_the_form_entry_list() {
    let server = forms();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    assert_eq!(
        view.evaluate_json("[...new FormData(document.getElementById('post'))]")
            .unwrap(),
        "[[\"q\",\"hello\"],[\"c\",\"yes\"]]"
    );
    assert_eq!(
        view.evaluate_json("[...new FormData(document.getElementById('post'),document.getElementById('go'))].length").unwrap(),
        "3"
    );
    view.evaluate("document.getElementById('multi').submit()")
        .unwrap();
    settle(&mut view);
    let request = &server.requests("/submit")[0];
    let kind = request.header("content-type").unwrap();
    assert!(kind.starts_with("multipart/form-data; boundary="), "{kind}");
    let body = String::from_utf8_lossy(&request.body);
    assert!(
        body.contains("Content-Disposition: form-data; name=\"file\"\r\n\r\nx\r\n"),
        "{body}"
    );
}

#[test]
fn enter_in_a_single_field_submits_its_form() {
    let server = forms();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.evaluate("document.getElementById('field').focus();document.getElementById('search').addEventListener('submit',()=>globalThis.implicit=true)").unwrap();
    view.key(BlitzKeyEvent {
        key: "Enter".parse().unwrap_or(Key::Character("Enter".into())),
        code: Code::Enter,
        text: None,
        state: KeyState::Pressed,
        location: Location::Standard,
        modifiers: Modifiers::empty(),
        is_auto_repeating: false,
        is_composing: false,
    });
    assert_eq!(
        view.evaluate_json("globalThis.implicit??false").unwrap(),
        "true"
    );
    view.poll();
    // GET submissions are navigations the browser performs.
    let next = view.next_navigation().expect("navigation");
    assert_eq!(next.path(), "/search");
    assert_eq!(next.query(), Some("q=wega"));
    let _ = wait(&mut view, "true");
}
