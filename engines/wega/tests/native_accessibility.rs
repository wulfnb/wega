mod support;
use wega_dom::DocumentView;
use wega_engine::events::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
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
    // Chrome: 160px content + 2px padding + 2px border per side (content-box).
    assert_eq!(node(&t, email).bounds().unwrap().width(), 168.0);
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
    assert_eq!(node(&t, email).bounds().unwrap().width(), 218.0); // 210 content + 8 padding/border
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

#[test]
fn native_accessibility_semantic_tags_aria_roles_and_states() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            r#"<style>body{margin:0}div,header,footer,aside,article,section,dialog{display:block}</style>
            <header id=hdr>Site Header</header>
            <footer id=ftr>Site Footer</footer>
            <aside id=asd>Related Notes</aside>
            <article id=art>Main Article</article>
            <section id=sec>Content Section</section>
            <h1 id=heading1>Title</h1>
            <h3 id=heading3>Subheading</h3>
            <div id=custom_heading role="heading" aria-level="2">Level 2 Heading</div>
            <div id=live_alert role="alert" aria-live="assertive">Alert Message</div>
            <dialog id=modal_dlg aria-modal="true">Modal Dialog</dialog>
            <div id=tab_list role="tablist">
                <button id=tab1 role="tab" aria-selected="true" aria-expanded="true">Tab 1</button>
                <button id=tab2 role="tab" aria-selected="false">Tab 2</button>
            </div>
            <div id=tab_panel role="tabpanel" aria-description="First panel content">Panel 1</div>
            <input id=req_input required aria-required="true" aria-invalid="spelling" value="badtext">
            <input id=ro_input readonly aria-readonly="true" value="readonly">
            <div id=custom_slider role="slider" aria-valuenow="42" aria-valuemin="10" aria-valuemax="100">42</div>
            <input id=switch_ctrl role="switch" aria-checked="true">
            <div inert id=inert_container>
                <button id=inert_button>Should Not Be in A11y Tree</button>
            </div>
            "#,
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.paint(600, 400, 1.0, 1.0, false).unwrap();

    let t = tree(&view);
    // Semantic tags
    let hdr = id(&view, "hdr");
    let ftr = id(&view, "ftr");
    let asd = id(&view, "asd");
    let art = id(&view, "art");
    let sec = id(&view, "sec");
    assert_eq!(node(&t, hdr).role(), accesskit::Role::Header);
    assert_eq!(node(&t, ftr).role(), accesskit::Role::Footer);
    assert_eq!(node(&t, asd).role(), accesskit::Role::Complementary);
    assert_eq!(node(&t, art).role(), accesskit::Role::Article);
    assert_eq!(node(&t, sec).role(), accesskit::Role::Region);

    // Headings and levels (accesskit level is 0-based: h1 -> 0, h2 -> 1, h3 -> 2)
    let h1 = id(&view, "heading1");
    let h3 = id(&view, "heading3");
    let h_custom = id(&view, "custom_heading");
    assert_eq!(node(&t, h1).role(), accesskit::Role::Heading);
    assert_eq!(node(&t, h1).level(), Some(0));
    assert_eq!(node(&t, h3).role(), accesskit::Role::Heading);
    assert_eq!(node(&t, h3).level(), Some(2));
    assert_eq!(node(&t, h_custom).role(), accesskit::Role::Heading);
    assert_eq!(node(&t, h_custom).level(), Some(1));

    // Alert and live region
    let alert = id(&view, "live_alert");
    assert_eq!(node(&t, alert).role(), accesskit::Role::Alert);
    assert_eq!(node(&t, alert).live(), Some(accesskit::Live::Assertive));

    // Dialog & modal
    let dlg = id(&view, "modal_dlg");
    assert_eq!(node(&t, dlg).role(), accesskit::Role::Dialog);
    assert!(node(&t, dlg).is_modal());

    // Tab, selected, expanded, description
    let tab1 = id(&view, "tab1");
    let tab2 = id(&view, "tab2");
    let panel = id(&view, "tab_panel");
    assert_eq!(node(&t, tab1).role(), accesskit::Role::Tab);
    assert_eq!(node(&t, tab1).is_selected(), Some(true));
    assert_eq!(node(&t, tab1).is_expanded(), Some(true));
    assert_eq!(node(&t, tab2).role(), accesskit::Role::Tab);
    assert_eq!(node(&t, tab2).is_selected(), Some(false));
    assert_eq!(node(&t, panel).description(), Some("First panel content"));

    // Form states: required, readonly, invalid
    let req = id(&view, "req_input");
    let ro = id(&view, "ro_input");
    assert!(node(&t, req).is_required());
    assert_eq!(node(&t, req).invalid(), Some(accesskit::Invalid::Spelling));
    assert!(node(&t, ro).is_read_only());

    // Slider and numeric values
    let slider = id(&view, "custom_slider");
    assert_eq!(node(&t, slider).role(), accesskit::Role::Slider);
    assert_eq!(node(&t, slider).numeric_value(), Some(42.0));
    assert_eq!(node(&t, slider).min_numeric_value(), Some(10.0));
    assert_eq!(node(&t, slider).max_numeric_value(), Some(100.0));

    // Switch and toggled
    let sw = id(&view, "switch_ctrl");
    assert_eq!(node(&t, sw).role(), accesskit::Role::Switch);
    assert_eq!(node(&t, sw).toggled(), Some(accesskit::Toggled::True));

    // Inert subtree exclusion: neither inert_container nor inert_button should be present
    let inert_btn = id(&view, "inert_button");
    let inert_cont = id(&view, "inert_container");
    assert!(!t.nodes.iter().any(|(id, _)| *id == inert_btn));
    assert!(!t.nodes.iter().any(|(id, _)| *id == inert_cont));
}

#[test]
fn keyboard_navigation_tab_order_and_focus_restoration() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            r#"<style>body{margin:0}input{display:block;height:20px;margin:5px}</style>
            <input id=inp_first tabindex="2" value="first">
            <input id=inp_second tabindex="1" value="second">
            <div inert><input id=inp_inert tabindex="3" value="inert"></div>
            <input id=inp_third value="third">
            "#,
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/page"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.paint(400, 300, 1.0, 1.0, false).unwrap();

    // Verify focus order from DOM document
    {
        let native = view.native_document.as_ref().unwrap().borrow();
        let doc = native.doc.borrow();
        let order = doc.focus_order();
        let second = doc.element_by_id("inp_second").unwrap();
        let first = doc.element_by_id("inp_first").unwrap();
        let third = doc.element_by_id("inp_third").unwrap();
        let inert = doc.element_by_id("inp_inert").unwrap();

        assert_eq!(order[0], second);
        assert_eq!(order[1], first);
        assert_eq!(order[2], third);
        assert!(!order.contains(&inert));
    }

    // Step 1: Send Tab key -> should focus inp_second (tabindex 1)
    let tab_down = KeyEvent::new(Key::Named(NamedKey::Tab), Code::Tab, KeyState::Pressed);
    view.key(tab_down.clone());
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"inp_second\""
    );

    // Step 2: Send Tab key again -> should focus inp_first (tabindex 2)
    view.key(tab_down.clone());
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"inp_first\""
    );

    // Step 3: Send Tab key again -> should focus inp_third (default tabindex)
    view.key(tab_down.clone());
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"inp_third\""
    );

    // Step 4: Shift+Tab -> should reverse back to inp_first
    let shift_tab = KeyEvent::new(Key::Named(NamedKey::Tab), Code::Tab, KeyState::Pressed)
        .with_modifiers(Modifiers::SHIFT);
    view.key(shift_tab);
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"inp_first\""
    );

    // Step 5: Focus restoration after DOM mutation
    // Remove currently focused element ('inp_first')
    view.evaluate("document.getElementById('inp_first').remove()")
        .unwrap();

    // Prune stale caches / re-layout
    view.paint(400, 300, 1.0, 1.0, false).unwrap();

    // After removing focused element, focus is pruned from document.
    // Pressing Tab smoothly starts focus sequence on inp_second:
    view.key(tab_down.clone());
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"inp_second\""
    );

    // Next Tab focuses inp_third:
    view.key(tab_down);
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"inp_third\""
    );
}
