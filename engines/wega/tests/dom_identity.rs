//! Node identity and document lifecycle as seen from JavaScript.
use std::sync::Arc;
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

fn view(html: &str) -> View {
    let engine = Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
    let mut view = engine
        .create_view(Url::parse("https://identity.test/").unwrap())
        .unwrap();
    view.set_html(html).unwrap();
    view
}
fn js(view: &mut View, source: &str) -> String {
    view.evaluate_json(source).unwrap()
}

#[test]
fn wrappers_keep_their_identity_across_mutation_and_reparenting() {
    let mut v = view("<!doctype html><body><div id=a><p id=p>x</p></div><div id=b></div></body>");
    let out = js(
        &mut v,
        "(()=>{const p=document.getElementById('p');p.setAttribute('data-k','1');p.textContent='y';\
         document.getElementById('b').appendChild(p);\
         return JSON.stringify([p===document.getElementById('p'),p.parentNode===document.getElementById('b'),\
         document.getElementById('a').childNodes.length,p.getAttribute('data-k'),p.textContent,\
         p.getRootNode()===document])})()",
    );
    assert_eq!(out, r#""[true,true,0,\"1\",\"y\",true]""#);
}

#[test]
fn removed_nodes_stay_usable_until_dropped_and_ids_resolve_by_current_state() {
    let mut v = view("<!doctype html><body><div id=a></div></body>");
    let out = js(
        &mut v,
        "(()=>{const a=document.getElementById('a');a.remove();const gone=document.getElementById('a');\
         document.body.appendChild(a);return JSON.stringify([gone,a===document.getElementById('a'),a.isConnected])})()",
    );
    assert_eq!(out, r#""[null,true,true]""#);
}

#[test]
fn node_info_matches_the_dom_interface() {
    let mut v = view(
        "<!doctype html><body><div id=d>t<!--c--></div><svg id=s><circle id=c></circle></svg></body>",
    );
    let out = js(
        &mut v,
        "(()=>{const d=document.getElementById('d');const c=document.getElementById('c');\
         return JSON.stringify([d.nodeType,d.nodeName,d.localName,d.namespaceURI,d.firstChild.nodeType,d.firstChild.nodeName,\
         d.lastChild.nodeType,d.lastChild.nodeName,c.nodeName,c.namespaceURI,document.nodeType,document.nodeName,\
         document.createDocumentFragment().nodeType])})()",
    );
    assert_eq!(
        out,
        r##""[1,\"DIV\",\"div\",\"http://www.w3.org/1999/xhtml\",3,\"#text\",8,\"#comment\",\"circle\",\"http://www.w3.org/2000/svg\",9,\"#document\",11]""##
    );
}

#[test]
fn each_document_gets_a_fresh_identity_space() {
    let engine = Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
    let mut v = engine
        .create_view(Url::parse("https://identity.test/").unwrap())
        .unwrap();
    v.set_html("<!doctype html><body><div id=old></div></body>")
        .unwrap();
    assert_eq!(
        js(&mut v, "String(!!document.getElementById('old'))"),
        r#""true""#
    );
    v.set_html("<!doctype html><body><div id=new></div></body>")
        .unwrap();
    assert_eq!(
        js(
            &mut v,
            "JSON.stringify([document.getElementById('old'),!!document.getElementById('new')])"
        ),
        r#""[null,true]""#
    );
}
