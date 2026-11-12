//! Native input events: trust, propagation, focus and input routing.
use keyboard_types::{Code, Key, Location, Modifiers};
use std::{sync::Arc, time::Duration};
use wega_engine::events::{BlitzKeyEvent, KeyState};
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

pub fn page(html: &str) -> View {
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
    view.url = Url::parse("https://events.test/").unwrap();
    view.set_html(html).unwrap();
    view.poll();
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    view
}
pub fn key(name: &str, code: Code) -> BlitzKeyEvent {
    BlitzKeyEvent {
        key: name.parse().unwrap_or(Key::Character(name.into())),
        code,
        modifiers: Modifiers::empty(),
        location: Location::Standard,
        is_auto_repeating: false,
        is_composing: false,
        state: KeyState::Pressed,
        text: None,
    }
}
/// Evaluate and drain the page's `log` array.
pub fn log(view: &mut View) -> String {
    view.evaluate_json("JSON.stringify(log.splice(0))").unwrap()
}
pub const FIELDS: &str = "<style>body{margin:0}input{display:block;width:100px;height:30px;margin:0}</style>\
    <input id=a><input id=b><div id=plain style='height:40px'>x</div>";
pub const SPY: &str = "<script>globalThis.log=[];for(const id of ['a','b'])for(const t of ['focus','blur','focusin','focusout','keydown','keyup','click','mousedown'])\
    document.getElementById(id).addEventListener(t,e=>log.push(id+':'+t+':'+e.isTrusted+':'+(e.relatedTarget&&e.relatedTarget.id)));</script>";

#[test]
fn native_input_events_are_trusted_and_script_events_are_not() {
    let mut view = page(&format!("{FIELDS}{SPY}"));
    view.click(20.0, 10.0);
    view.key(key("x", Code::KeyX));
    let native = log(&mut view);
    assert!(
        native.contains("a:mousedown:true")
            && native.contains("a:click:true")
            && native.contains("a:focus:true")
            && native.contains("a:keydown:true"),
        "{native}"
    );
    assert!(
        !native.contains(":false:"),
        "native events must be trusted: {native}"
    );
    view.evaluate(
        "const a=document.getElementById('a');a.dispatchEvent(new Event('click',{bubbles:true}));a.click();a.dispatchEvent(new KeyboardEvent('keydown',{key:'y'}))",
    )
    .unwrap();
    let synthetic = log(&mut view);
    assert_eq!(
        synthetic.matches(":false:").count(),
        3,
        "script events stay untrusted: {synthetic}"
    );
    assert_eq!(synthetic.matches(":true:").count(), 0, "{synthetic}");
}
