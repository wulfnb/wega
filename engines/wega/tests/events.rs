//! Native input events: trust, propagation, focus, shadow DOM, identity and document lifecycle.
mod support;
use keyboard_types::{Code, Key, Location, Modifiers};
use std::{sync::Arc, time::Duration};
use wega_engine::events::{BlitzKeyEvent, KeyState};
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

pub fn page(html: &str) -> View {
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
    view.url = Url::parse("https://events.test/").unwrap();
    view.set_html(html).unwrap();
    view.poll();
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    view
}
pub fn key(name: &str, code: Code) -> BlitzKeyEvent {
    BlitzKeyEvent {
        key: name.parse().unwrap_or(Key::Character(name.into())),
        code,
        modifiers: Modifiers::empty(),
        location: Location::Standard,
        is_auto_repeating: false,
        is_composing: false,
        state: KeyState::Pressed,
        text: None,
    }
}
/// Evaluate and drain the page's `log` array.
pub fn log(view: &mut View) -> String {
    let encoded = view.evaluate_json("JSON.stringify(log.splice(0))").unwrap();
    serde_json::from_str(&encoded).unwrap()
}
pub const FIELDS: &str = "<style>body{margin:0}input{display:block;width:100px;height:30px;margin:0}</style>\
    <input id=a><input id=b><div id=plain style='height:40px'>x</div>";
pub const SPY: &str = "<script>globalThis.log=[];for(const id of ['a','b'])for(const t of ['focus','blur','focusin','focusout','keydown','keyup','click','mousedown'])\
    document.getElementById(id).addEventListener(t,e=>log.push(id+':'+t+':'+e.isTrusted+':'+(e.relatedTarget&&e.relatedTarget.id)));</script>";

#[test]
fn native_input_events_are_trusted_and_script_events_are_not() {
    let mut view = page(&format!("{FIELDS}{SPY}"));
    view.click(20.0, 10.0);
    view.key(key("x", Code::KeyX));
    let native = log(&mut view);
    assert!(
        native.contains("a:mousedown:true")
            && native.contains("a:click:true")
            && native.contains("a:focus:true")
            && native.contains("a:keydown:true"),
        "{native}"
    );
    assert!(
        !native.contains(":false:"),
        "native events must be trusted: {native}"
    );
    view.evaluate(
        "const a=document.getElementById('a');a.dispatchEvent(new Event('click',{bubbles:true}));a.click();a.dispatchEvent(new KeyboardEvent('keydown',{key:'y'}))",
    )
    .unwrap();
    let synthetic = log(&mut view);
    assert_eq!(
        synthetic.matches(":false:").count(),
        3,
        "script events stay untrusted: {synthetic}"
    );
    assert_eq!(synthetic.matches(":true:").count(), 0, "{synthetic}");
}

fn eval(view: &mut View, script: &str) -> serde_json::Value {
    serde_json::from_str(&view.evaluate_json(script).unwrap()).unwrap()
}

// ---- dispatch semantics (deno_web owns them; these pin the behaviour the engine relies on)

const TREE: &str = "<div id=outer><div id=mid><button id=inner>go</button></div></div>";

#[test]
fn capture_target_bubble_order_and_phases_through_a_deep_tree() {
    let mut view = page(TREE);
    let result = eval(
        &mut view,
        r#"(()=>{const log=[];const ids=['outer','mid','inner'].map(i=>document.getElementById(i));
        const nodes=[window,document,...ids];
        const name=n=>n===window?'win':n===document?'doc':n.id;
        for(const n of nodes){n.addEventListener('ping',e=>log.push(name(n)+':cap:'+e.eventPhase),true);n.addEventListener('ping',e=>log.push(name(n)+':bub:'+e.eventPhase));}
        const notBubbling=[];
        ids[2].dispatchEvent(new Event('ping',{bubbles:true}));
        const first=log.splice(0);
        ids[2].dispatchEvent(new Event('ping',{bubbles:false}));
        return {first,second:log.splice(0)};})()"#,
    );
    assert_eq!(
        result["first"],
        serde_json::json!([
            "win:cap:1",
            "doc:cap:1",
            "outer:cap:1",
            "mid:cap:1",
            "inner:cap:2",
            "inner:bub:2",
            "mid:bub:3",
            "outer:bub:3",
            "doc:bub:3",
            "win:bub:3"
        ])
    );
    assert_eq!(
        result["second"],
        serde_json::json!([
            "win:cap:1",
            "doc:cap:1",
            "outer:cap:1",
            "mid:cap:1",
            "inner:cap:2",
            "inner:bub:2"
        ]),
        "a non-bubbling event still captures and reaches its target"
    );
}

#[test]
fn stop_propagation_and_stop_immediate_propagation() {
    let mut view = page(TREE);
    let result = eval(
        &mut view,
        r#"(()=>{const log=[];const [outer,mid,inner]=['outer','mid','inner'].map(i=>document.getElementById(i));
        inner.addEventListener('p',e=>log.push('inner1'));
        inner.addEventListener('p',e=>{log.push('inner2');e.stopPropagation()});
        inner.addEventListener('p',e=>log.push('inner3'));
        mid.addEventListener('p',e=>log.push('mid'));outer.addEventListener('p',e=>log.push('outer'));
        inner.dispatchEvent(new Event('p',{bubbles:true}));const stop=log.splice(0);
        const inner4=document.createElement('i');mid.appendChild(inner4);
        inner4.addEventListener('q',e=>{log.push('a');e.stopImmediatePropagation()});
        inner4.addEventListener('q',e=>log.push('b'));mid.addEventListener('q',e=>log.push('mid'));
        inner4.dispatchEvent(new Event('q',{bubbles:true}));
        const capture=[];outer.addEventListener('r',e=>{capture.push('outer-cap');e.stopPropagation()},true);
        inner.addEventListener('r',e=>capture.push('inner'));inner.dispatchEvent(new Event('r',{bubbles:true}));
        return {stop,immediate:log.splice(0),capture};})()"#,
    );
    assert_eq!(
        result["stop"],
        serde_json::json!(["inner1", "inner2", "inner3"]),
        "same-target listeners still run, ancestors do not"
    );
    assert_eq!(result["immediate"], serde_json::json!(["a"]));
    assert_eq!(
        result["capture"],
        serde_json::json!(["outer-cap"]),
        "a capture-phase stop prevents the target"
    );
}

#[test]
fn once_passive_cancelable_and_prevent_default() {
    let mut view = page(TREE);
    let result = eval(
        &mut view,
        r#"(()=>{const inner=document.getElementById('inner');const r={};let n=0;
        inner.addEventListener('o',()=>n++,{once:true});
        inner.dispatchEvent(new Event('o'));inner.dispatchEvent(new Event('o'));r.once=n;
        inner.addEventListener('c',e=>e.preventDefault());
        r.cancelable=inner.dispatchEvent(new Event('c',{cancelable:true}));
        r.nonCancelable=inner.dispatchEvent(new Event('c',{cancelable:false}));
        inner.addEventListener('p',e=>e.preventDefault(),{passive:true});
        const passive=new Event('p',{cancelable:true});r.passive=[inner.dispatchEvent(passive),passive.defaultPrevented];
        const e2=new Event('c',{cancelable:true});inner.dispatchEvent(e2);r.defaultPrevented=e2.defaultPrevented;
        return r;})()"#,
    );
    assert_eq!(result["once"], 1);
    assert_eq!(
        result["cancelable"], false,
        "dispatchEvent returns false when cancelled"
    );
    assert_eq!(
        result["nonCancelable"], true,
        "preventDefault on a non-cancelable event does nothing"
    );
    assert_eq!(
        result["passive"],
        serde_json::json!([true, false]),
        "passive listeners cannot cancel"
    );
    assert_eq!(result["defaultPrevented"], true);
}

// ---- native input

#[test]
fn native_events_have_the_right_cancelable_and_bubbles_flags() {
    let mut view = page(&format!(
        "{FIELDS}<script>globalThis.log=[];for(const t of ['focus','focusin','click','keydown'])document.addEventListener(t,e=>log.push(t+':'+e.bubbles+':'+e.cancelable),true);</script>"
    ));
    view.click(20.0, 10.0);
    view.key(key("x", Code::KeyX));
    let flags = log(&mut view);
    assert!(
        flags.contains("focus:false:false"),
        "focus does not bubble and is not cancelable: {flags}"
    );
    assert!(flags.contains("focusin:true:false"), "{flags}");
    assert!(flags.contains("click:true:true"), "{flags}");
    assert!(flags.contains("keydown:true:true"), "{flags}");
}

#[test]
fn a_click_targets_the_innermost_element_and_bubbles_to_ancestors() {
    let mut view = page(
        "<style>body{margin:0}#outer{width:100px;height:40px}#inner{display:block;width:60px;height:20px}</style>\
         <div id=outer><span id=inner style='display:block'>x</span></div>\
         <script>globalThis.log=[];for(const id of ['outer','inner'])document.getElementById(id).addEventListener('click',e=>log.push(id+':'+e.target.id+':'+e.currentTarget.id));</script>",
    );
    view.click(10.0, 5.0);
    assert_eq!(
        log(&mut view),
        r#"["inner:inner:inner","outer:inner:outer"]"#
    );
    view.click(80.0, 30.0);
    assert_eq!(
        log(&mut view),
        r#"["outer:outer:outer"]"#,
        "a click outside the child targets the parent"
    );
}

#[test]
fn preventing_a_click_stops_its_default_action_and_preventing_keydown_stops_typing() {
    let mut view = page(
        "<style>body{margin:0}input{display:block;width:100px;height:30px;margin:0}</style><input id=box type=checkbox><input id=text>",
    );
    view.click(5.0, 10.0);
    assert_eq!(
        view.evaluate_json("document.getElementById('box').checked")
            .unwrap(),
        "true"
    );
    view.evaluate("document.getElementById('box').addEventListener('click',e=>e.preventDefault())")
        .unwrap();
    view.click(5.0, 10.0);
    assert_eq!(
        view.evaluate_json("document.getElementById('box').checked")
            .unwrap(),
        "true",
        "a cancelled click does not toggle the checkbox"
    );
    view.click(20.0, 40.0);
    let mut typed = key("q", Code::KeyQ);
    typed.text = Some("q".into());
    view.key(typed.clone());
    assert_eq!(
        view.evaluate_json("document.getElementById('text').value")
            .unwrap(),
        "\"q\""
    );
    view.evaluate(
        "document.getElementById('text').addEventListener('keydown',e=>e.preventDefault())",
    )
    .unwrap();
    view.key(typed);
    assert_eq!(
        view.evaluate_json("document.getElementById('text').value")
            .unwrap(),
        "\"q\"",
        "a cancelled keydown inserts nothing"
    );
}

#[test]
fn keyboard_events_go_to_the_focused_element_else_the_body() {
    let mut view = page(&format!(
        "{FIELDS}<script>globalThis.log=[];document.addEventListener('keydown',e=>log.push(e.target.id||e.target.tagName));</script>"
    ));
    view.key(key("x", Code::KeyX));
    assert_eq!(
        log(&mut view),
        r#"["BODY"]"#,
        "nothing focused: the body, not <html>"
    );
    view.click(20.0, 40.0);
    view.key(key("x", Code::KeyX));
    assert_eq!(log(&mut view), r#"["b"]"#);
}

// ---- focus

#[test]
fn focus_transitions_have_one_order_trusted_related_targets_and_a_single_owner() {
    let mut view = page(&format!("{FIELDS}{SPY}"));
    view.click(20.0, 10.0);
    assert_eq!(
        log(&mut view),
        r#"["a:mousedown:true:null","a:focus:true:null","a:focusin:true:null","a:click:true:null"]"#
    );
    view.click(20.0, 40.0);
    assert_eq!(
        log(&mut view),
        r#"["b:mousedown:true:null","a:blur:true:b","a:focusout:true:b","b:focus:true:a","b:focusin:true:a","b:click:true:null"]"#
    );
    // Script-initiated focus uses the same sequence (it used to dispatch only `focus`).
    view.evaluate("document.getElementById('a').focus()")
        .unwrap();
    assert_eq!(
        log(&mut view),
        r#"["b:blur:true:a","b:focusout:true:a","a:focus:true:b","a:focusin:true:b"]"#
    );
    view.evaluate("document.getElementById('a').focus()")
        .unwrap();
    assert_eq!(
        log(&mut view),
        "[]",
        "focusing the focused element dispatches nothing"
    );
    // Tab (Blitz default action) goes through the same path.
    view.key(key("Tab", Code::Tab));
    assert_eq!(
        log(&mut view),
        r#"["a:keydown:true:undefined","a:blur:true:b","a:focusout:true:b","b:focus:true:a","b:focusin:true:a"]"#
    );
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"b\""
    );
}

#[test]
fn blur_clears_the_active_element_and_a_fresh_document_has_the_body_active() {
    let mut view = page(&format!("{FIELDS}{SPY}"));
    assert_eq!(
        view.evaluate_json("document.activeElement.tagName")
            .unwrap(),
        "\"BODY\"",
        "it used to be HTML"
    );
    view.evaluate("document.getElementById('a').focus()")
        .unwrap();
    log(&mut view);
    view.evaluate("document.getElementById('a').blur()")
        .unwrap();
    assert_eq!(
        log(&mut view),
        r#"["a:blur:true:null","a:focusout:true:null"]"#
    );
    assert_eq!(
        view.evaluate_json("document.activeElement.tagName")
            .unwrap(),
        "\"BODY\"",
        "blur() used to leave the element active"
    );
    view.evaluate("document.getElementById('b').blur()")
        .unwrap();
    assert_eq!(
        log(&mut view),
        "[]",
        "blurring an element that is not focused does nothing"
    );
}

#[test]
fn clicking_a_non_focusable_element_blurs_the_focused_one() {
    // Blitz's pointer-down default clears focus; the engine now reports it as blur/focusout.
    let mut view = page(&format!("{FIELDS}{SPY}"));
    view.click(20.0, 10.0);
    log(&mut view);
    view.click(20.0, 100.0);
    assert_eq!(
        log(&mut view),
        r#"["a:blur:true:null","a:focusout:true:null"]"#
    );
    assert_eq!(
        view.evaluate_json("document.activeElement.tagName")
            .unwrap(),
        "\"BODY\""
    );
}

// ---- identity

#[test]
fn stale_ids_removed_nodes_and_recreated_nodes_do_not_alias() {
    let mut view = page(
        "<style>body{margin:0}#t{width:100px;height:30px}</style><div id=t></div><script>globalThis.log=[];\
         globalThis.old=document.getElementById('t');old.addEventListener('click',()=>log.push('old'));</script>",
    );
    let stale = view
        .evaluate_json("String(__wega_nodeId?.(old) ?? '')")
        .ok();
    let _ = stale;
    // Remove the node and put a new one, with the same id attribute, at the same spot.
    view.evaluate("old.remove();globalThis.fresh=document.createElement('div');fresh.id='t';fresh.style.height='30px';fresh.addEventListener('click',()=>log.push('fresh'));document.body.appendChild(fresh)").unwrap();
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    view.click(10.0, 10.0);
    assert_eq!(
        log(&mut view),
        r#"["fresh"]"#,
        "the removed node's listener does not run for the new node"
    );
    assert_eq!(
        view.evaluate_json(
            "old.parentNode===null&&old!==fresh&&document.getElementById('t')===fresh"
        )
        .unwrap(),
        "true"
    );
}

#[test]
fn native_events_for_dropped_nodes_are_ignored_without_error() {
    // BUG fixed this round: __wega_node threw "Node no longer exists" for a dropped node, so
    // an event aimed at it raised a script error and the event was lost noisily.
    let mut view = page(
        "<div id=gone></div><script>globalThis.log=[];document.addEventListener('click',()=>log.push('doc'));</script>",
    );
    let id = view
        .evaluate_json("document.getElementById('gone')._id")
        .unwrap();
    let id: String = serde_json::from_str(&id).unwrap();
    // Dropping the node needs the wrapper released too; a never-allocated key is the same case.
    for stale in [id.as_str(), "18446744073709551615", "0"] {
        let call = format!("__wega_native_event('click','{stale}',{{}})");
        let result = view.evaluate_json(&call);
        assert!(result.is_ok(), "{stale}: {result:?}");
    }
    assert_eq!(
        view.evaluate_json("__wega_native_event('click','18446744073709551615',{})")
            .unwrap(),
        "false"
    );
    assert_eq!(
        view.evaluate_json("__wega_node('18446744073709551615')")
            .unwrap(),
        "null"
    );
}

// ---- shadow DOM

const SHADOW_PAGE: &str = "<style>body{margin:0}#host{display:block;width:100px;height:30px}</style>\
    <div id=host><span id=light slot=s>L</span></div>";
const SHADOW_SETUP: &str = r#"const host=document.getElementById('host');const root=host.attachShadow({mode:'open'});
    root.innerHTML='<div id=wrap><button id=btn>b</button><slot name=s id=slot></slot></div>';
    const btn=root.getElementById('btn'),slot=root.getElementById('slot'),light=document.getElementById('light');
    const log=[];const rec=n=>e=>log.push(n+':'+(e.target.id||e.target.nodeName)+':'+e.eventPhase+':'+e.composedPath().map(x=>x.id||x.nodeName||'win').join('>'));
    document.addEventListener('t',rec('doc'));host.addEventListener('t',rec('host'));root.addEventListener('t',rec('root'));btn.addEventListener('t',rec('btn'));"#;

#[test]
fn shadow_dom_composed_and_non_composed_events_retarget_at_the_host() {
    let mut view = page(SHADOW_PAGE);
    let r = eval(
        &mut view,
        &format!(
            r#"(()=>{{{SHADOW_SETUP}
        btn.dispatchEvent(new Event('t',{{bubbles:true,composed:true}}));const composed=log.splice(0);
        btn.dispatchEvent(new Event('t',{{bubbles:true,composed:false}}));
        return {{composed,nonComposed:log.splice(0)}};}})()"#
        ),
    );
    let composed = r["composed"].as_array().unwrap();
    assert_eq!(composed.len(), 4);
    assert!(
        composed[0]
            .as_str()
            .unwrap()
            .starts_with("btn:btn:2:btn>wrap>#document-fragment>host>BODY>HTML>#document>win")
    );
    assert!(
        composed[1].as_str().unwrap().starts_with("root:btn:3:"),
        "inside the shadow tree the target is the button"
    );
    assert!(
        composed[2].as_str().unwrap().starts_with("host:host:2:"),
        "at the host the target is retargeted to the host, at-target phase"
    );
    assert!(
        composed[3].as_str().unwrap().starts_with("doc:host:3:"),
        "the document sees the host"
    );
    assert_eq!(
        r["nonComposed"],
        serde_json::json!([
            "btn:btn:2:btn>wrap>#document-fragment",
            "root:btn:3:btn>wrap>#document-fragment"
        ]),
        "a non-composed event stops at the shadow root"
    );
}

#[test]
fn shadow_dom_slotted_events_pass_through_the_slot_and_closed_roots_hide_internals() {
    let mut view = page(SHADOW_PAGE);
    let r = eval(
        &mut view,
        &format!(
            r#"(()=>{{{SHADOW_SETUP}
        light.addEventListener('t',rec('light'));slot.addEventListener('t',rec('slot'));
        light.dispatchEvent(new Event('t',{{bubbles:true,composed:true}}));const slotted=log.splice(0);
        return {{slotted,assigned:[slot.assignedNodes().map(n=>n.id),light.assignedSlot.id],
          identity:[host.shadowRoot===root,root.host===host,btn.getRootNode()===root,light.getRootNode()===document]}};}})()"#
        ),
    );
    let slotted = r["slotted"].as_array().unwrap();
    assert!(
        slotted[0]
            .as_str()
            .unwrap()
            .starts_with("light:light:2:light>slot>wrap>#document-fragment>host>"),
        "{slotted:?}"
    );
    assert_eq!(slotted.len(), 5, "light, slot, root, host, doc");
    assert_eq!(r["assigned"], serde_json::json!([["light"], "slot"]));
    assert_eq!(r["identity"], serde_json::json!([true, true, true, true]));
    let mut closed = page("<div id=host></div>");
    let r = eval(
        &mut closed,
        r#"(()=>{const host=document.getElementById('host');const root=host.attachShadow({mode:'closed'});
        root.innerHTML='<button id=btn>b</button>';let path=null,target=null;
        host.addEventListener('t',e=>{path=e.composedPath().map(n=>n.id||n.nodeName);target=e.target.id});
        root.getElementById('btn').dispatchEvent(new Event('t',{bubbles:true,composed:true}));
        return {path,target,shadowRoot:host.shadowRoot};})()"#,
    );
    assert_eq!(r["target"], "host");
    assert!(
        !r["path"].to_string().contains("btn"),
        "closed mode hides shadow internals: {}",
        r["path"]
    );
    assert_eq!(r["shadowRoot"], serde_json::Value::Null);
}

#[test]
fn shadow_dom_focus_retargets_active_element_and_focusin_crosses_the_boundary() {
    let mut view = page(SHADOW_PAGE);
    let r = eval(
        &mut view,
        &format!(
            r#"(()=>{{{SHADOW_SETUP}
        const f=[];host.addEventListener('focusin',e=>f.push('host:'+e.target.id));root.addEventListener('focusin',e=>f.push('root:'+e.target.id));
        btn.focus();
        const inside=[document.activeElement.id,root.activeElement&&root.activeElement.id];
        light.focus();
        return {{inside,f,afterLight:[document.activeElement.id,root.activeElement]}};}})()"#
        ),
    );
    assert_eq!(
        r["inside"],
        serde_json::json!(["host", "btn"]),
        "document.activeElement is the host; shadowRoot.activeElement is the inner element"
    );
    assert_eq!(
        r["f"],
        serde_json::json!(["root:btn", "host:host", "root:light", "host:light"])
    );
    assert_eq!(r["afterLight"], serde_json::json!(["light", null]));
}

#[test]
fn detached_nodes_dispatch_to_their_own_tree_only() {
    let mut view = page("<div id=x></div>");
    let r = eval(
        &mut view,
        r#"(()=>{const log=[];const d=document.createElement('div');d.addEventListener('t',()=>log.push('detached'));
        document.addEventListener('t',()=>log.push('document'));
        const child=document.createElement('i');d.appendChild(child);child.dispatchEvent(new Event('t',{bubbles:true}));return log;})()"#,
    );
    assert_eq!(r, serde_json::json!(["detached"]));
}

// ---- document lifecycle

#[test]
fn replacing_the_document_drops_listeners_focus_and_old_state() {
    let mut view = page(&format!(
        "{FIELDS}{SPY}<script>window.addEventListener('keydown',()=>log.push('window'));</script>"
    ));
    view.click(20.0, 10.0);
    log(&mut view);
    view.set_html("<style>body{margin:0}input{display:block;width:100px;height:30px;margin:0}</style><input id=a><script>globalThis.fresh=[];document.addEventListener('keydown',e=>fresh.push(e.target.tagName));</script>").unwrap();
    view.poll();
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    assert_eq!(
        view.evaluate_json("typeof log").unwrap(),
        "\"undefined\"",
        "script state of the old document is gone"
    );
    assert_eq!(
        view.evaluate_json("document.activeElement.tagName")
            .unwrap(),
        "\"BODY\"",
        "focus is reset"
    );
    view.key(key("x", Code::KeyX));
    assert_eq!(
        view.evaluate_json("JSON.stringify(fresh)").unwrap(),
        "\"[\\\"BODY\\\"]\"",
        "only the new document's listeners run"
    );
    view.click(20.0, 10.0);
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"a\"",
        "input routing works on the new document"
    );
}

#[test]
fn navigation_reload_and_error_pages_replace_the_document_cleanly() {
    use support::{Reply, Server, engine, open, settle};
    let server = Server::new(|seen| match seen.path.as_str() {
        "/two" => {
            Reply::html("<title>two</title><input id=two><script>globalThis.marker='two'</script>")
        }
        _ => Reply::html(
            "<title>one</title><input id=one><script>globalThis.marker='one';document.addEventListener('keydown',()=>globalThis.hits=(globalThis.hits||0)+1)</script>",
        ),
    });
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    view.evaluate("document.getElementById('one').focus()")
        .unwrap();
    view.key(key("x", Code::KeyX));
    assert_eq!(view.evaluate_json("hits").unwrap(), "1");
    // Navigation.
    view.load(Url::parse(&server.url("127.0.0.1", "/two")).unwrap())
        .unwrap();
    settle(&mut view);
    assert_eq!(view.evaluate_json("marker").unwrap(), "\"two\"");
    assert_eq!(
        view.evaluate_json("typeof hits").unwrap(),
        "\"undefined\"",
        "old listeners and state are gone"
    );
    assert_eq!(
        view.evaluate_json("document.activeElement.tagName")
            .unwrap(),
        "\"BODY\""
    );
    // Reload of the first page: a fresh document, not the old one revived.
    view.load(Url::parse(&server.url("127.0.0.1", "/")).unwrap())
        .unwrap();
    settle(&mut view);
    view.key(key("x", Code::KeyX));
    assert_eq!(
        view.evaluate_json("hits").unwrap(),
        "1",
        "one keydown on the reloaded document, not two"
    );
    // Error page: a failed load is replaced by a normal, usable document.
    view.load(Url::parse("http://127.0.0.1:1/").unwrap()).ok();
    settle(&mut view);
    assert_eq!(view.evaluate_json("typeof hits").unwrap(), "\"undefined\"");
    view.set_html("<input id=again>").unwrap();
    view.poll();
    view.evaluate("document.getElementById('again').focus()")
        .unwrap();
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"again\""
    );
}

#[test]
fn replacing_a_document_mid_parse_tears_the_parser_down() {
    let mut view = page("<p>first</p>");
    // A document whose script is still waiting for its source never finishes parsing; replacing
    // it must drop the pending parser and scripts, and the replacement must complete normally.
    view.set_html("<p id=slow>before</p><script src='http://127.0.0.1:9/never.js'></script><p id=after>after</p>").unwrap();
    view.set_html("<p id=ok>fine</p><script>globalThis.done=document.getElementById('ok').textContent</script>").unwrap();
    view.poll();
    assert_eq!(view.evaluate_json("done").unwrap(), "\"fine\"");
    assert_eq!(
        view.evaluate_json(
            "document.getElementById('after')===null&&document.getElementById('slow')===null"
        )
        .unwrap(),
        "true"
    );
}
