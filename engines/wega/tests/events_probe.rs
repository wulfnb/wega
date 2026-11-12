use keyboard_types::{Code, Key, Location, Modifiers};
use std::{sync::Arc, time::Duration};
use wega_engine::events::{BlitzKeyEvent, KeyState};
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};
fn page(html: &str) -> View {
    let engine = Engine::new(
        Limits {
            script_time: Duration::from_secs(2),
            ..Limits::default()
        },
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap();
    let mut view = engine
        .create_view(Url::parse("about:blank").unwrap())
        .unwrap();
    view.url = Url::parse("https://probe.test/").unwrap();
    view.set_html(html).unwrap();
    view.poll();
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    view
}
fn key(k: Key, c: Code) -> BlitzKeyEvent {
    BlitzKeyEvent {
        key: k,
        code: c,
        modifiers: Modifiers::empty(),
        location: Location::Standard,
        is_auto_repeating: false,
        is_composing: false,
        state: KeyState::Pressed,
        text: None,
    }
}
#[test]
fn probe() {
    let mut v = page(
        "<style>body{margin:0}input{display:block;width:100px;height:30px;margin:0}</style><input id=a><input id=b><div id=d>x</div>\
      <script>globalThis.log=[];for(const id of ['a','b'])for(const t of ['focus','blur','focusin','focusout','keydown','click','mousedown'])document.getElementById(id).addEventListener(t,e=>log.push(id+':'+t+':'+e.isTrusted+':'+(e.relatedTarget&&e.relatedTarget.id)));</script>",
    );
    let show = |v: &mut View, label: &str| {
        let l = v.evaluate_json("JSON.stringify(log.splice(0))").unwrap();
        let a = v
            .evaluate_json(
                "document.activeElement&&document.activeElement.id||document.activeElement.tagName",
            )
            .unwrap();
        println!("PROBE {label}: log={l} active={a}");
    };
    v.click(20.0, 10.0);
    show(&mut v, "click a");
    v.click(20.0, 40.0);
    show(&mut v, "click b");
    v.evaluate("document.getElementById('a').focus()").unwrap();
    show(&mut v, "a.focus()");
    v.evaluate("document.getElementById('a').blur()").unwrap();
    show(&mut v, "a.blur()");
    v.click(20.0, 10.0);
    v.key(key("Tab".parse().unwrap(), Code::Tab));
    show(&mut v, "click a, Tab");
    v.key(key(
        "x".parse().unwrap_or(Key::Character("x".into())),
        Code::KeyX,
    ));
    show(&mut v, "key x");
    v.click(20.0, 70.0);
    show(&mut v, "click div");
}
#[test]
fn probe_active() {
    let mut v = page("<input id=a><p id=p>x</p>");
    println!(
        "PROBE2 initial active={}",
        v.evaluate_json("document.activeElement.tagName").unwrap()
    );
    v.evaluate("document.getElementById('a').focus()").unwrap();
    println!(
        "PROBE2 after focus={}",
        v.evaluate_json("document.activeElement.tagName").unwrap()
    );
}
