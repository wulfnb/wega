mod support;
use wega_dom::DocumentView;
use wega_engine::{RenderingBackend, View};
fn tree(view: &View) -> accesskit::TreeUpdate {
    serde_json::from_str(
        &view
            .accessibility_tree()
            .expect("native accessibility tree"),
    )
    .unwrap()
}
fn id(view: &View, name: &str) -> accesskit::NodeId {
    accesskit::NodeId(
        view.native_document
            .as_ref()
            .unwrap()
            .borrow()
            .doc
            .borrow()
            .element_by_id(name)
            .unwrap()
            .as_u64(),
    )
}
fn node(tree: &accesskit::TreeUpdate, id: accesskit::NodeId) -> &accesskit::Node {
    &tree.nodes.iter().find(|(k, _)| *k == id).unwrap().1
}
#[test]
fn native_accessibility_roles_names_labels_values_focus_and_geometry() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            "<style>body{margin:0}input{display:block;width:160px;height:30px}</style><label for=email>Email address</label><input id=email type=email value=user@example.test><label>Password<input id=password type=password value=s3cr3t></label><button id=button aria-label='Send form'>Go</button><input id=check type=checkbox checked disabled><select id=select><option value=a>A</option><option value=b selected>B</option></select><span id=label>Read guide</span><a id=link href=/guide aria-labelledby=label>Link</a><div aria-hidden=true><button id=hidden>Hidden control</button></div>",
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.paint(400, 300, 1.0, 1.0, false).unwrap();
    let email = id(&view, "email");
    let password = id(&view, "password");
    let button = id(&view, "button");
    let check = id(&view, "check");
    let link = id(&view, "link");
    let hidden = id(&view, "hidden");
    let t = tree(&view);
    assert_eq!(node(&t, email).role(), accesskit::Role::EmailInput);
    assert_eq!(node(&t, email).label(), Some("Email address"));
    assert_eq!(node(&t, email).value(), Some("user@example.test"));
    assert_eq!(node(&t, email).bounds().unwrap().width(), 160.0);
    assert_eq!(node(&t, password).role(), accesskit::Role::PasswordInput);
    assert_eq!(node(&t, password).label(), Some("Password"));
    assert_eq!(node(&t, password).value(), Some("••••••"));
    assert!(!serde_json::to_string(&t).unwrap().contains("s3cr3t"));
    assert_eq!(node(&t, button).label(), Some("Send form"));
    assert_eq!(node(&t, check).toggled(), Some(accesskit::Toggled::True));
    assert!(node(&t, check).is_disabled());
    assert_eq!(node(&t, link).label(), Some("Read guide"));
    assert!(!t.nodes.iter().any(|(id, _)| *id == hidden));
    view.evaluate("document.getElementById('email').focus();document.getElementById('email').value='changed@example.test'").unwrap();
    let t = tree(&view);
    assert_eq!(t.focus, email);
    assert_eq!(node(&t, email).value(), Some("changed@example.test"));
    view.accessibility_action("focus", button.0, None);
    assert_eq!(tree(&view).focus, button);
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"button\""
    );
    view.evaluate("document.getElementById('check').checked=false;document.getElementById('select').value='a'").unwrap();
    let t = tree(&view);
    assert_eq!(node(&t, check).toggled(), Some(accesskit::Toggled::False));
    assert_eq!(node(&t, id(&view, "select")).value(), Some("a"));
    view.evaluate("document.getElementById('email').style.width='210px'")
        .unwrap();
    let t = tree(&view);
    assert_eq!(node(&t, email).bounds().unwrap().width(), 210.0);
}
#[test]
fn cached_geometry_invalidates_for_stylesheets_viewport_mutations_and_nested_scroll() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            "<style id=s>body{margin:0}#target{width:80px;height:40px;background:red}@media(min-width:500px){#target{width:100px}}</style><div id=scroller style='height:100px;width:200px;overflow:auto'><div style='height:60px'></div><div id=target></div><div style='height:500px'></div></div>",
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.paint(400, 300, 1.0, 1.0, false).unwrap();
    let rect = "(()=>{let r=document.getElementById('target').getBoundingClientRect();return [r.x,r.y,r.width,r.height]})()";
    assert_eq!(view.evaluate_json(rect).unwrap(), "[0,60,80,40]");
    assert_eq!(view.evaluate_json(rect).unwrap(), "[0,60,80,40]");
    view.evaluate("document.getElementById('scroller').scrollTop=40")
        .unwrap();
    assert_eq!(view.evaluate_json(rect).unwrap(), "[0,20,80,40]");
    view.evaluate("document.getElementById('s').sheet.insertRule('#target{height:50px}',2)")
        .unwrap();
    assert_eq!(view.evaluate_json(rect).unwrap(), "[0,20,80,50]");
    view.paint(600, 300, 1.0, 1.0, false).unwrap();
    assert_eq!(view.evaluate_json(rect).unwrap(), "[0,20,100,50]");
    view.evaluate("document.getElementById('s').media='print'")
        .unwrap();
    assert_ne!(view.evaluate_json(rect).unwrap(), "[0,20,100,50]");
}
