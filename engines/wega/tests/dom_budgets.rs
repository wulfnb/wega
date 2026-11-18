//! Mutation accounting: every way script can add nodes or text is bounded by the document's
//! budgets, and a native (inspector) edit charges the *same* counters.
use std::sync::Arc;
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

const NODES: usize = 300;
const TEXT: usize = 64 * 1024;

fn limits() -> Limits {
    Limits {
        dom_nodes: NODES,
        dom_text_bytes: TEXT,
        ..Default::default()
    }
}
fn page(html: &str) -> View {
    let mut view = Engine::new(
        limits(),
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .create_view(Url::parse("about:blank").unwrap())
    .unwrap();
    view.stop();
    view.url = Url::parse("https://fixture.test/budget.html").unwrap();
    view.set_html(html).unwrap();
    for _ in 0..6 {
        view.poll();
    }
    view
}
fn blank() -> View {
    page("<!doctype html><body></body>")
}
fn fill_leaving(view: &mut View, spare: usize) {
    let fill = NODES - view.node_count() - spare;
    view.evaluate_json(&format!(
        "for(let i=0;i<{fill};i++)document.body.appendChild(document.createElement('i'));true"
    ))
    .expect("filling within the budget must succeed");
}

/// Every mutating route a script has, each driven well past the limits.
const ROUTES: &[(&str, &str)] = &[
    (
        "createElement",
        "for(let i=0;i<5000;i++)document.body.appendChild(document.createElement('i'))",
    ),
    (
        "createElementNS",
        "for(let i=0;i<5000;i++)document.body.appendChild(document.createElementNS('http://www.w3.org/2000/svg','g'))",
    ),
    (
        "createTextNode",
        "for(let i=0;i<5000;i++)document.body.appendChild(document.createTextNode('t'))",
    ),
    (
        "createComment",
        "for(let i=0;i<5000;i++)document.body.appendChild(document.createComment('c'))",
    ),
    (
        "createDocumentFragment",
        "for(let i=0;i<5000;i++)document.body.appendChild(document.createDocumentFragment())",
    ),
    (
        "cloneNode(deep)",
        "const s=document.createElement('div');for(let i=0;i<60;i++)s.appendChild(document.createElement('b'));document.body.appendChild(s);for(let i=0;i<50;i++)document.body.appendChild(s.cloneNode(true))",
    ),
    (
        "innerHTML (nodes)",
        "document.body.innerHTML='<i></i>'.repeat(5000)",
    ),
    (
        "innerHTML (text)",
        "document.body.innerHTML='x'.repeat(200000)",
    ),
    (
        "insertAdjacentHTML",
        "document.body.insertAdjacentHTML('beforeend','<i></i>'.repeat(5000))",
    ),
    (
        "textContent",
        "document.body.textContent='x'.repeat(200000)",
    ),
    (
        "text node data",
        "const t=document.body.appendChild(document.createTextNode('a'));t.data='x'.repeat(200000)",
    ),
    (
        "setAttribute (many)",
        "for(let i=0;i<5000;i++)document.body.setAttribute('data-'+i,'v'.repeat(100))",
    ),
    (
        "setAttribute (huge)",
        "document.body.setAttribute('data-x','v'.repeat(200000))",
    ),
    (
        "setAttributeNS",
        "for(let i=0;i<5000;i++)document.body.setAttributeNS('http://www.w3.org/1999/xlink','xlink:h'+i,'v'.repeat(100))",
    ),
    (
        "style.setProperty",
        "for(let i=0;i<5000;i++)document.body.style.setProperty('--v'+i,'x'.repeat(100))",
    ),
    (
        "document.write",
        "document.open();document.write('<i></i>'.repeat(5000));document.close()",
    ),
    (
        "attachShadow",
        "for(let i=0;i<5000;i++)document.body.appendChild(document.createElement('div')).attachShadow({mode:'open'})",
    ),
    (
        "custom element construction",
        "customElements.define('x-budget',class extends HTMLElement{});for(let i=0;i<5000;i++)document.body.appendChild(new (customElements.get('x-budget'))())",
    ),
];

#[test]
fn every_script_route_that_adds_nodes_or_text_is_bounded() {
    for (name, script) in ROUTES {
        // `document.write` feeds the HTML parser, which stops at the node limit instead of
        // throwing; every other route must fail with a budget error.
        let truncates = *name == "document.write";
        let mut view = blank();
        match view.evaluate_json(&format!("{script};true")) {
            Err(error) => assert!(
                error.contains("limit") || error.contains("budget") || error.contains("exceeded"),
                "{name}: failed, but not with a budget error: {error}"
            ),
            Ok(_) => assert!(truncates, "{name}: unbounded growth was accepted"),
        }
        assert!(
            view.node_count() <= NODES + 8,
            "{name}: {} nodes for a budget of {NODES}",
            view.node_count()
        );
        assert!(
            view.evaluate_json("1+1").is_ok(),
            "{name}: the page must stay usable"
        );
    }
}

#[test]
fn template_contents_are_charged_by_the_shared_node_budget() {
    let mut view = blank();
    view.evaluate_json(
        "for(let i=0;i<140;i++){const t=document.createElement('template');document.body.appendChild(t);t.content}true",
    ).unwrap();
    let counted = view.node_count();
    assert_eq!(
        view.dom_budget().unwrap().0,
        counted,
        "every content fragment must be charged exactly once"
    );
    assert!(
        view.reserve_dom_budget(NODES - counted + 1, 0, 1).is_err(),
        "native edits must not spend an uncharged template allowance"
    );
}

#[test]
fn template_fragments_and_deep_clones_charge_every_owned_descendant() {
    let mut view = blank();
    view.evaluate("document.body.innerHTML='<template id=t><i>value</i></template>'")
        .unwrap();
    let before = view.node_count();
    assert_eq!(view.dom_budget().unwrap().0, before);
    view.evaluate("document.body.appendChild(document.getElementById('t').cloneNode(true))")
        .unwrap();
    assert_eq!(
        view.node_count(),
        before + 4,
        "template, fragment, element and text each have an identity"
    );
    assert_eq!(view.dom_budget().unwrap().0, view.node_count());
    assert_eq!(
        view.evaluate_json("document.body.lastChild.content.firstChild.textContent")
            .unwrap(),
        "\"value\""
    );
    assert!(
        view.evaluate("document.body.innerHTML='<template><i></i></template>'.repeat(150)")
            .is_err()
    );
    assert_eq!(
        view.node_count(),
        before + 4,
        "a rejected fragment parse must leave the live tree intact"
    );
    assert_eq!(view.dom_budget().unwrap().0, view.node_count());
}

#[test]
fn parser_template_ownership_counts_toward_the_existing_limit_before_scripts() {
    let mut view = page(&format!(
        "{}<script>globalThis.afterTemplateLimit=true</script>",
        "<template><i></i></template>".repeat(100)
    ));
    assert!(view.node_count() <= NODES + 8);
    assert_eq!(
        view.evaluate_json("globalThis.afterTemplateLimit ?? false")
            .unwrap(),
        "false",
        "code following an over-budget tree must not run"
    );
}

#[test]
fn known_gap_script_nesting_depth_has_no_budget() {
    // The engine bounds node count and text, never depth: 250 nested elements are accepted.
    // Native edits are held to `MAX_EDIT_DEPTH` instead. Pinned so that adding a script depth
    // limit is noticed.
    let mut view = blank();
    view.evaluate_json(
        "let n=document.body;for(let i=0;i<250;i++)n=n.appendChild(document.createElement('div'));true",
    )
    .expect("script depth is unbounded today");
    assert!(view.node_count() >= 250);
}

#[test]
fn native_edits_share_the_scripts_counters() {
    let view = blank();
    let spare = NODES - view.node_count();
    // A native edit reserves nodes; script then has correspondingly less room.
    view.reserve_dom_budget(spare - 20, 0, 3).unwrap();
    let mut view = view;
    let script = "for(let i=0;i<40;i++)document.body.appendChild(document.createElement('i'));true";
    let error = view
        .evaluate_json(script)
        .expect_err("only ~20 nodes are left for script");
    assert!(
        error.contains("budget") || error.contains("limit"),
        "{error}"
    );
    // Without the reservation the same script fits.
    assert!(blank().evaluate_json(script).is_ok());
    // The other way round: script fills the document, a native edit cannot add to it.
    let mut full = blank();
    fill_leaving(&mut full, 30);
    assert_eq!(
        full.reserve_dom_budget(40, 0, 3).unwrap_err(),
        "DOM node limit exceeded"
    );
    assert!(full.reserve_dom_budget(20, 0, 3).is_ok());
}

#[test]
fn native_edit_reservations_check_nodes_text_and_depth_and_fail_without_side_effects() {
    let mut view = blank();
    assert_eq!(
        view.reserve_dom_budget(NODES + 1, 0, 1).unwrap_err(),
        "DOM node limit exceeded"
    );
    assert_eq!(
        view.reserve_dom_budget(0, TEXT + 1, 1).unwrap_err(),
        "DOM text allocation budget exceeded"
    );
    assert_eq!(
        view.reserve_dom_budget(1, 0, 122).unwrap_err(),
        "DOM nesting limit exceeded"
    );
    assert_eq!(
        view.reserve_dom_budget(1, 0, usize::MAX).unwrap_err(),
        "DOM nesting limit exceeded"
    );
    assert_eq!(
        view.reserve_dom_budget(usize::MAX, 0, 1).unwrap_err(),
        "DOM node limit exceeded",
        "no overflow"
    );
    assert_eq!(
        view.reserve_dom_budget(0, usize::MAX, 1).unwrap_err(),
        "DOM text allocation budget exceeded",
        "no overflow"
    );
    // Refusals left nothing reserved: the budget is still fully available to script.
    fill_leaving(&mut view, 15);
    // The deepest permitted nesting is accepted, one more is not.
    let fresh = blank();
    fresh.reserve_dom_budget(1, 0, 121).unwrap();
    assert!(fresh.reserve_dom_budget(1, 0, 122).is_err());
    // A text failure after a node success does not keep the nodes.
    let atomic = blank();
    assert!(atomic.reserve_dom_budget(100, TEXT + 1, 1).is_err());
    atomic
        .reserve_dom_budget(NODES - atomic.node_count() - 10, 0, 1)
        .expect("the 100 nodes were not committed");
}

#[test]
fn after_a_fatal_script_stop_there_is_nothing_left_to_charge() {
    let limits = Limits {
        script_time: std::time::Duration::from_millis(200),
        ..limits()
    };
    let mut view = Engine::new(limits, NetworkPolicy::default(), Arc::new(|| {}))
        .unwrap()
        .create_view(Url::parse("about:blank").unwrap())
        .unwrap();
    view.stop();
    view.url = Url::parse("https://fixture.test/budget.html").unwrap();
    view.set_html("<!doctype html><body></body>").unwrap();
    assert!(view.reserve_dom_budget(1, 0, 1).is_ok());
    let error = view
        .evaluate_json("while(true){}")
        .expect_err("the watchdog ends it");
    assert!(
        error.contains("deadline") || error.contains("Script"),
        "{error}"
    );
    assert!(view.take_script_failure().is_some());
    assert!(view.reserve_dom_budget(1, 0, 1).is_err());
}
