//! VM-neutral regression contracts: only the public View API is used.
use std::{sync::Arc, time::Duration};
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};
fn page(html: &str) -> View {
    let mut view = Engine::new(
        Limits::default(),
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .create_view(Url::parse("about:blank").unwrap())
    .unwrap();
    // Cancel the initial network request; the tests never require a server.
    view.stop();
    view.url = Url::parse("https://fixture.test/errors.html").unwrap();
    view.set_html(html).unwrap();
    for _ in 0..8 {
        view.poll();
    }
    view
}
#[test]
fn missing_api_names_source_and_keeps_the_next_script_alive() {
    let mut view = page(
        "<p id=out>before</p><script>for(let i=0;i<64;i++)void document.body['featureProbe'+i];document.body.unimplementedFixtureAPI()</script><script>document.getElementById('out').textContent='after'</script>",
    );
    assert_eq!(view.text("#out").as_deref(), Some("after"));
    let errors = view.notices.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(errors.contains("TypeError"), "{errors}");
    assert!(errors.contains("inline #1"), "{errors}");
    assert!(
        errors.contains("Element.unimplementedFixtureAPI"),
        "{errors}"
    );
    assert!(view.evaluate_json("1+1").is_ok());
}
#[test]
fn earlier_timers_and_listeners_survive_a_script_exception() {
    let mut view = page(
        "<button id=b>0</button><script>const b=document.getElementById('b');b.addEventListener('click',()=>b.textContent='clicked');setTimeout(()=>document.title='timer survived',5)</script><script>throw Error('ordinary failure')</script>",
    );
    std::thread::sleep(Duration::from_millis(12));
    view.poll();
    assert_eq!(view.title(), "timer survived", "{:?}", view.notices);
    view.evaluate("__wega_dispatch('click',document.getElementById('b')._id)")
        .unwrap();
    assert_eq!(view.text("#b").as_deref(), Some("clicked"));
}
#[test]
fn one_listener_exception_does_not_block_other_listeners() {
    let mut view = page(
        "<button id=b>0</button><script>const b=document.getElementById('b');b.addEventListener('click',()=>{throw Error('listener failure')});b.addEventListener('click',()=>b.textContent='second listener')</script>",
    );
    view.evaluate("__wega_dispatch('click',document.getElementById('b')._id)")
        .unwrap();
    view.poll();
    assert_eq!(view.text("#b").as_deref(), Some("second listener"));
    assert!(view.notices.iter().any(|e| e.contains("listener failure")));
}
#[test]
fn one_timer_exception_does_not_block_other_timers() {
    let mut view = page(
        "<script>setTimeout(()=>{throw Error('timer failure')},5);setTimeout(()=>document.title='second timer',5)</script>",
    );
    std::thread::sleep(Duration::from_millis(12));
    view.poll();
    assert_eq!(view.title(), "second timer", "{:?}", view.notices);
    assert!(view.notices.iter().any(|e| e.contains("timer failure")));
}
#[test]
fn script_errors_fire_onerror_and_error_event() {
    let mut view = page(
        "<script>globalThis.onCount=0;globalThis.eventCount=0;window.onerror=()=>onCount++;window.addEventListener('error',()=>eventCount++)</script><script>throw Error('reported')</script>",
    );
    assert_eq!(view.evaluate_json("[onCount,eventCount]").unwrap(), "[1,1]");
}
#[test]
fn evaluate_json_preserves_exception_text() {
    let mut view = page("<title>live</title>");
    let error = view
        .evaluate_json("(()=>{throw new TypeError('evaluation detail')})()")
        .unwrap_err();
    assert!(error.contains("TypeError: evaluation detail"), "{error}");
    assert!(error.contains("evaluate_json"), "{error}");
    assert_eq!(view.evaluate_json("42").unwrap(), "42");
}
#[test]
fn deadline_remains_fatal_and_a_new_document_recovers() {
    let mut view = page(
        "<title>before</title><script>while(true){}</script><script>document.title='must not run'</script>",
    );
    assert_eq!(view.title(), "before");
    assert!(
        view.evaluate_json("1")
            .unwrap_err()
            .contains("No script context")
    );
    assert!(view.notices.iter().any(|e| e.contains("deadline")));
    view.set_html("<title>recovered</title>").unwrap();
    view.poll();
    assert_eq!(view.evaluate_json("1").unwrap(), "1");
}
#[test]
fn heap_and_stack_exhaustion_remain_fatal() {
    for source in [
        "globalThis.leak=new Array(40000000).fill(0.5)",
        "function recurse(){return recurse()}recurse()",
    ] {
        let mut view = page("<title>bounded</title>");
        assert!(view.evaluate(source).is_err());
        assert!(
            view.evaluate_json("1")
                .unwrap_err()
                .contains("No script context")
        );
    }
}

#[test]
fn event_loop_deadlines_are_named_and_remain_fatal() {
    let mut view = Engine::new(
        Limits {
            script_time: Duration::from_millis(100),
            ..Limits::default()
        },
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .create_view(Url::parse("about:blank").unwrap())
    .unwrap();
    view.set_html("<script>setTimeout(()=>{while(true){}},0)</script>")
        .unwrap();
    for _ in 0..12 {
        view.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        view.notices
            .iter()
            .any(|notice| notice.contains("Event loop script deadline exceeded")),
        "{:?}",
        view.notices
    );
    assert!(
        view.evaluate_json("1")
            .unwrap_err()
            .contains("No script context")
    );
}

#[test]
fn unrelated_type_errors_do_not_turn_feature_probes_into_servo_fallbacks() {
    let view = page(
        "<p id=out>before</p><script>void document.body.jQueryPrivateFixture;void document.optionalFeatureFixture;let absent;absent.length</script><script>document.getElementById('out').textContent='still running'</script>",
    );
    assert_eq!(view.text("#out").as_deref(), Some("still running"));
    assert!(
        view.notices
            .iter()
            .any(|notice| notice.contains("TypeError")),
        "{:?}",
        view.notices
    );
    assert!(view.compatibility.is_none(), "{:?}", view.compatibility);
    assert!(
        view.notices
            .iter()
            .all(|notice| !notice.contains("; missing:")),
        "{:?}",
        view.notices
    );
}
