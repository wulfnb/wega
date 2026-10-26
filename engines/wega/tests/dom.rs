//! DOM integration against the public View: the V8 host exercises the native tree.
use keyboard_types::{Code, Key, Location, Modifiers};
use std::{sync::Arc, time::Duration};
use wega_engine::{
    Engine, Limits, NetworkPolicy, Url, View,
    events::{BlitzImeEvent, BlitzKeyEvent, KeyState},
};
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
    view.url = Url::parse("https://fixture.test/start").unwrap();
    view.set_html(html).unwrap();
    view.poll();
    view
}
#[test]
fn mutations_namespaces_collections_and_native_style_share_one_tree() {
    let mut view = page("<main id='root'></main>");
    assert_eq!(view.evaluate_json("(()=>{const root=document.getElementById('root'),live=root.children;root.innerHTML='<div class=one data-kind=test><b>hello</b></div><!-- kept -->';const first=root.firstElementChild;const clone=first.cloneNode(true);root.append(clone);clone.after(document.createTextNode('tail'));const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttributeNS('http://www.w3.org/1999/xlink','xlink:href','#shape');root.prepend(svg);first.style.color='rgb(1, 2, 3)';return [live.length,root.querySelector('.one')===first,first.matches('.one'),first.closest('main')===root,first.dataset.kind,clone.textContent,svg.getAttributeNS('http://www.w3.org/1999/xlink','href'),getComputedStyle(first).color,root.innerHTML.includes('<!-- kept -->')];})()").unwrap(),"[3,true,true,true,\"test\",\"hello\",\"#shape\",\"rgb(1, 2, 3)\",true]");
    assert_eq!(view.text("b").as_deref(), Some("hello"));
}
#[test]
fn capture_bubble_inline_handlers_and_mutation_observers() {
    let mut view =
        page("<main><button id='button' onclick=\"this.dataset.inline='yes'\">go</button></main>");
    view.evaluate("globalThis.order=[];globalThis.records=[];const button=document.getElementById('button');window.addEventListener('click',()=>order.push('window'),true);document.querySelector('main').addEventListener('click',()=>order.push('parent'));button.addEventListener('click',()=>order.push('target'));new MutationObserver(items=>records.push(...items.map(item=>[item.type,item.oldValue,item.target===button]))).observe(button,{attributes:true,attributeOldValue:true});button.setAttribute('title','one');button.setAttribute('title','two');button.click()").unwrap();
    view.poll();
    assert_eq!(
        view.evaluate_json("order").unwrap(),
        "[\"window\",\"target\",\"parent\"]"
    );
    assert_eq!(
        view.evaluate_json("[records.length,records[0],records[1],button.dataset.inline]")
            .unwrap(),
        "[3,[\"attributes\",null,true],[\"attributes\",\"one\",true],\"yes\"]"
    );
}
#[test]
fn frame_callbacks_observers_media_and_layout_come_from_blitz() {
    let mut view =
        page("<style>body{margin:0}#box{width:80px;height:40px}</style><div id='box'></div>");
    view.evaluate("globalThis.frames=[];globalThis.sizes=[];globalThis.intersections=[];const box=document.getElementById('box');requestAnimationFrame(t=>{frames.push(t);requestAnimationFrame(t=>frames.push(t))});const canceled=requestAnimationFrame(()=>frames.push('canceled'));cancelAnimationFrame(canceled);new ResizeObserver(entries=>sizes.push(entries[0].contentRect.width)).observe(box);new IntersectionObserver(entries=>intersections.push(entries[0].isIntersecting)).observe(box);globalThis.media=matchMedia('(min-width: 250px)');globalThis.mediaChanges=[];media.addEventListener('change',event=>mediaChanges.push(event.matches))").unwrap();
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    assert_eq!(view.evaluate_json("[frames.length,typeof frames[0],sizes,intersections,box.getBoundingClientRect().width,innerWidth,media.matches]").unwrap(),"[2,\"number\",[80],[true],80,300,true]");
    view.evaluate("box.style.width='120px'").unwrap();
    view.paint(200, 200, 1.0, 1.0, false).unwrap();
    assert_eq!(
        view.evaluate_json("[sizes,mediaChanges,media.matches]")
            .unwrap(),
        "[[80,120],[true,false],false]"
    );
}
#[test]
fn history_is_same_origin_and_delivers_popstate() {
    let mut view = page("<p>history</p>");
    view.evaluate("globalThis.popped=[];addEventListener('popstate',event=>popped.push(event.state));history.pushState({page:1},'','/one');history.pushState({page:2},'','/two');history.back()").unwrap();
    view.poll();
    assert_eq!(
        view.evaluate_json("[location.pathname,history.length,history.state,popped]")
            .unwrap(),
        "[\"/one\",3,{\"page\":1},[{\"page\":1}]]"
    );
    assert!(
        view.evaluate("history.pushState(null,'','https://other.test/')")
            .is_err()
    );
    assert_eq!(view.url.path(), "/one");
}
fn key(key: &str, text: Option<&str>) -> BlitzKeyEvent {
    BlitzKeyEvent {
        key: key.parse().unwrap_or(Key::Character(key.into())),
        code: Code::KeyA,
        modifiers: Modifiers::empty(),
        location: Location::Standard,
        is_auto_repeating: false,
        is_composing: false,
        state: KeyState::Pressed,
        text: text.map(Into::into),
    }
}
#[test]
fn native_keyboard_and_ime_edit_parley_and_deliver_cancelable_input() {
    let mut view = page("<input id='edit'><p id='output'></p>");
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    view.evaluate("globalThis.events=[];const edit=document.getElementById('edit');edit.focus();edit.addEventListener('keydown',event=>{events.push(event.key);if(event.key==='x')event.preventDefault()});edit.addEventListener('input',()=>events.push(edit.value))").unwrap();
    view.key(key("a", Some("a")));
    view.key(key("x", Some("x")));
    view.ime(BlitzImeEvent::Preedit("文".into(), Some((3, 3))));
    view.ime(BlitzImeEvent::Commit("文".into()));
    assert_eq!(
        view.evaluate_json("[edit.value,document.activeElement===edit,events]")
            .unwrap(),
        "[\"a文\",true,[\"a\",\"a\",\"x\",\"a文\"]]"
    );
    view.evaluate("edit.addEventListener('beforeinput',event=>event.preventDefault())")
        .unwrap();
    view.key(key("b", Some("b")));
    assert_eq!(view.evaluate_json("edit.value").unwrap(), "\"a文\"");
}
fn inserted_input_before_paint(ime: bool) {
    let mut view = page("<p>Ready</p>");
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    view.evaluate("globalThis.events=[];globalThis.edit=document.createElement('input');document.body.append(edit);edit.focus();edit.addEventListener('input',()=>events.push(edit.value))").unwrap();
    // Deliberately send input before another paint, as a worker command can
    // arrive during the frame throttle interval after JavaScript mutates DOM.
    if ime {
        view.ime(BlitzImeEvent::Commit("Hello 文".into()));
    } else {
        view.key(key("a", Some("a")));
    }
    let expected = if ime { "Hello 文" } else { "a" };
    assert_eq!(
        view.evaluate_json("[edit.value,document.activeElement===edit,events]")
            .unwrap(),
        serde_json::json!([expected, true, [expected]]).to_string(),
        "native {} before paint",
        if ime { "IME" } else { "keyboard" }
    );
}
#[test]
fn newly_inserted_focused_input_accepts_keyboard_before_paint() {
    inserted_input_before_paint(false);
}
#[test]
fn newly_inserted_focused_input_accepts_ime_before_paint() {
    inserted_input_before_paint(true);
}
#[test]
fn detached_wrappers_and_listeners_release_the_native_node_budget() {
    let engine = Engine::new(
        Limits {
            dom_nodes: 256,
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
    view.set_html("<main></main>").unwrap();
    view.poll();
    view.evaluate("globalThis.held=document.createElement('button');held.addEventListener('click',()=>held.textContent='still live');document.body.append(held);held.remove()").unwrap();
    for _ in 0..6 {
        view.evaluate("(()=>{for(let i=0;i<80;i++){const node=document.createElement('button');node.addEventListener('click',()=>{});document.body.append(node);node.remove();}})()").unwrap();
        view.freeze(true);
        view.freeze(false);
        view.paint(100, 100, 1.0, 1.0, false).unwrap();
        view.poll();
    }
    view.evaluate("held.click();document.body.append(held)")
        .unwrap();
    assert_eq!(view.text("button").as_deref(), Some("still live"));
}
#[test]
fn box_metrics_distinguish_content_padding_borders_and_scroll_overflow() {
    let mut view = page(
        "<main style='position:relative'><div id='box' style='width:80px;height:40px;padding:10px;border:5px solid;overflow:auto'><div style='width:200px;height:100px'></div></div></main>",
    );
    view.evaluate("globalThis.boxEntry=null;const box=document.getElementById('box');new ResizeObserver(entries=>boxEntry=entries[0]).observe(box)").unwrap();
    view.paint(400, 300, 1.0, 1.0, false).unwrap();
    assert_eq!(view.evaluate_json("[box.offsetWidth,box.clientLeft,box.clientTop,box.offsetParent===document.querySelector('main'),box.scrollWidth>box.clientWidth,box.scrollHeight>box.clientHeight,boxEntry.contentRect.width,boxEntry.borderBoxSize[0].inlineSize]").unwrap(),"[110,5,5,true,true,true,80,110]");
}

#[test]
fn window_named_elements_are_live_and_do_not_override_own_globals() {
    let mut view =
        page("<div id=namedFixture></div><form name=namedForm></form><div id=location></div>");
    view.evaluate("if(namedFixture!==document.getElementById('namedFixture')||namedForm!==document.querySelector('form')||typeof location.href!=='string')throw Error('Named access');namedFixture.id='renamedFixture';if(typeof namedFixture!=='undefined'||!renamedFixture)throw Error('Stale ID');globalThis.renamedFixture=7;if(renamedFixture!==7)throw Error('Own property shadowing');delete globalThis.renamedFixture;if(!renamedFixture.nodeType)throw Error('Named access restoration');").unwrap();
}

#[test]
fn adjacent_html_uses_the_insertion_position_and_namespace_context() {
    let mut view = page(
        "<div id=anchor><b>existing</b></div><table><tbody id=rows></tbody></table><svg id=graphic xmlns='http://www.w3.org/2000/svg'></svg>",
    );
    view.evaluate("const anchor=document.getElementById('anchor');anchor.insertAdjacentHTML('afterbegin','<span id=first>first</span>');anchor.insertAdjacentHTML('beforeend','<span id=last>last</span>');document.getElementById('rows').insertAdjacentHTML('beforeend','<tr><td>cell</td></tr>');document.getElementById('graphic').insertAdjacentHTML('beforeend','<circle id=dot r=5></circle>');document.body.insertAdjacentHTML('afterbegin','<style>p{color:green}</style><p id=intro>intro</p>')").unwrap();
    assert_eq!(view.evaluate_json("[anchor.firstElementChild.id,anchor.lastElementChild.id,document.getElementById('rows').firstElementChild.localName,document.getElementById('dot').namespaceURI,document.querySelectorAll('body').length,document.getElementById('intro').parentNode===document.body]").unwrap(), "[\"first\",\"last\",\"tr\",\"http://www.w3.org/2000/svg\",1,true]");
}
