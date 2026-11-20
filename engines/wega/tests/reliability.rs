//! Negative and resource-exhaustion tests found by the Round 12 reliability review: hostile
//! pages must not grow engine queues without bound or hang the engine process.
use std::sync::Arc;
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

fn view() -> View {
    Engine::new(
        Limits::default(),
        NetworkPolicy {
            offline: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .create_view(Url::parse("https://probe.test/").unwrap())
    .unwrap()
}
fn nested(depth: usize) -> String {
    format!(
        "<!doctype html><body>{}x{}",
        "<div>".repeat(depth),
        "</div>".repeat(depth)
    )
}

#[test]
fn page_event_producers_cannot_flood_the_browser_queue() {
    // Each producer is gated (user activation, permission); the queue stays tiny however many
    // times a hostile script calls it within one task.
    let mut v = view();
    v.set_html("<!doctype html><div></div>").unwrap();
    v.poll();
    for (name, js) in [
        (
            "window.open",
            "for(let i=0;i<50000;i++)window.open('https://probe.test/'+i);1",
        ),
        (
            "clipboard.writeText",
            "for(let i=0;i<50000;i++)navigator.clipboard&&navigator.clipboard.writeText('x'+i).catch(()=>{});1",
        ),
        (
            "Notification",
            "for(let i=0;i<20000;i++){try{new Notification('n'+i)}catch(e){}};1",
        ),
        (
            "pushState",
            "for(let i=0;i<5000;i++)history.pushState(null,'','#'+i);1",
        ),
    ] {
        v.evaluate(js).unwrap();
        v.poll();
        assert!(
            v.events.len() <= 8,
            "{name} queued {} events",
            v.events.len()
        );
        v.events.clear();
    }
}

#[test]
fn deep_nesting_within_the_supported_depth_lays_out_and_paints() {
    for depth in [20, 100, 121] {
        let mut v = view();
        v.set_html(&nested(depth)).unwrap();
        v.poll();
        assert_eq!(
            v.evaluate_json(
                "(()=>{let n=document.body,d=0;while(n.firstElementChild){n=n.firstElementChild;d++}return d})()"
            )
            .unwrap(),
            depth.to_string()
        );
        v.evaluate_json("document.body.getBoundingClientRect().height")
            .unwrap();
        v.paint(100, 100, 1.0, 1.0, false).unwrap();
    }
}

// The two pins for the "depth-122 hang" (paint-first lays out, script-first never finishes) were
// replaced in D3-C.0 by `layout_depth.rs`: the hang was a native stack overflow on a 2 MiB thread,
// not a layout loop, and `MAX_LAYOUT_DEPTH` now refuses documents that would overflow the engine
// thread. Those tests run each depth in a killable child process on the engine's stack size.
