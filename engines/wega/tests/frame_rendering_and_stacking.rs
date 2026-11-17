//! Stage 3: Frame rendering order, stacking contexts, clipping, opacity, and hit-testing order.

use std::sync::Arc;
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

fn child_eval(view: &mut View, index: usize, script: &str) -> String {
    let key = view.child_frames()[index].0;
    view.child_by_key(key)
        .unwrap()
        .evaluate_json(script)
        .unwrap()
}

#[test]
fn positioned_element_above_frame_receives_input_and_paints_on_top() {
    let mut view = page("https://example.test/stacking");
    view.set_html(
        r#"<!doctype html>
        <html>
        <body style="margin:0; background:#fff">
            <!-- Frame with z-index: 1 and red background -->
            <iframe id="frame" srcdoc="<body style='margin:0; width:100px; height:100px; background:#f00' onclick='globalThis.frameClicked = true'></body>"
                    style="position:absolute; left:50px; top:50px; width:100px; height:100px; z-index:1; border:0"></iframe>
            <!-- Overlay element with z-index: 2, covering the frame with green background -->
            <div id="overlay" onclick="globalThis.overlayClicked = true"
                 style="position:absolute; left:50px; top:50px; width:100px; height:100px; z-index:2; background:#0f0"></div>
        </body>
        </html>"#,
    )
    .unwrap();
    view.poll();

    // Render frame
    let frame = view.paint(200, 200, 1.0, 1.0, false).unwrap();
    let px = |x: usize, y: usize| {
        let i = (y * 200 + x) * 4;
        [frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]]
    };

    // The overlay is z-index 2 over the frame (z-index 1): pixels at (75, 75) must be GREEN [0, 255, 0], NOT red
    assert_eq!(
        px(75, 75),
        [0, 255, 0],
        "overlay positioned above frame must paint on top"
    );

    // Hit-testing order: clicking at (75, 75) must hit the overlay, NOT the frame
    view.click(75.0, 75.0);
    view.poll();

    assert_eq!(
        eval(&mut view, "globalThis.overlayClicked === true"),
        "true",
        "overlay must receive the click event"
    );
    assert_eq!(
        child_eval(&mut view, 0, "globalThis.frameClicked === undefined"),
        "true",
        "frame behind overlay must NOT receive the click event"
    );
}

#[test]
fn positioned_element_below_frame_paints_under_frame_and_frame_receives_input() {
    let mut view = page("https://example.test/under");
    view.set_html(
        r#"<!doctype html>
        <html>
        <body style="margin:0; background:#fff">
            <!-- Underlay element with z-index: 1 (blue) -->
            <div id="underlay" onclick="globalThis.underlayClicked = true"
                 style="position:absolute; left:50px; top:50px; width:100px; height:100px; z-index:1; background:#00f"></div>
            <!-- Frame with z-index: 2 (red) -->
            <iframe id="frame" srcdoc="<body style='margin:0; width:100px; height:100px; background:#f00' onclick='globalThis.frameClicked = true'></body>"
                    style="position:absolute; left:50px; top:50px; width:100px; height:100px; z-index:2; border:0"></iframe>
        </body>
        </html>"#,
    )
    .unwrap();
    view.poll();

    let frame = view.paint(200, 200, 1.0, 1.0, false).unwrap();
    let px = |x: usize, y: usize| {
        let i = (y * 200 + x) * 4;
        [frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]]
    };

    // Frame is z-index 2 over underlay (z-index 1): pixels at (75, 75) must be RED [255, 0, 0]
    assert_eq!(
        px(75, 75),
        [255, 0, 0],
        "frame positioned above underlay must paint on top"
    );

    view.click(75.0, 75.0);
    view.poll();

    assert_eq!(
        child_eval(&mut view, 0, "globalThis.frameClicked === true"),
        "true",
        "frame must receive click"
    );
    assert_eq!(
        eval(&mut view, "globalThis.underlayClicked === undefined"),
        "true",
        "underlay must not receive click"
    );
}

#[test]
fn container_overflow_clipping_clips_frame() {
    let mut view = page("https://example.test/clipping");
    view.set_html(
        r#"<!doctype html>
        <html>
        <body style="margin:0; background:#fff">
            <!-- Container with overflow: hidden; width: 60px; height: 60px -->
            <div style="position:absolute; left:20px; top:20px; width:60px; height:60px; overflow:hidden; background:#fff">
                <!-- Frame 100x100 (red) placed inside the 60x60 container -->
                <iframe srcdoc="<body style='margin:0; background:#f00; width:100px; height:100px'></body>"
                        style="width:100px; height:100px; border:0"></iframe>
            </div>
        </body>
        </html>"#,
    )
    .unwrap();
    view.poll();

    let frame = view.paint(200, 200, 1.0, 1.0, false).unwrap();
    let px = |x: usize, y: usize| {
        let i = (y * 200 + x) * 4;
        [frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]]
    };

    // Inside the container (30, 30): Frame paints red [255, 0, 0]
    assert_eq!(
        px(30, 30),
        [255, 0, 0],
        "inside clipped container must show frame content"
    );

    // Outside the container (100, 100): Clipped away by overflow: hidden, so background is white [255, 255, 255]
    assert_eq!(
        px(100, 100),
        [255, 255, 255],
        "outside overflow:hidden container must be clipped"
    );
}

#[test]
fn opacity_applied_to_frame_rendering() {
    let mut view = page("https://example.test/opacity");
    view.set_html(
        r#"<!doctype html>
        <html>
        <body style="margin:0; background:#000">
            <!-- Frame with white background (#fff) and opacity: 0.5 over black background (#000) -->
            <iframe srcdoc="<body style='margin:0; background:#fff'></body>"
                    style="position:absolute; left:0; top:0; width:100px; height:100px; opacity:0.5; border:0"></iframe>
        </body>
        </html>"#,
    )
    .unwrap();
    view.poll();

    let frame = view.paint(100, 100, 1.0, 1.0, false).unwrap();
    let px = |x: usize, y: usize| {
        let i = (y * 100 + x) * 4;
        [frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]]
    };

    // White with opacity 0.5 over black produces mid-gray (~128)
    let color = px(50, 50);
    assert!(
        color[0] > 100 && color[0] < 160 && color[1] > 100 && color[1] < 160,
        "opacity 0.5 on frame must blend with parent background: got {:?}",
        color
    );
}

#[test]
fn nested_frame_rendering_and_stacking() {
    let mut view = page("https://example.test/nested");
    view.set_html(
        r#"<!doctype html>
        <html>
        <body style="margin:0; background:#000">
            <!-- Parent hosts Child 1 (green, 150x150) which hosts Child 2 (red, 80x80) -->
            <iframe id="child1"
                    srcdoc="<body style='margin:0; width:150px; height:150px; background:#0f0'><iframe id='child2' srcdoc='&lt;body style=&quot;margin:0; width:80px; height:80px; background:#f00&quot;&gt;&lt;/body&gt;' style='position:absolute; left:20px; top:20px; width:80px; height:80px; border:0'></iframe></body>"
                    style="position:absolute; left:10px; top:10px; width:150px; height:150px; border:0"></iframe>
        </body>
        </html>"#,
    )
    .unwrap();
    for _ in 0..5 {
        view.poll();
    }

    let frame = view.paint(200, 200, 1.0, 1.0, false).unwrap();
    let px = |x: usize, y: usize| {
        let i = (y * 200 + x) * 4;
        [frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]]
    };

    // Inside Child 2 (at 10 + 20 + 40 = 70, 70): Red [255, 0, 0]
    assert_eq!(px(70, 70), [255, 0, 0], "nested child 2 paints red");

    // Inside Child 1 but outside Child 2 (at 15, 15): Green [0, 255, 0]
    assert_eq!(px(15, 15), [0, 255, 0], "child 1 paints green");

    // Outside Child 1 (at 180, 180): Black [0, 0, 0]
    assert_eq!(px(180, 180), [0, 0, 0], "parent background paints black");
}
