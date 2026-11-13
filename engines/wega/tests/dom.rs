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
    view.url = Url::parse("https://fixture.test/start").unwrap();
    view.set_html(html).unwrap();
    view.poll();
    view
}
#[test]
fn document_location_html_document_and_live_names_support_trak_scripts() {
    let mut view = page(
        "<input name='account'><input name='Account'><div name='account'></div><svg><g name='account'></g></svg>",
    );
    assert_eq!(view.evaluate_json("[document.location===window.location,document.location.protocol,document instanceof HTMLDocument,HTMLDocument.prototype===Document.prototype]").unwrap(), "[true,\"https:\",true,true]");
    assert_eq!(view.evaluate_json("(()=>{Window.prototype.trakPrototypeProbe=function(){return this===window};let illegal=false;try{new Window()}catch(error){illegal=error instanceof TypeError}const result=[window instanceof Window,window instanceof EventTarget,window.trakPrototypeProbe(),Object.prototype.toString.call(window),illegal];delete Window.prototype.trakPrototypeProbe;return result})()").unwrap(), "[true,true,true,\"[object Window]\",true]");
    assert_eq!(view.evaluate_json("(()=>{const names=document.getElementsByName('account');const input=document.createElement('input');input.name='account';document.body.append(input);const added=names.length;input.name='other';return [names instanceof NodeList,added,names.length,document.getElementsByName('Account').length,document.implementation.createHTMLDocument('Detached').location]})()").unwrap(), "[true,3,2,1,null]");
    view.evaluate("document.location='#section'").unwrap();
    assert_eq!(
        view.evaluate_json("[location.hash,document.location.href===location.href]")
            .unwrap(),
        "[\"#section\",true]"
    );
}
#[test]
fn html_document_creation_keeps_mutations_separate_from_the_active_page() {
    let mut view = page("<title>Active</title><main id='mount'>kept</main>");
    assert_eq!(
        view.evaluate_json(
            r#"(() => {
                const detached = document.implementation.createHTMLDocument('Detached');
                detached.body.innerHTML = '<form></form><form></form>';
                const nested = detached.implementation.createHTMLDocument();
                nested.body.textContent = 'nested';
                return [detached !== document, detached.title, detached.forms.length,
                    detached.body.ownerDocument === detached,
                    detached.location, detached.defaultView, detached.visibilityState,
                    nested !== detached, nested.body.textContent, detached.forms.length,
                    document.title, document.getElementById('mount').textContent,
                    document.forms.length];
            })()"#,
        )
        .unwrap(),
        r#"[true,"Detached",2,true,null,null,"hidden",true,"nested",2,"Active","kept",0]"#
    );
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

#[test]
fn ranges_parse_contextual_html_svg_and_keep_live_boundary_points() {
    let mut view =
        page("<table><tbody id=rows></tbody></table><svg id=svg></svg><div id=mount>abcdef</div>");
    assert_eq!(view.evaluate_json(r#"(()=>{
      const range=document.createRange(), rows=document.getElementById('rows');
      range.selectNodeContents(rows);
      const fragment=range.createContextualFragment('<tr><td>row</td></tr>');
      const result=[range instanceof Range,fragment instanceof DocumentFragment,fragment.ownerDocument===document];
      rows.append(fragment);
      range.selectNodeContents(document.getElementById('svg'));
      document.getElementById('svg').append(range.createContextualFragment('<circle id="dot"/>'));
      const text=document.getElementById('mount').firstChild;
      range.setStart(text,1);range.setEnd(text,5);
      text.insertData(0,'XY');
      result.push(String(range),range.startOffset,range.endOffset);
      const second=text.splitText(4);
      result.push(String(range),range.endContainer===second,range.endOffset);
      const clone=range.cloneRange();range.collapse(true);
      result.push(range.collapsed,String(clone),clone.commonAncestorContainer.id,
        rows.firstElementChild.localName,rows.firstElementChild.firstElementChild.localName,
        document.getElementById('dot').namespaceURI);
      return result;
    })()"#).unwrap(), r#"[true,true,true,"bcde",3,7,"bcde",true,3,true,"bcde","mount","tr","td","http://www.w3.org/2000/svg"]"#);
}

#[test]
fn contextual_fragment_scripts_run_once_while_inner_html_scripts_stay_inert() {
    let mut view = page(
        "<div id=mount></div><script>globalThis.fragmentRuns=0;globalThis.fragmentCurrent=null;</script>",
    );
    view.evaluate(r#"globalThis.fragment=document.createRange().createContextualFragment('<span id="inserted">ad content</span><script id="fragment-script">fragmentRuns++;fragmentCurrent=document.currentScript.id</script>');globalThis.beforeInsertion=fragmentRuns;document.getElementById('mount').append(fragment);"#).unwrap();
    for _ in 0..8 {
        view.poll();
    }
    assert_eq!(view.evaluate_json("[beforeInsertion,fragmentRuns,fragmentCurrent,document.getElementById('inserted').textContent]").unwrap(), r#"[0,1,"fragment-script","ad content"]"#);
    view.evaluate("const script=document.getElementById('fragment-script');script.remove();document.head.append(script);document.getElementById('mount').innerHTML='<script>fragmentRuns=99</script>';").unwrap();
    for _ in 0..8 {
        view.poll();
    }
    assert_eq!(view.evaluate_json("fragmentRuns").unwrap(), "1");
    assert!(view.compatibility.is_none(), "{:?}", view.notices);
}

#[test]
fn range_insertion_removal_validation_and_document_ownership() {
    let mut view = page("<div id=mount><b>one</b><i>two</i><u>three</u></div>");
    assert_eq!(view.evaluate_json(r#"(()=>{
      const root=document.getElementById('mount'), range=document.createRange();
      range.setStart(root,1);range.setEnd(root,3);
      root.insertBefore(document.createElement('em'),root.firstChild);
      const shifted=[range.startOffset,range.endOffset];
      root.removeChild(root.children[1]);
      const removed=[range.startOffset,range.endOffset];
      range.selectNodeContents(root.children[1]);root.children[1].remove();
      const collapsed=[range.collapsed,range.startContainer===root,range.startOffset];
      const detached=document.implementation.createHTMLDocument('detached');
      const other=detached.createRange();
      const errors=[];for(const [node,offset] of [[root,100],[root,-1]]) {try{range.setStart(node,offset)}catch(error){errors.push(error.name)}}
      range.selectNodeContents(root);range.collapse(true);
      range.insertNode(range.createContextualFragment('<strong>inserted</strong>'));
      return [shifted,removed,collapsed,other.startContainer===detached,
        other.createContextualFragment('<p>detached</p>').ownerDocument===detached,errors,
        range.startOffset,range.endOffset,root.firstElementChild.localName];
    })()"#).unwrap(), r#"[[2,4],[1,3],[true,true,1],true,true,["IndexSizeError","IndexSizeError"],0,1,"strong"]"#);
}

#[test]
fn visibility_tracks_freezing_and_domain_never_relaxes_profile_origin() {
    let mut view = page("<p>visible</p>");
    view.evaluate("globalThis.visibility=[];document.onvisibilitychange=()=>visibility.push(document.visibilityState);document.domain='other.test';").unwrap();
    assert_eq!(view.evaluate_json("[document.domain,location.hostname,document.visibilityState,document.hidden,document.namespaceURI===undefined,'namespaceURI' in document,document.implementation.createHTMLDocument('').visibilityState]").unwrap(), r#"["fixture.test","fixture.test","visible",false,true,false,"hidden"]"#);
    view.freeze(true);
    view.freeze(true);
    assert_eq!(
        view.evaluate_json("[document.visibilityState,document.hidden,visibility]")
            .unwrap(),
        r#"["hidden",true,["hidden"]]"#
    );
    view.freeze(false);
    assert_eq!(
        view.evaluate_json("[document.visibilityState,document.hidden,visibility]")
            .unwrap(),
        r#"["visible",false,["hidden","visible"]]"#
    );
}

#[test]
fn wide_dom_traversal_and_live_child_lists_finish_within_the_script_budget() {
    let mut html = String::from("<main id=wide>");
    for index in 0..3000 {
        html.push_str(&format!("<span id=n{index}>item</span>"));
    }
    html.push_str("</main>");
    let mut view = page(&html);
    assert_eq!(view.evaluate_json("(()=>{let count=0;for(let node=document.getElementById('wide').firstChild;node;node=node.nextSibling)count++;return count})()").unwrap(),"3000");
    assert_eq!(view.evaluate_json("(()=>{const root=document.getElementById('wide'),list=root.childNodes,iterator=list.values();const first=iterator.next().value.id;root.firstChild.remove();return [list instanceof NodeList,list.length,first,iterator.next().value.id,root.lastChild.id,root.lastChild.previousSibling.id]})()").unwrap(),r#"[true,2999,"n0","n2","n2999","n2998"]"#);
}

#[test]
fn live_query_collections_finish_in_budget_and_invalidate_after_mutation() {
    let mut html = String::from("<main id=wide>");
    for index in 0..1500 {
        html.push_str(&format!("<span class=item name=entry id=n{index}></span>"));
    }
    html.push_str("</main>");
    let mut view = page(&html);
    assert_eq!(view.evaluate_json("(()=>{const tags=document.getElementsByTagName('span'),classes=document.getElementById('wide').getElementsByClassName('item'),names=document.getElementsByName('entry');let count=0;for(const list of [tags,classes,names])for(let i=0;i<list.length;i++)if(list[i].id)count++;classes[0].className='other';names[0].setAttribute('name','other');tags[0].remove();return [count,tags.length,classes.length,names.length,classes[0].id]})()").unwrap(),"[4500,1499,1499,1499,\"n1\"]");
    view.evaluate("document.getElementById('wide').appendChild(document.createElement('span'))")
        .unwrap();
    assert_eq!(
        view.evaluate_json("document.getElementsByTagName('span').length")
            .unwrap(),
        "1500"
    );
}

#[test]
fn repeated_style_and_geometry_reads_share_layout_until_a_mutation() {
    let mut html = String::from("<style>span{display:block;width:32px;height:4px}</style><main>");
    for _ in 0..1500 {
        html.push_str("<span></span>");
    }
    html.push_str("</main>");
    let mut view = page(&html);
    assert_eq!(view.evaluate_json("(()=>{const nodes=document.querySelectorAll('span');let total=0;for(const node of nodes){total+=node.offsetWidth;total+=parseFloat(getComputedStyle(node).width)}const first=nodes[0];first.style.width='80px';const changed=[first.offsetWidth,getComputedStyle(first).width,first.getBoundingClientRect().width];first.style.width='96px';return [total,changed,first.clientWidth]})()").unwrap(),"[96000,[80,\"80px\",80],96]");
    view.evaluate("document.querySelector('span').style.width='120px'")
        .unwrap();
    assert_eq!(
        view.evaluate_json("document.querySelector('span').offsetWidth")
            .unwrap(),
        "120"
    );
}

#[test]
fn range_fragment_limits_preserve_existing_dom_and_boundary_points() {
    let engine = Engine::new(
        Limits {
            dom_nodes: 64,
            html_bytes: 4096,
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
    view.set_html("<main id=root>original</main>").unwrap();
    view.poll();
    assert_eq!(view.evaluate_json(r#"(()=>{const root=document.getElementById('root'),text=root.firstChild,range=document.createRange();range.setStart(text,2);range.setEnd(text,5);let failures=0;for(const change of [()=>range.createContextualFragment('<b></b>'.repeat(70)),()=>range.createContextualFragment('x'.repeat(4097)),()=>root.innerHTML='<b></b>'.repeat(70),()=>text.data='x'.repeat(1024*1024+1),()=>root.textContent='x'.repeat(1024*1024+1)]){try{change()}catch(error){failures++}}return [failures,root.textContent,range.startContainer===text,range.startOffset,range.endOffset,range.toString()]})()"#).unwrap(),r#"[5,"original",true,2,5,"igi"]"#);
}

#[test]
fn live_collections_refresh_when_the_parser_adds_nodes_between_scripts() {
    let mut view = page(
        "<span></span><script>globalThis.parsed=document.getElementsByTagName('span');globalThis.firstCount=parsed.length</script><span></span><script>globalThis.secondCount=parsed.length</script>",
    );
    assert_eq!(
        view.evaluate_json("[firstCount,secondCount,parsed.length]")
            .unwrap(),
        "[1,2,2]"
    );
}

#[test]
fn live_collection_caches_do_not_keep_removed_nodes_charged() {
    let engine = Engine::new(
        Limits {
            dom_nodes: 256,
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
    view.set_html("<main></main>").unwrap();
    view.poll();
    view.evaluate("globalThis.liveButtons=document.getElementsByTagName('button');for(let i=0;i<140;i++)document.body.appendChild(document.createElement('button'));if(liveButtons.length!==140)throw Error('collection');document.body.replaceChildren()").unwrap();
    view.freeze(true);
    view.freeze(false);
    view.poll();
    view.evaluate(
        "for(let i=0;i<140;i++)document.body.appendChild(document.createElement('button'))",
    )
    .unwrap();
    assert_eq!(view.evaluate_json("liveButtons.length").unwrap(), "140");
}

#[test]
fn document_scripts_is_a_live_collection_for_ad_loader_inspection() {
    let mut view = page("<script type='application/json' id=data>{}</script>");
    assert_eq!(view.evaluate_json("(()=>{const scripts=document.scripts,script=document.createElement('script');script.type='application/json';script.id='later';document.body.append(script);const added=[scripts instanceof HTMLCollection,scripts.length,scripts[0].id,scripts[1].type];script.remove();return [added,scripts.length]})()").unwrap(),"[[true,2,\"data\",\"application/json\"],1]");
}

#[test]
fn inline_geometry_uses_line_fragments_and_dom_rect_objects() {
    let mut view = page(
        "<style>body{margin:0}p{width:70px;font:20px/24px monospace;margin:0}</style><p><a id=link>one two three</a></p>",
    );
    view.paint(200, 200, 1.0, 1.0, false).unwrap();
    assert_eq!(view.evaluate_json("(()=>{const a=document.getElementById('link'),r=a.getBoundingClientRect(),rs=a.getClientRects();return [r instanceof DOMRect,r.width>0,r.height>=48,a.offsetWidth>0,rs.length>=2,rs.every(x=>x.width>0),r.toJSON().width===r.width]})()").unwrap(),"[true,true,true,true,true,true,true]");
}

#[test]
fn computed_style_returns_used_auto_and_percentage_sizes_for_layout_consumers() {
    let mut view = page(
        "<style>body{margin:0}main{width:200px}#box{width:50%;height:20px;padding:5px;margin-left:10%;box-sizing:content-box}</style><main><div id=box></div></main>",
    );
    view.paint(300, 100, 1.0, 1.0, false).unwrap();
    assert_eq!(view.evaluate_json("(()=>{const box=document.getElementById('box'),s=getComputedStyle(box);return [s.width,s.height,s.marginLeft,s.paddingLeft,getComputedStyle(document.body).width]})()").unwrap(),"[\"100px\",\"20px\",\"20px\",\"5px\",\"300px\"]");
}

#[test]
fn computed_styles_inside_display_none_do_not_panic_the_worker() {
    let mut view = page(
        "<div style='display:none'><span id=hidden style='color:rgb(0,255,0);width:50%'>hidden</span></div>",
    );
    view.paint(200, 100, 1.0, 1.0, false).unwrap();
    assert_eq!(
        view.evaluate_json("getComputedStyle(document.getElementById('hidden')).color")
            .unwrap(),
        "\"rgb(0, 255, 0)\""
    );
    assert_eq!(
        view.evaluate_json("document.title='alive';document.title")
            .unwrap(),
        "\"alive\""
    );
}

#[test]
fn canvas_and_screen_support_speedtest_and_lottie() {
    let mut view = page("<canvas id='c' width='400' height='200'></canvas>");
    let result = view
        .evaluate_json(
            "(()=>{
            const c = document.getElementById('c');
            const created = document.createElement('canvas');
            const ctx = c.getContext('2d');
            ctx.save();
            ctx.fillStyle = '#ff0000';
            ctx.fillRect(0, 0, 10, 10);
            ctx.restore();
            const metrics = ctx.measureText('Speedtest');
            const url = c.toDataURL();
            return [
                c instanceof HTMLCanvasElement,
                created instanceof HTMLCanvasElement,
                ctx instanceof CanvasRenderingContext2D,
                c.width,
                c.height,
                created.width,
                created.height,
                metrics.width > 0,
                url.startsWith('data:image/png'),
                typeof window.screen === 'object',
                screen.width > 0,
                screen.height > 0,
                screen.colorDepth === 24,
                screen.orientation.type === 'landscape-primary'
            ];
        })()",
        )
        .unwrap();
    assert_eq!(
        result,
        "[true,true,true,400,200,300,150,true,true,true,true,true,true,true]"
    );
}

// Step 08 generated-binding contracts: brands, conversions, WebIDL descriptors,
// hidden bootstrap internals and reentrant mutation during conversion/dispatch.
#[test]
fn generated_bindings_enforce_brands_conversions_and_survive_reentrancy() {
    let mut view = page("<main id='root'><p id='a' class='x y'>text</p></main>");
    let throws = |view: &mut View, source: &str| {
        view.evaluate_json(&format!(
            "(()=>{{try{{{source};return 'no throw'}}catch(e){{return e.name}}}})()"
        ))
        .unwrap()
    };
    for source in [
        "Object.getOwnPropertyDescriptor(Node.prototype,'firstChild').get.call({})",
        "Element.prototype.getAttribute.call(document.createTextNode('t'),'id')",
        "new Node()",
        "new Element()",
        "document.body.appendChild({})",
        "document.body.insertBefore(document.createElement('i'),'not a node')",
        "a.classList.add('bad token')",
    ] {
        let name = throws(&mut view, source);
        assert!(
            name == "\"TypeError\"" || name == "\"InvalidCharacterError\"",
            "{source}: {name}"
        );
    }
    assert_eq!(
        view.evaluate_json("(()=>{const c=Object.getOwnPropertyDescriptor(Node,'ELEMENT_NODE'),e=Object.getOwnPropertyDescriptor(Event,'NONE');return [Node.ELEMENT_NODE,c.writable,c.configurable,Node.prototype.DOCUMENT_POSITION_CONTAINS,e.configurable,new Text('t').data]})()").unwrap(),
        "[1,false,false,8,false,\"t\"]"
    );
    assert_eq!(
        view.evaluate_json("(()=>{const p=document.getElementById('a');return [p.childNodes===p.childNodes,p.classList===p.classList,p.attributes===p.attributes,NodeList.prototype[Symbol.iterator]===Array.prototype.values,[...p.classList].join()]})()").unwrap(),
        "[true,true,true,true,\"x,y\"]"
    );
    assert_eq!(
        view.evaluate_json("['__wega_dom','__wega_semantics','__wega_guard_binding','__wega_call'].map(n=>typeof globalThis[n])").unwrap(),
        "[\"undefined\",\"undefined\",\"undefined\",\"undefined\"]"
    );
    // Conversion callbacks and listeners mutate the tree mid-call; the native
    // DOM must not be borrowed across JS and the call must stay consistent.
    assert_eq!(
        view.evaluate_json("(()=>{const p=document.getElementById('a'),root=document.getElementById('root');p.setAttribute('data-v',{toString(){p.remove();root.innerHTML='<b>new</b>';return 'v'}});const order=[];p.addEventListener('ping',()=>{order.push('listener');p.remove();root.append(p);p.textContent='moved'});p.dispatchEvent(new Event('ping'));order.push(p.parentNode===root);return [p.getAttribute('data-v'),p.isConnected,root.firstChild.nodeName,p.textContent,order]})()").unwrap(),
        "[\"v\",true,\"B\",\"moved\",[\"listener\",true]]"
    );
}

#[test]
fn fragment_parsing_covers_contexts_foreign_content_and_budgets() {
    let engine = Engine::new(
        Limits {
            dom_nodes: 256,
            html_bytes: 4 * 1024 * 1024,
            script_time: Duration::from_secs(5),
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
    view.set_html("<main id=m><table id=t><tbody id=tb></tbody></table><template id=tpl></template><svg id=s></svg><p id=p>old</p></main>")
        .unwrap();
    view.poll();
    let result = view
        .evaluate_json(
            r#"(()=>{
  const out = {};
  // Table insertion contexts: rows and cells need the table-aware tree builder.
  tb.innerHTML = '<tr><td>a<td>b<tr><td>c';
  out.rows = tb.rows ? tb.rows.length : tb.children.length;
  out.cells = tb.querySelectorAll('td').length;
  // insertAdjacentHTML into a row keeps the table structure.
  tb.firstElementChild.insertAdjacentHTML('beforeend', '<td>d</td>');
  out.after = tb.firstElementChild.children.length;
  // Template contents are a separate fragment.
  tpl.innerHTML = '<tr><td>t</td></tr><b>x</b>';
  out.template = [tpl.content.childNodes.length, tpl.childNodes.length];
  // SVG and foreign content.
  s.innerHTML = '<circle r="1"/><foreignObject><p>html</p></foreignObject>';
  out.svg = [s.firstChild.namespaceURI, s.lastChild.firstChild.namespaceURI];
  // Misnested markup.
  p.innerHTML = '<b><i>x</b>y</i>';
  out.misnested = p.innerHTML;
  // Nodes parsed into the document are the nodes the page holds afterwards.
  p.innerHTML = '<span id=held>h</span>';
  const held = document.getElementById('held');
  out.owned = held.parentNode === p && p.firstChild === held;
  // A document fragment from a range, with scripts inert until inserted.
  const range = document.createRange(); range.selectNode(p);
  const fragment = range.createContextualFragment('<i>one</i><u>two</u>');
  out.fragment = fragment.childNodes.length;
  // Budget: a huge fragment is rejected without touching the page.
  const before = p.innerHTML;
  let rejected = false;
  try { p.innerHTML = '<b></b>'.repeat(100000); } catch (e) { rejected = /limit/i.test(String(e)); }
  out.overflow = [rejected, p.innerHTML === before, held.parentNode === p];
  // And the page keeps working afterwards.
  p.innerHTML = '<em>ok</em>';
  out.after_overflow = p.firstChild.nodeName;
  return out;
})()"#,
        )
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["cells"], 3, "{value}");
    assert_eq!(value["after"], 3, "{value}");
    assert_eq!(value["template"], serde_json::json!([2, 0]), "{value}");
    assert_eq!(
        value["svg"],
        serde_json::json!(["http://www.w3.org/2000/svg", "http://www.w3.org/1999/xhtml"]),
        "{value}"
    );
    assert_eq!(value["owned"], true, "{value}");
    assert_eq!(value["fragment"], 2, "{value}");
    assert_eq!(
        value["overflow"],
        serde_json::json!([true, true, true]),
        "{value}"
    );
    assert_eq!(value["after_overflow"], "EM", "{value}");
    assert!(
        value["misnested"].as_str().unwrap().contains("<b>"),
        "{value}"
    );
}
