mod support;
use wega_engine::{Pointer, PointerKind, RenderingBackend};
fn native_page(html: &'static str) -> (support::Server, wega_engine::View) {
    let server = support::Server::new(move |_| support::Reply::html(html));
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.paint(400, 300, 1.0, 1.0, false).unwrap();
    (server, view)
}
#[test]
fn javascript_mutations_query_current_native_geometry_without_invalidate() {
    let (_server, mut view) = native_page(
        "<body style='margin:0;color:blue'><div id=target style='width:100px'>Initial text</div></body>",
    );
    let value=view.evaluate_json("(()=>{let t=document.getElementById('target');t.style.width='180px';let a=t.getBoundingClientRect().width;t.classList.add('wide');t.style.width='';let s=document.createElement('style');s.textContent='.wide {width:240px}';document.head.appendChild(s);let b=t.getBoundingClientRect().width;t.textContent='Long wrapped text '.repeat(25);let h=t.getBoundingClientRect().height;let n=document.createElement('div');n.style.height='37px';t.appendChild(n);let withChild=t.getBoundingClientRect().height;n.remove();let after=t.getBoundingClientRect().height;return [a,b,h,withChild,after,getComputedStyle(t).color]})()").unwrap();
    let v: serde_json::Value = serde_json::from_str(&value).unwrap();
    assert_eq!(v[0], 180);
    assert_eq!(v[1], 240);
    assert!(v[2].as_f64().unwrap() > 40.0);
    assert_eq!(v[3].as_f64().unwrap() - v[4].as_f64().unwrap(), 37.0);
    assert_eq!(v[5], "rgb(0, 0, 255)");
    view.paint(400, 300, 1.0, 1.0, false).unwrap();
    let generation = view
        .native_document
        .as_ref()
        .unwrap()
        .borrow()
        .layout_generation;
    view.paint(400, 300, 1.0, 1.0, false).unwrap();
    assert_eq!(
        generation,
        view.native_document
            .as_ref()
            .unwrap()
            .borrow()
            .layout_generation
    );
    view.evaluate("document.querySelector('style').textContent='.wide {width:50%}'")
        .unwrap();
    view.paint(300, 300, 1.0, 1.0, false).unwrap();
    assert_eq!(
        view.evaluate_json("document.getElementById('target').getBoundingClientRect().width")
            .unwrap(),
        "150"
    );
}
#[test]
fn pointer_targets_bubble_after_transform_clip_and_overlap() {
    let (_server, mut view) = native_page(
        "<body style='margin:0'><div id=low style='position:absolute;width:200px;height:100px;background:blue'></div><div id=top style='position:absolute;left:20px;top:20px;width:40px;height:40px;transform:scale(2);z-index:10;background:red'></div><div id=clip style='position:absolute;left:100px;top:0;width:40px;height:40px;overflow:hidden'><div id=outside style='position:absolute;left:30px;top:0;width:60px;height:40px;z-index:50'></div></div></body>",
    );
    view.evaluate("globalThis.events=[];for (const type of ['pointermove','pointerdown','pointerup','click']) document.addEventListener(type,e=>events.push(type+':'+e.target.id));document.getElementById('top').addEventListener('click',()=>events.push('target'))").unwrap();
    view.click(5.0, 5.0);
    let events = view.evaluate_json("events").unwrap();
    assert_eq!(
        events,
        r#"["pointermove:top","pointerdown:top","pointerup:top","target","click:top"]"#
    );
    view.evaluate("events=[]").unwrap();
    view.click(150.0, 20.0);
    assert_eq!(
        view.evaluate_json("events[events.length-1]").unwrap(),
        "\"click:low\""
    );
}
#[test]
fn nested_scroll_geometry_hit_targets_and_fixed_viewport() {
    let (_server, mut view) = native_page(
        "<body style='margin:0'><div id=scroller style='width:100px;height:50px;overflow:auto'><div style='height:40px'></div><div id=target style='height:40px;background:red'></div><div style='height:100px'></div></div><div style='height:1000px'></div><div id=fixed style='position:fixed;left:200px;top:10px;width:30px;height:30px;background:blue'></div></body>",
    );
    view.evaluate("globalThis.clicked='';document.addEventListener('click',e=>clicked=e.target.id);document.getElementById('scroller').scrollTop=40").unwrap();
    assert_eq!(view.evaluate_json("[document.getElementById('target').getBoundingClientRect().top,document.getElementById('scroller').scrollTop]").unwrap(),"[0,40]");
    view.click(20.0, 20.0);
    assert_eq!(view.evaluate_json("clicked").unwrap(), "\"target\"");
    view.evaluate("document.getElementById('target').style.color='blue'")
        .unwrap();
    assert_eq!(
        view.evaluate_json("document.getElementById('scroller').scrollTop")
            .unwrap(),
        "40"
    );
    view.scroll(0.0, 100.0);
    assert_eq!(
        view.evaluate_json("document.getElementById('fixed').getBoundingClientRect().top")
            .unwrap(),
        "10"
    );
    view.click(210.0, 20.0);
    assert_eq!(view.evaluate_json("clicked").unwrap(), "\"fixed\"");
    let mut wheel = Pointer::new(PointerKind::Wheel, 210.0, 20.0);
    wheel.delta_y = 50.0;
    view.pointer(wheel);
    assert_eq!(view.evaluate_json("window.scrollY").unwrap(), "150");
}
#[test]
fn iframe_real_child_content_clipping_transform_and_input() {
    let server = support::Server::new(|seen| match seen.path.as_str() {
        "/page" => support::Reply::html(
            "<body style='margin:0'><iframe id=f src='/child' style='display:block;width:100px;height:70px;border:0;transform:translate(40px,20px)'></iframe></body>",
        ),
        _ => support::Reply::html(
            "<body style='margin:0;background:green'><div style='height:140px;width:200px;background:red'>Readable child frame</div><script>document.addEventListener('click',()=>parent.postMessage('child-click','*'))</script></body>",
        ),
    });
    let engine = support::engine();
    let mut view = engine
        .create_view(wega_engine::Url::parse(&server.url("localhost", "/page")).unwrap())
        .unwrap();
    view.set_rendering_backend(RenderingBackend::WegaNative);
    support::settle(&mut view);
    view.evaluate("globalThis.childClicked=false;addEventListener('message',e=>childClicked=e.data==='child-click')").unwrap();
    let frame = view.paint(300, 200, 1.0, 1.0, false).unwrap();
    let at = |x: usize, y: usize| &frame.rgba[(y * 300 + x) * 4..(y * 300 + x) * 4 + 4];
    assert_eq!(at(50, 60), &[255, 0, 0, 255]);
    assert_eq!(at(150, 60), &[255, 255, 255, 255]);
    assert_eq!(at(50, 100), &[255, 255, 255, 255]);
    view.click(50.0, 30.0);
    assert_eq!(
        support::wait(&mut view, "childClicked ? true : null"),
        "true"
    );
    // Frame-local DOM access still runs through the existing same-origin checks.
    assert_eq!(
        view.evaluate_json(
            "document.getElementById('f').contentDocument.body.getBoundingClientRect().width"
        )
        .unwrap(),
        "100"
    );
}

#[test]
fn native_rendering_layout_and_input() {
    let (_server, mut view) = native_page(
        "<body style='margin:0'><div id=box style='width:80px;height:60px;background:red'></div></body>",
    );
    assert!(view.native_document.is_some());
    let frame = view.paint(300, 200, 1.0, 1.0, false).unwrap();
    assert_eq!(
        &frame.rgba[(30 * 300 + 30) * 4..(30 * 300 + 30) * 4 + 4],
        &[255, 0, 0, 255]
    );
    view.evaluate(
        "globalThis.count=0;document.getElementById('box').addEventListener('click',()=>count++)",
    )
    .unwrap();
    view.click(30.0, 30.0);
    assert_eq!(view.evaluate_json("count").unwrap(), "1");
}
