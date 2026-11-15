use html5ever::{LocalName, QualName, ns};
use wega_adapter_blitz::{BlitzDocument, DocumentSetup, ToNodeId};
use wega_dom::{DocumentMutation, DocumentView, MutationClock, Mutator, NodeView, inspect::*};
fn doc(html: &str) -> BlitzDocument {
    BlitzDocument::from_html(html, DocumentSetup::default())
}
fn read(d: &BlitzDocument, q: StyleRead, offset: u32) -> StylePage {
    for _ in 0..5 {
        match d.read_style(&q, offset) {
            Ok(p) => return p,
            Err(StyleReadError::Timeout) => std::thread::yield_now(),
            Err(e) => panic!("{e:?}"),
        }
    }
    panic!("deadline repeatedly expired")
}
fn value(p: &StylePage, name: &str) -> String {
    p.items
        .iter()
        .find_map(|i| match i {
            StyleItem::Declaration(d) if d.name == name => Some(d.value.clone()),
            _ => None,
        })
        .unwrap()
}
#[test]
fn cached_cascade_inheritance_specificity_important_inline_and_media() {
    let mut d = doc(
        "<!doctype html><style>body{color:rgb(1,2,3)}div{width:11px}.a{width:12px}#x{width:13px}div{height:14px!important}@media(min-width:1px){#x{padding-top:7px}}</style><div id=x class=a style='height:99px;width:42px'></div>",
    );
    let x = d.element_by_id("x").unwrap();
    assert_eq!(
        read(&d, StyleRead::Computed(x), 0).availability,
        Availability::NotComputed
    );
    d.set_viewport(blitz_traits::shell::Viewport {
        window_size: (800, 600),
        hidpi_scale: 1.,
        zoom: 1.,
        color_scheme: blitz_traits::shell::ColorScheme::Light,
    });
    d.resolve(0.);
    let epoch = d.mutation_epoch();
    let p = read(&d, StyleRead::Computed(x), 0);
    assert_eq!(p.availability, Availability::Cached);
    assert_eq!(value(&p, "width"), "42px");
    assert_eq!(value(&p, "height"), "14px");
    assert_eq!(value(&p, "color"), "rgb(1, 2, 3)");
    assert_eq!(value(&p, "padding-top"), "7px");
    assert_eq!(d.mutation_epoch(), epoch);
    d.mutate()
        .set_attribute(
            x,
            QualName::new(None, ns!(), LocalName::from("style")),
            "height:99px",
        )
        .unwrap();
    let pending = read(&d, StyleRead::Computed(x), 0);
    assert!(pending.layout_pending);
    assert_eq!(
        value(&pending, "width"),
        "42px",
        "cached, no inspector flush"
    );
    d.resolve(0.);
    assert_eq!(value(&read(&d, StyleRead::Computed(x), 0), "width"), "13px");
    assert_eq!(
        read(&d, StyleRead::Matched(x), 0).availability,
        Availability::Unsupported
    );
}
#[test]
fn cssom_mutations_invalidate_paths_and_failed_mutations_do_not() {
    let mut d = doc(
        "<!doctype html><style>#x{color:red!important}@media(all){p{width:1px}}</style><div id=x></div>",
    );
    let owner = d.stylesheet_owner_nodes().next().unwrap();
    let key = d.element_by_id("x").unwrap();
    let before = d.style_generation();
    assert!(d.stylesheet_delete_rule(owner, &[], 99).is_err());
    assert_eq!(d.style_generation(), before);
    d.stylesheet_insert_rule(owner, &[], "p{color:blue}", 0)
        .unwrap();
    assert_ne!(d.style_generation(), before);
    let mut previous = d.style_generation();
    d.stylesheet_delete_rule(owner, &[], 0).unwrap();
    assert_ne!(d.style_generation(), previous);
    previous = d.style_generation();
    d.stylesheet_rule_style_set_property(owner, &[0], "color", "green", false)
        .unwrap();
    assert_ne!(d.style_generation(), previous);
    previous = d.style_generation();
    d.stylesheet_rule_set_selector_text(owner, &[0], ".a")
        .unwrap();
    assert_ne!(d.style_generation(), previous);
    previous = d.style_generation();
    d.stylesheet_rule_style_set_css_text(owner, &[0], "color:purple")
        .unwrap();
    assert_ne!(d.style_generation(), previous);
    previous = d.style_generation();
    d.stylesheet_rule_style_remove_property(owner, &[0], "color")
        .unwrap();
    assert_ne!(d.style_generation(), previous);
    previous = d.style_generation();
    d.set_stylesheet_disabled(owner, true);
    assert_ne!(d.style_generation(), previous);
    assert!(read(&d, StyleRead::Inline(key), 0).items.is_empty());
}
#[test]
fn author_sheet_rule_group_declarations_and_locations() {
    let d = doc(
        "<!doctype html><style>p{color:red!important}@media(all){p{width:1px}}</style><div></div>",
    );
    let sheets = read(&d, StyleRead::Sheets, 0);
    let StyleItem::Sheet(sheet) = &sheets.items[0] else {
        panic!()
    };
    let rules = read(
        &d,
        StyleRead::Rules {
            owner: sheet.owner,
            path: vec![],
        },
        0,
    );
    assert_eq!(rules.items.len(), 2);
    let StyleItem::Rule(rule) = &rules.items[0] else {
        panic!()
    };
    assert_eq!(rule.selector.as_deref(), Some("p"));
    assert_eq!(rule.source.as_ref().unwrap().line, 1);
    let decls = read(
        &d,
        StyleRead::Declarations {
            owner: sheet.owner,
            path: rule.path.clone(),
        },
        0,
    );
    let StyleItem::Declaration(decl) = &decls.items[0] else {
        panic!()
    };
    assert!(decl.important);
    assert_eq!(decl.value, "red");
    assert_eq!(
        read(
            &d,
            StyleRead::Rules {
                owner: sheet.owner,
                path: vec![1]
            },
            0
        )
        .items
        .len(),
        1
    );
}
#[test]
fn huge_values_many_rules_and_pathological_selectors_are_bounded_without_matching() {
    let css = format!(
        "{}{{ --huge:{}; color:red}}{}",
        ".a ".repeat(1000),
        "x".repeat(1_000_000),
        "p{margin:0}".repeat(2000)
    );
    let d = doc(&format!(
        "<!doctype html><style>{css}</style><div id=x></div>"
    ));
    let owner = d.stylesheet_owner_nodes().next().unwrap();
    let page = read(
        &d,
        StyleRead::Rules {
            owner: wega_adapter_blitz::ToKey::key(owner),
            path: vec![],
        },
        1990,
    );
    assert!(page.items.len() <= STYLE_PAGE);
    let key = d.stylesheet_owner_nodes().next().unwrap();
    let rules = read(
        &d,
        StyleRead::Rules {
            owner: wega_adapter_blitz::ToKey::key(key),
            path: vec![],
        },
        0,
    );
    let StyleItem::Rule(r) = &rules.items[0] else {
        panic!()
    };
    assert!(r.truncated && r.selector.as_ref().unwrap().len() <= STYLE_TEXT);
    let declarations = read(
        &d,
        StyleRead::Declarations {
            owner: wega_adapter_blitz::ToKey::key(key),
            path: vec![0],
        },
        0,
    );
    assert!(
        declarations.items.iter().any(
            |i| matches!(i,StyleItem::Declaration(d) if d.truncated&&d.value.len()==STYLE_TEXT)
        )
    );
}
#[test]
fn deep_unrendered_and_detached_nodes_do_not_trigger_style_work() {
    let mut d = doc(&format!(
        "<!doctype html><body><div id=x>{}x{}</div>",
        "<div>".repeat(1000),
        "</div>".repeat(1000)
    ));
    let key = d.element_by_id("x").unwrap();
    assert_eq!(
        read(&d, StyleRead::Computed(key), 0).availability,
        Availability::NotComputed
    );
    let mut deep = key;
    for _ in 0..140 {
        deep = d.node(deep).unwrap().child_at(0).unwrap();
    }
    assert_eq!(
        d.read_style(&StyleRead::Computed(deep), 0).unwrap_err(),
        StyleReadError::Depth
    );
    d.mutate().remove(key).unwrap();
    assert_eq!(
        d.read_style(&StyleRead::Computed(key), 0).unwrap_err(),
        StyleReadError::Stale
    );
    let _ = key.node_id();
}
