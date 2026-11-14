//! Layout geometry through the JavaScript API: boxes, rectangles, offsets, client and scroll
//! measurements, the viewport, hit-testing and click targeting. The engine reaches geometry
//! only through `wega_dom::layout`; these tests pin what script sees. Values were first
//! recorded from the pre-migration implementation (the parity baseline); the intentional
//! differences are in `known_fixes_*` and listed in docs/architecture/dom-round10-report.md.
use std::sync::Arc;
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

fn page(html: &str) -> View {
    let mut view = Engine::new(
        Limits::default(),
        NetworkPolicy {
            offline: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .create_view(Url::parse("about:blank").unwrap())
    .unwrap();
    view.set_html(&format!("<!doctype html>{html}")).unwrap();
    view.poll();
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    view
}
/// Runs `body` with `$` = getElementById, `r(e)` = the bounding rect as `[x,y,w,h]`.
fn js(view: &mut View, body: &str) -> String {
    view.evaluate_json(&format!(
        "(()=>{{const $=id=>document.getElementById(id);const r=e=>{{const b=e.getBoundingClientRect();return [b.x,b.y,b.width,b.height]}};{body}}})()"
    ))
    .unwrap()
}

const BOXES: &str = "<style>body{margin:0}\
    #outer{width:100px;height:80px;padding:5px;border:2px solid;margin:10px;position:relative}\
    #inner{width:40px;height:20px;margin:3px}#frac{width:33.5px;height:10.25px;margin-left:.5px}\
    #ov{width:50px;height:30px;overflow:scroll}#big{width:200px;height:100px}\
    #none{display:none}#tall{height:1000px}\
    #tf{transform:translate(10px,20px) scale(2);width:10px;height:10px}</style>\
    <div id=outer><div id=inner></div></div><div id=frac></div>\
    <div id=ov><div id=big></div></div><div id=none><b id=nb></b></div>\
    <p>some <span id=sp>inline text that is fairly long and may wrap across several lines when the container is narrow</span></p>\
    <div id=tf></div><div id=tall></div>";

#[test]
fn nested_boxes_report_borders_padding_margins_and_offsets() {
    let mut v = page(BOXES);
    assert_eq!(
        js(
            &mut v,
            "const o=$('outer'),i=$('inner');return [r(o),o.getClientRects().length,[o.clientLeft,o.clientTop,o.clientWidth,o.clientHeight],[o.offsetLeft,o.offsetTop,o.offsetWidth,o.offsetHeight,o.offsetParent?.localName],[o.scrollWidth,o.scrollHeight]]"
        ),
        r#"[[10,10,114,94],1,[2,2,110,90],[10,10,114,94,"body"],[110,90]]"#
    );
    assert_eq!(
        js(
            &mut v,
            "const i=$('inner');return [r(i),[i.offsetLeft,i.offsetTop,i.offsetWidth,i.offsetHeight],i.offsetParent.id,[i.clientWidth,i.clientHeight]]"
        ),
        r#"[[20,20,40,20],[8,8,40,20],"outer",[40,20]]"#,
        "offsets are from the padding edge of the offset parent, rects from the viewport"
    );
}

#[test]
fn fractional_sizes_keep_precision_in_rects_and_round_in_integer_getters() {
    let mut v = page(BOXES);
    assert_eq!(
        js(
            &mut v,
            "const f=$('frac');return [r(f),[f.offsetLeft,f.offsetTop,f.offsetWidth,f.offsetHeight],[f.clientWidth,f.clientHeight]]"
        ),
        "[[0.5,114,33.5,10.25],[1,114,34,10],[33,10]]"
    );
}

#[test]
fn inline_elements_report_one_rect_per_line_box() {
    let mut v = page(BOXES);
    assert_eq!(
        js(
            &mut v,
            "const s=$('sp');const rs=[...s.getClientRects()];const b=s.getBoundingClientRect();return [rs.length>=2,Math.min(...rs.map(x=>x.x))===b.x,Math.min(...rs.map(x=>x.y))===b.y,Math.max(...rs.map(x=>x.right))===b.right,Math.max(...rs.map(x=>x.bottom))===b.bottom,s.clientWidth,s.offsetParent?.localName]"
        ),
        r#"[true,true,true,true,true,0,"body"]"#,
        "the bounding rect is the union of the line boxes; inline boxes have no client box"
    );
}

#[test]
fn overflow_and_element_scrolling() {
    let mut v = page(BOXES);
    assert_eq!(
        js(
            &mut v,
            "const o=$('ov');return [o.scrollWidth,o.scrollHeight,o.clientWidth,o.clientHeight,o.scrollLeft,o.scrollTop]"
        ),
        "[200,100,50,30,0,0]"
    );
    assert_eq!(
        js(
            &mut v,
            "const o=$('ov');const before=r(o);o.scrollTop=20;o.scrollLeft=30;return [o.scrollTop,o.scrollLeft,r($('big'))[0],r($('big'))[1]-before[1],r(o).join()===before.join()]"
        ),
        "[20,30,-30,-20,true]",
        "content moves with scrolling, the scroller's own box does not"
    );
    assert_eq!(
        js(
            &mut v,
            "const o=$('ov');o.scrollTo(1000,1000);return [o.scrollLeft,o.scrollTop]"
        ),
        "[150,70]",
        "clamped to scrollSize - clientSize"
    );
}

#[test]
fn viewport_scrolling_moves_rects_not_offsets() {
    let mut v = page(BOXES);
    assert_eq!(
        js(
            &mut v,
            "const f=$('frac');const before=[r(f)[1],f.offsetTop];scrollTo(0,50);return [scrollX,scrollY,r(f)[1]-before[0],f.offsetTop-before[1]]"
        ),
        "[0,50,-50,0]"
    );
    assert_eq!(
        js(
            &mut v,
            "scrollTo(0,0);return [innerWidth,innerHeight,devicePixelRatio,document.documentElement.clientWidth]"
        ),
        "[300,200,1,300]"
    );
}

#[test]
fn viewport_resize_and_scale_change_what_script_sees() {
    let mut v = page(BOXES);
    v.paint(600, 400, 2.0, 1.0, false).unwrap();
    assert_eq!(
        js(&mut v, "return [innerWidth,innerHeight,devicePixelRatio]"),
        "[300,200,2]",
        "CSS pixels do not change with the device scale"
    );
    v.paint(200, 100, 1.0, 1.0, false).unwrap();
    assert_eq!(
        js(
            &mut v,
            "return [innerWidth,innerHeight,devicePixelRatio,$('tall').offsetWidth]"
        ),
        "[200,100,1,200]",
        "a block follows the new viewport width"
    );
    v.paint(300, 200, 1.0, 2.0, false).unwrap();
    assert_eq!(
        js(&mut v, "return [innerWidth,innerHeight,devicePixelRatio]"),
        "[150,100,2]",
        "page zoom shrinks the CSS viewport and raises the pixel ratio"
    );
}

#[test]
fn boxless_hidden_and_detached_nodes_have_empty_geometry() {
    let mut v = page(BOXES);
    assert_eq!(
        js(
            &mut v,
            "const d=document.createElement('div');const n=[$('none'),$('nb'),d];return n.map(e=>[e.getClientRects().length,r(e).join(),e.offsetWidth,e.clientWidth,e.scrollHeight,e.offsetParent?.localName??null])"
        ),
        r#"[[0,"0,0,0,0",0,0,0,null],[0,"0,0,0,0",0,0,0,null],[0,"0,0,0,0",0,0,0,null]]"#,
        "known fix: getClientRects() is empty and the rect is all zero (it was one zero rect, and display:none kept its y)"
    );
    assert_eq!(
        js(
            &mut v,
            "const t=$('outer').firstChild;const x=document.createTextNode('t');return [r($('outer'))[0],typeof document.body.getBoundingClientRect]"
        ),
        r#"[10,"function"]"#
    );
}

#[test]
fn transforms_are_not_applied_to_geometry() {
    let mut v = page(BOXES);
    assert_eq!(
        js(&mut v, "const t=$('tf');return [r(t),t.offsetWidth]"),
        "[[0,234.25,10,10],10]",
        "known gap: getBoundingClientRect ignores transforms (the box is untransformed)"
    );
}

#[test]
fn geometry_follows_dom_and_css_mutation_without_stale_reads() {
    let mut v = page(BOXES);
    assert_eq!(
        js(
            &mut v,
            "const f=$('frac');const a=r(f)[3];f.style.height='50px';const b=r(f)[3];f.className='x';document.head.appendChild(Object.assign(document.createElement('style'),{textContent:'#frac.x{height:70px}'}));f.style.height='';const c=r(f)[3];const below=r($('ov'))[1];f.remove();return [a,b,c,r($('ov'))[1]-below]"
        ),
        "[10.25,50,70,-70]",
        "every read after a mutation sees the new layout"
    );
    assert_eq!(
        js(
            &mut v,
            "const o=$('outer');const a=r(o).join();const b=r(o).join();const w=o.offsetWidth;o.style.width='200px';return [a===b,w,o.offsetWidth,o.clientWidth]"
        ),
        "[true,114,214,210]",
        "repeated reads are stable; text and attribute changes feed the next read"
    );
    assert_eq!(
        js(
            &mut v,
            "const p=document.createElement('div');p.style.cssText='width:30px;height:12px';const before=p.offsetWidth;document.body.appendChild(p);const after=[p.offsetWidth,r(p)[3]];p.remove();return [before,after,p.offsetWidth]"
        ),
        "[0,[30,12],0]",
        "insertion and removal change boxlessness"
    );
}

#[test]
fn shadow_tree_geometry_is_empty_and_the_host_is_unaffected() {
    let mut v = page(
        "<style>body{margin:0}#h{width:50px;height:20px}</style><div id=h></div><script>globalThis.root=document.getElementById('h').attachShadow({mode:'open'});root.innerHTML='<div id=in style=\"width:30px;height:30px\"></div>'</script>",
    );
    assert_eq!(
        js(
            &mut v,
            "const i=root.getElementById('in');return [r($('h')),r(i).join(),i.getClientRects().length,i.offsetWidth]"
        ),
        r#"[[0,0,50,20],"0,0,0,0",0,0]"#,
        "known gap (see the Round 9 report): a shadow tree has no layout"
    );
}

#[test]
fn replacing_the_document_resets_geometry_and_scroll() {
    let mut v = page(BOXES);
    js(&mut v, "scrollTo(0,50);return 1");
    v.set_html(
        "<!doctype html><style>body{margin:0}#a{width:77px;height:33px}</style><div id=a></div>",
    )
    .unwrap();
    v.poll();
    v.paint(300, 200, 1.0, 1.0, false).unwrap();
    assert_eq!(
        js(&mut v, "return [r($('a')),scrollY,$('outer')]"),
        "[[0,0,77,33],0,null]"
    );
}

#[test]
fn resize_observer_reports_content_and_border_boxes() {
    let mut v = page(
        "<style>body{margin:0}#t{width:60px;height:20px;padding:4px;border:1px solid}</style><div id=t></div>",
    );
    v.evaluate("globalThis.seen=[];new ResizeObserver(es=>{for(const e of es)seen.push([e.contentRect.width,e.contentRect.height,e.contentRect.left,e.contentRect.top])}).observe(document.getElementById('t'));")
        .unwrap();
    v.paint(300, 200, 1.0, 1.0, false).unwrap();
    v.paint(300, 200, 1.0, 1.0, false).unwrap();
    assert_eq!(v.evaluate_json("seen").unwrap(), "[[60,20,4,4]]");
}

// ---- hit-testing and click targeting -------------------------------------------------

const HIT: &str = "<style>body{margin:0}#pad{height:300px;background:#eee}\
    #scroller{width:100px;height:60px;overflow:auto}#content{height:300px}\
    #btn{display:block;width:80px;height:30px}</style>\
    <div id=pad></div><div id=scroller><div id=content>c</div></div><button id=btn>b</button><div style=height:400px></div>\
    <script>globalThis.log=[];for(const id of ['pad','scroller','content','btn'])document.getElementById(id).addEventListener('click',e=>log.push(id));\
    document.getElementById('scroller').addEventListener('wheel',()=>log.push('wheel-scroller'));</script>";

#[test]
fn clicks_target_the_element_under_the_pointer_with_and_without_viewport_scroll() {
    let mut v = page(HIT);
    v.click(10.0, 10.0);
    // The scroller starts at document y=300; scroll so it is under client y=50..110.
    js(&mut v, "scrollTo(0,250);return 1");
    v.paint(300, 200, 1.0, 1.0, false).unwrap();
    v.click(10.0, 60.0);
    v.click(10.0, 10.0);
    v.click(10.0, 130.0);
    assert_eq!(
        v.evaluate_json("log").unwrap(),
        r#"["pad","content","scroller","pad","btn"]"#,
        "client coordinates are mapped through the viewport scroll offset"
    );
}

#[test]
fn hover_follows_the_pointer_through_a_scrolled_viewport() {
    let mut v = page(HIT);
    js(&mut v, "scrollTo(0,250);return 1");
    v.paint(300, 200, 1.0, 1.0, false).unwrap();
    v.pointer(wega_engine::Pointer::new(
        wega_engine::PointerKind::Move,
        10.0,
        60.0,
    ));
    assert_eq!(
        js(
            &mut v,
            "return [$('content').matches(':hover'),$('pad').matches(':hover')]"
        ),
        "[true,false]"
    );
}

#[test]
fn known_fix_wheel_scrolling_targets_the_container_under_a_scrolled_viewport() {
    let mut v = page(HIT);
    js(&mut v, "scrollTo(0,250);return 1");
    v.paint(300, 200, 1.0, 1.0, false).unwrap();
    // Wheel deltas use the windowing system's sign; +30 scrolls the container down by 30.
    v.scroll_at(0.0, 30.0, 10.0, 60.0);
    assert_eq!(
        js(&mut v, "return [$('scroller').scrollTop,scrollY]"),
        "[30,250]",
        "the wheel scrolled the scroller under the pointer, not the viewport (it hit the wrong document point)"
    );
}
