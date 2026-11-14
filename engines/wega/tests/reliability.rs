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

#[test]
fn known_gap_nesting_of_122_blocks_never_finishes_layout() {
    // Pre-existing, found by the Round 12 review: 122 or more nested block elements keep the
    // engine busy in Taffy block layout (vendored Blitz) for minutes (sampled: all time in
    // taffy `compute_block_layout` recursion) where 121 take ~22 ms.
    // The script watchdog does not cover layout, so a ~1 KB page can hang its tab's engine
    // process. Pinned (the layout does NOT finish within the window; the spinning thread ends
    // with the test process) so that a fix is noticed: the correct result is that it finishes.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut v = view();
        v.set_html(&nested(122)).unwrap();
        v.poll();
        let _ = v.evaluate_json("document.body.getBoundingClientRect().height");
        let _ = tx.send(());
    });
    assert!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).is_err(),
        "layout of 122 nested blocks finished: the hang is fixed, update this test and the audit"
    );
}
