//! Pointer input, hover, cursor, wheel, dialogs and popups.
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};
use wega_engine::{Engine, Limits, NetworkPolicy, PageEvent, Pointer, PointerKind, Url, View};

fn page(html: &str) -> View {
    let engine = Engine::new(
        Limits {
            script_time: Duration::from_secs(2),
            ..Limits::default()
        },
        NetworkPolicy::default(),
        Arc::new(|| {}),
    )
    .unwrap();
    let mut view = engine
        .create_view(Url::parse("about:blank").unwrap())
        .unwrap();
    view.url = Url::parse("https://input.test/").unwrap();
    view.set_html(html).unwrap();
    view.poll();
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    view
}
const BOXES: &str = "<style>body{margin:0}#target{width:100px;height:50px;color:rgb(0,0,0)}#target:hover{color:rgb(255,0,0);cursor:pointer}\
    #scroller{width:100px;height:50px;overflow:auto}#tall{height:500px}</style>\
    <div id=target>hover</div><div id=scroller><div id=tall></div></div>\
    <script>globalThis.log=[];for(const type of ['pointerover','mouseover','mouseenter','pointerdown','mousedown','pointerup','mouseup','click','dblclick','contextmenu','mouseout','mouseleave'])\
    document.getElementById('target').addEventListener(type,e=>log.push(type+(type==='click'?':'+e.detail+':'+e.clientX:'')))</script>";

#[test]
fn hover_applies_css_and_events_and_reports_the_cursor() {
    let mut view = page(BOXES);
    view.pointer(Pointer::new(PointerKind::Move, 20.0, 20.0));
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    assert_eq!(
        view.evaluate_json("getComputedStyle(document.getElementById('target')).color")
            .unwrap(),
        "\"rgb(255, 0, 0)\""
    );
    assert_eq!(
        view.events.pop_front(),
        Some(PageEvent::Cursor("pointer".into()))
    );
    view.pointer(Pointer::new(PointerKind::Move, 20.0, 80.0));
    assert_eq!(
        view.evaluate_json("log").unwrap(),
        "[\"pointerover\",\"mouseover\",\"mouseenter\",\"mouseout\",\"mouseleave\"]"
    );
}

#[test]
fn clicks_produce_the_full_event_sequence_and_double_clicks() {
    let mut view = page(BOXES);
    view.click(20.0, 20.0);
    view.click(21.0, 20.0);
    let log = view
        .evaluate_json("log.filter(t=>!/over|enter|out|leave/.test(t))")
        .unwrap();
    assert_eq!(
        log,
        "[\"pointerdown\",\"mousedown\",\"pointerup\",\"mouseup\",\"click:1:20\",\"pointerdown\",\"mousedown\",\"pointerup\",\"mouseup\",\"click:1:21\",\"dblclick\"]"
    );
    let mut right = Pointer::new(PointerKind::Down, 20.0, 20.0);
    right.button = 2;
    right.buttons = 2;
    view.pointer(right);
    right.kind = PointerKind::Up;
    right.buttons = 0;
    view.pointer(right);
    assert!(
        view.evaluate_json("log.at(-1)")
            .unwrap()
            .contains("contextmenu")
    );
}

#[test]
fn wheel_scrolls_the_container_under_the_pointer_unless_cancelled() {
    let mut view = page(BOXES);
    let mut wheel = Pointer::new(PointerKind::Wheel, 20.0, 70.0);
    wheel.delta_y = 40.0;
    view.pointer(wheel);
    assert_eq!(
        view.evaluate_json("document.getElementById('scroller').scrollTop")
            .unwrap(),
        "40"
    );
    view.evaluate(
        "document.getElementById('scroller').addEventListener('wheel',e=>e.preventDefault())",
    )
    .unwrap();
    view.pointer(wheel);
    assert_eq!(
        view.evaluate_json("document.getElementById('scroller').scrollTop")
            .unwrap(),
        "40"
    );
}

#[test]
fn dialogs_block_for_the_browser_answer() {
    let mut view = page("<title>dialogs</title>");
    let asked: Rc<RefCell<Vec<String>>> = Default::default();
    let record = asked.clone();
    view.set_dialog_handler(Box::new(move |kind, message, default| {
        record
            .borrow_mut()
            .push(format!("{kind}:{message}:{default}"));
        match kind {
            "confirm" => Some("true".into()),
            "prompt" => Some("Ada".into()),
            _ => None,
        }
    }));
    assert_eq!(
        view.evaluate_json("[alert('hi'),confirm('sure?'),prompt('name?','x')]")
            .unwrap(),
        "[null,true,\"Ada\"]"
    );
    assert_eq!(
        *asked.borrow(),
        ["alert:hi:", "confirm:sure?:", "prompt:name?:x"]
    );
}

#[test]
fn popups_need_a_user_gesture_and_target_blank_links_open_windows() {
    let mut view = page(
        "<style>body{margin:0}a,button{display:block;height:30px}</style><a id=out href='https://other.test/' target=_blank>out</a>\
        <button id=open onclick=\"globalThis.handle=window.open('https://popup.test/')!==null\">open</button>",
    );
    assert_eq!(
        view.evaluate_json("window.open('https://popup.test/')")
            .unwrap(),
        "null"
    );
    assert!(view.events.is_empty());
    let windows = |view: &mut View| {
        view.events
            .drain(..)
            .filter(|event| !matches!(event, PageEvent::Cursor(_)))
            .collect::<Vec<_>>()
    };
    view.click(10.0, 10.0);
    assert_eq!(
        windows(&mut view),
        [PageEvent::OpenWindow(
            "https://other.test/".parse().unwrap()
        )]
    );
    view.click(10.0, 40.0);
    assert_eq!(view.evaluate_json("handle").unwrap(), "true");
    assert_eq!(
        windows(&mut view),
        [PageEvent::OpenWindow(
            "https://popup.test/".parse().unwrap()
        )]
    );
}
#[test]
fn events_reach_ancestors_when_the_target_has_no_listeners() {
    // Event delegation (jQuery `on`, React's root listeners) depends on this.
    let mut view = page("<main><div id=leaf></div></main>");
    assert_eq!(
        view.evaluate_json("(()=>{const seen=[];addEventListener('click',()=>seen.push('window'),true);document.addEventListener('click',e=>seen.push('document:'+e.target.id));document.getElementById('leaf').dispatchEvent(new Event('click',{bubbles:true}));return seen})()").unwrap(),
        "[\"window\",\"document:leaf\"]"
    );
}
