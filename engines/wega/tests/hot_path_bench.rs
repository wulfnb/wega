//! Hot-path timings used to check that architectural refactors add no measurable cost.
//! Run: `cargo test -p wega-engine --test hot_path_bench -- --ignored --nocapture`
//! (debug build; compare numbers only between runs of the same profile and machine state).
use std::{sync::Arc, time::Instant};
use wega_engine::events::{BlitzKeyEvent, KeyState};
use wega_engine::{Engine, Limits, NetworkPolicy, Pointer, PointerKind, Url, View};

fn view() -> View {
    let engine = Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
    let mut view = engine
        .create_view(Url::parse("https://bench.test/").unwrap())
        .unwrap();
    view.set_html("<!doctype html><title>b</title><body><div id=root></div></body>")
        .unwrap();
    view
}
fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}
fn repeat(mut run: impl FnMut() -> f64) -> f64 {
    median((0..7).map(|_| run()).collect())
}
#[test]
#[ignore]
fn hot_paths() {
    let mut page = view();
    let eval = repeat(|| {
        let start = Instant::now();
        for _ in 0..20_000 {
            page.evaluate_json("1+1").unwrap();
        }
        start.elapsed().as_secs_f64() * 1e6 / 20_000.0
    });
    let dom_ops = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');r.textContent='';for(let i=0;i<2000;i++){const d=document.createElement('div');d.setAttribute('data-i',String(i));r.appendChild(d);d.getAttribute('data-i');}return r.childNodes.length})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 2_000.0
    });
    // Node lookup, node info and subtree walks (getElementById, nodeType/nodeName, deep clone).
    let lookups = repeat(|| {
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');r.textContent='';for(let i=0;i<200;i++){const d=document.createElement('div');d.id='n'+i;d.appendChild(document.createElement('span'));r.appendChild(d);}return r.childNodes.length})()",
        )
        .unwrap();
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{let n=0;for(let i=0;i<2000;i++){const e=document.getElementById('n'+(i%200));n+=e.nodeType+e.nodeName.length+(e.parentNode?1:0);}return n})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 2_000.0
    });
    let clone = repeat(|| {
        // A fresh 51-node subtree per sample keeps the clones within the node budget.
        let mut page = view();
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');for(let i=0;i<25;i++){const d=document.createElement('div');d.appendChild(document.createElement('span'));r.appendChild(d);}return r.childNodes.length})()",
        )
        .unwrap();
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');let n=0;for(let i=0;i<10;i++){n+=r.cloneNode(true).childNodes.length}return n})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 10.0
    });
    // Mutation paths: insertion/removal, attribute and text updates, HTML parsing.
    let insert_remove = repeat(|| {
        let mut page = view();
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');const kids=[];for(let i=0;i<1000;i++){const d=document.createElement('div');r.appendChild(d);kids.push(d);}for(const d of kids)r.removeChild(d);return r.childNodes.length})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 1_000.0
    });
    let attributes = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const e=document.getElementById('root');for(let i=0;i<2000;i++){e.setAttribute('data-k',String(i));e.removeAttribute('data-k');}return 1})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 2_000.0
    });
    let text = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const t=document.createTextNode('a');document.getElementById('root').appendChild(t);for(let i=0;i<2000;i++){t.data='x'+i;}return 1})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 2_000.0
    });
    let parse = repeat(|| {
        let mut html = String::from("<!doctype html><body><div id=root>");
        for i in 0..3_000 {
            html.push_str(&format!(
                "<div id=n{i} data-i={i}><span>text {i}</span></div>"
            ));
        }
        html.push_str("</div></body>");
        let engine =
            Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
        let mut v = engine
            .create_view(Url::parse("https://bench.test/").unwrap())
            .unwrap();
        let start = Instant::now();
        v.set_html(&html).unwrap();
        start.elapsed().as_secs_f64() * 1e3
    });
    // Round 6 additions: fragment parsing, namespaced creation, shallow clone, inline style.
    let fragment_small = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');for(let i=0;i<500;i++){r.innerHTML='<p class=a>one <b>two</b></p><i>three</i>'}return 1})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 500.0
    });
    let fragment_large = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const r=document.getElementById('root');const h='<div class=c><span>text</span><a href=x>l</a></div>'.repeat(300);for(let i=0;i<10;i++){r.innerHTML=h}return 1})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e3 / 10.0
    });
    let create_ns = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{let n=0;for(let i=0;i<1000;i++){n+=document.createElementNS('http://www.w3.org/2000/svg','circle').nodeType}return n})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 1_000.0
    });
    let shallow_clone = repeat(|| {
        let mut page = view();
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const e=document.createElement('div');e.setAttribute('a','1');let n=0;for(let i=0;i<500;i++){n+=e.cloneNode(false).nodeType}return n})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 500.0
    });
    let style = repeat(|| {
        let start = Instant::now();
        page.evaluate_json(
            "(()=>{const e=document.getElementById('root');for(let i=0;i<1000;i++){e.style.setProperty('color','red');e.style.removeProperty('color')}return 1})()",
        )
        .unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 1_000.0
    });
    let engine = Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
    let lifecycle = repeat(|| {
        let start = Instant::now();
        for _ in 0..30 {
            let mut v = engine
                .create_view(Url::parse("https://bench.test/").unwrap())
                .unwrap();
            v.set_html("<!doctype html><title>x</title><body>x</body>")
                .unwrap();
        }
        start.elapsed().as_secs_f64() * 1e3 / 30.0
    });
    println!("BENCH evaluate_json(1+1): {eval:.2} us/call");
    println!(
        "BENCH dom ops (createElement+setAttribute+appendChild+getAttribute): {dom_ops:.2} us/iteration"
    );
    println!("BENCH lookup+nodeType+nodeName+parentNode: {lookups:.2} us/iteration");
    println!("BENCH deep clone of a 51-node subtree: {clone:.2} us/clone");
    println!("BENCH appendChild+removeChild: {insert_remove:.2} us/node");
    println!("BENCH setAttribute+removeAttribute: {attributes:.2} us/pair");
    println!("BENCH text node data update: {text:.2} us/update");
    println!("BENCH innerHTML small fragment (3 nodes): {fragment_small:.2} us/call");
    println!("BENCH innerHTML large fragment (900 nodes): {fragment_large:.2} ms/call");
    println!("BENCH createElementNS: {create_ns:.2} us/call");
    println!("BENCH cloneNode(false): {shallow_clone:.2} us/call");
    println!("BENCH style.setProperty+removeProperty: {style:.2} us/pair");
    println!("BENCH parse 9000-node document (set_html): {parse:.2} ms");
    println!("BENCH create_view+set_html (document lifecycle): {lifecycle:.2} ms/document");
}

fn key_event() -> BlitzKeyEvent {
    BlitzKeyEvent {
        key: "F13"
            .parse()
            .unwrap_or(keyboard_types::Key::Character("a".into())),
        code: keyboard_types::Code::F13,
        modifiers: keyboard_types::Modifiers::empty(),
        location: keyboard_types::Location::Standard,
        is_auto_repeating: false,
        is_composing: false,
        state: KeyState::Pressed,
        text: None,
    }
}
fn input_page(html: &str) -> View {
    let mut page = view();
    page.set_html(html).unwrap();
    page.poll();
    page.paint(300, 200, 1.0, 1.0, false).unwrap();
    page
}
const FIELDS: &str = "<style>body{margin:0}input{display:block;width:100px;height:30px;margin:0}</style><input id=a><input id=b><div id=plain style='height:40px'>x</div>";

/// Event, input-routing and document-lifecycle paths (Round 8).
#[test]
#[ignore]
fn event_paths() {
    // Dispatch without and with listeners, and through a deep tree (script side).
    let mut page = input_page("<div id=host></div>");
    let no_listeners = repeat(|| {
        let start = Instant::now();
        page.evaluate_json("(()=>{const e=document.getElementById('host');for(let i=0;i<2000;i++)e.dispatchEvent(new Event('x'));return 1})()").unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 2_000.0
    });
    let with_listeners = repeat(|| {
        let start = Instant::now();
        page.evaluate_json("(()=>{const e=document.getElementById('host');let n=0;if(!globalThis.armed){globalThis.armed=1;for(const t of [e,document.body,document]){t.addEventListener('y',()=>n++,true);t.addEventListener('y',()=>n++)}}for(let i=0;i<1000;i++)e.dispatchEvent(new Event('y',{bubbles:true}));return n})()").unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 1_000.0
    });
    let deep = repeat(|| {
        let mut page = input_page("<div id=top></div>");
        page.evaluate_json("(()=>{let p=document.getElementById('top');for(let i=0;i<30;i++){const d=document.createElement('div');p.appendChild(d);d.addEventListener('z',()=>{},true);d.addEventListener('z',()=>{});p=d}globalThis.leaf=p;return 1})()").unwrap();
        let start = Instant::now();
        page.evaluate_json("(()=>{for(let i=0;i<10;i++)leaf.dispatchEvent(new Event('z',{bubbles:true}));return 1})()").unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 10.0
    });
    // Native routing: click targeting, hover targeting, keyboard delivery, focus transitions.
    let mut fields = input_page(&format!(
        "{FIELDS}<script>globalThis.n=0;for(const id of ['a','b','plain'])for(const t of ['click','mousedown','mouseup','keydown'])document.getElementById(id).addEventListener(t,()=>n++)</script>"
    ));
    let click = repeat(|| {
        let start = Instant::now();
        for i in 0..100 {
            fields.click(20.0, if i % 2 == 0 { 10.0 } else { 100.0 });
        }
        start.elapsed().as_secs_f64() * 1e6 / 100.0
    });
    let hover = repeat(|| {
        let start = Instant::now();
        for i in 0..200 {
            fields.pointer(Pointer::new(
                PointerKind::Move,
                20.0,
                if i % 2 == 0 { 10.0 } else { 100.0 },
            ));
        }
        start.elapsed().as_secs_f64() * 1e6 / 200.0
    });
    fields
        .evaluate("document.getElementById('a').focus()")
        .unwrap();
    let keyboard = repeat(|| {
        let start = Instant::now();
        for _ in 0..200 {
            fields.key(key_event());
        }
        start.elapsed().as_secs_f64() * 1e6 / 200.0
    });
    let focus_js = repeat(|| {
        let start = Instant::now();
        fields.evaluate_json("(()=>{const a=document.getElementById('a'),b=document.getElementById('b');for(let i=0;i<100;i++){a.focus();b.focus()}return 1})()").unwrap();
        start.elapsed().as_secs_f64() * 1e6 / 200.0
    });
    let focus_native = repeat(|| {
        let start = Instant::now();
        for _ in 0..50 {
            fields.click(20.0, 10.0);
            fields.click(20.0, 50.0);
        }
        start.elapsed().as_secs_f64() * 1e6 / 100.0
    });
    // Document replacement on an existing view, and construction.
    let mut replace = view();
    let replacement = repeat(|| {
        let start = Instant::now();
        for _ in 0..30 {
            replace
                .set_html("<!doctype html><title>x</title><input id=i><p>text</p>")
                .unwrap();
        }
        start.elapsed().as_secs_f64() * 1e3 / 30.0
    });
    let engine = Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
    let construction = repeat(|| {
        let start = Instant::now();
        for _ in 0..30 {
            let _ = engine
                .create_view(Url::parse("https://bench.test/").unwrap())
                .unwrap();
        }
        start.elapsed().as_secs_f64() * 1e3 / 30.0
    });
    println!("BENCH dispatch, no listeners: {no_listeners:.2} us/event");
    println!("BENCH dispatch, 6 listeners over 3 targets: {with_listeners:.2} us/event");
    println!("BENCH dispatch through a 30-deep tree, 60 listeners: {deep:.2} us/event");
    println!("BENCH native click (move+down+up, 2 targets): {click:.2} us/click");
    println!("BENCH native pointer move between 2 targets: {hover:.2} us/move");
    println!("BENCH native keydown to the focused input: {keyboard:.2} us/key");
    println!("BENCH element.focus() transitions (script): {focus_js:.2} us/transition");
    println!("BENCH native click focus transitions: {focus_native:.2} us/click");
    println!("BENCH set_html document replacement: {replacement:.2} ms/document");
    println!("BENCH create_view construction: {construction:.2} ms/view");
}

/// Style paths (Round 9): inline style reads/writes, computed queries, selector matching,
/// stylesheet insertion, invalidation, recalculation and styled document creation.
#[test]
#[ignore]
fn style_paths() {
    const ITEMS: &str = "<style>body{margin:0}.item{color:rgb(1,2,3);padding:1px}.item.on{color:rgb(4,5,6)}#list>.item:nth-child(odd){margin-left:2px}</style><div id=list></div><script>const l=document.getElementById('list');for(let i=0;i<300;i++){const d=document.createElement('div');d.className='item';d.style.cssText='width: 10px; height: 2px';l.appendChild(d)}globalThis.items=[...l.children]</script>";
    let mut page = input_page(ITEMS);
    let run = |page: &mut View, js: &str, n: f64| {
        let start = Instant::now();
        page.evaluate_json(js).unwrap();
        start.elapsed().as_secs_f64() * 1e6 / n
    };
    let inline_read = repeat(|| {
        run(
            &mut page,
            "(()=>{let n=0;for(let i=0;i<3000;i++){const s=items[i%300].style;n+=s.getPropertyValue('width').length+s.height.length}return n})()",
            6_000.0,
        )
    });
    let css_text = repeat(|| {
        run(
            &mut page,
            "(()=>{let n=0;for(let i=0;i<3000;i++)n+=items[i%300].style.cssText.length;return n})()",
            3_000.0,
        )
    });
    let inline_write = repeat(|| {
        run(
            &mut page,
            "(()=>{for(let i=0;i<3000;i++)items[i%300].style.setProperty('width',(10+i%7)+'px');return 1})()",
            3_000.0,
        )
    });
    let inline_remove = repeat(|| {
        run(
            &mut page,
            "(()=>{for(let i=0;i<1500;i++){const s=items[i%300].style;s.setProperty('opacity','0.5');s.removeProperty('opacity')}return 1})()",
            3_000.0,
        )
    });
    let computed = repeat(|| {
        run(
            &mut page,
            "(()=>{let n=0;for(let i=0;i<3000;i++)n+=getComputedStyle(items[i%300]).getPropertyValue('color').length;return n})()",
            3_000.0,
        )
    });
    let computed_after_write = repeat(|| {
        run(
            &mut page,
            "(()=>{globalThis.round=(globalThis.round|0)+1;let n=0;for(let i=0;i<300;i++){const e=items[i];e.style.setProperty('height',(2+(i+round)%5)+'px');n+=getComputedStyle(e).getPropertyValue('height').length}return n})()",
            300.0,
        )
    });
    let matching = repeat(|| {
        run(
            &mut page,
            "(()=>{let n=0;for(let i=0;i<200;i++)n+=document.querySelectorAll('#list > .item:nth-child(odd)').length;for(let i=0;i<3000;i++)n+=items[i%300].matches('.item.on, div.item')?1:0;return n})()",
            3_200.0,
        )
    });
    let sheet = repeat(|| {
        run(
            &mut page,
            "(()=>{let n=0;for(let i=0;i<30;i++){const s=document.createElement('style');s.textContent='.item{outline:'+(i%3+1)+'px solid red}';document.head.appendChild(s);n+=getComputedStyle(items[0]).getPropertyValue('outline-width').length;s.remove()}return n})()",
            30.0,
        )
    });
    let invalidation = repeat(|| {
        run(
            &mut page,
            "(()=>{let n=0;for(let i=0;i<300;i++){const e=items[i];e.classList.toggle('on');n+=getComputedStyle(e).getPropertyValue('color').length}return n})()",
            300.0,
        )
    });
    let recalc = repeat(|| {
        let start = Instant::now();
        for i in 0..20 {
            page.evaluate(if i % 2 == 0 {
                "for(const e of items)e.classList.add('on')"
            } else {
                "for(const e of items)e.classList.remove('on')"
            })
            .unwrap();
            page.paint(300, 200, 1.0, 1.0, false).unwrap();
        }
        start.elapsed().as_secs_f64() * 1e3 / 20.0
    });
    let mut fresh = view();
    let creation = repeat(|| {
        let start = Instant::now();
        for _ in 0..5 {
            fresh.set_html(ITEMS).unwrap();
            fresh.poll();
            fresh.paint(300, 200, 1.0, 1.0, false).unwrap();
        }
        start.elapsed().as_secs_f64() * 1e3 / 5.0
    });
    // The same value again: CSSOM makes this a no-op (no mutation, no restyle).
    let unchanged = repeat(|| {
        run(
            &mut page,
            "(()=>{let n=0;for(let i=0;i<300;i++){const e=items[i];e.style.setProperty('display','block');n+=getComputedStyle(e).getPropertyValue('display').length}return n})()",
            300.0,
        )
    });
    let supports = repeat(|| {
        run(
            &mut page,
            "(()=>{let n=0;for(let i=0;i<2000;i++)n+=(CSS.supports('display','grid')?1:0)+(CSS.supports('(display:grid) and (gap:1px)')?1:0)+(matchMedia('(min-width:100px)').matches?1:0);return n})()",
            6_000.0,
        )
    });
    println!("BENCH inline style property read: {inline_read:.2} us/read");
    println!("BENCH inline style cssText read: {css_text:.2} us/read");
    println!("BENCH inline style setProperty: {inline_write:.2} us/write");
    println!("BENCH inline style set+removeProperty: {inline_remove:.2} us/op");
    println!("BENCH getComputedStyle query, no mutation: {computed:.2} us/query");
    println!("BENCH getComputedStyle after inline write: {computed_after_write:.2} us/query");
    println!("BENCH getComputedStyle after an unchanged inline write: {unchanged:.2} us/query");
    println!("BENCH selector matching (querySelectorAll, matches): {matching:.2} us/op");
    println!("BENCH stylesheet insert+query+remove: {sheet:.2} us/sheet");
    println!("BENCH class toggle invalidation + query: {invalidation:.2} us/toggle");
    println!("BENCH restyle 300 elements + paint: {recalc:.2} ms/frame");
    println!("BENCH styled document creation (300 items) + paint: {creation:.2} ms/document");
    println!("BENCH CSS.supports/matchMedia: {supports:.2} us/query");
}
