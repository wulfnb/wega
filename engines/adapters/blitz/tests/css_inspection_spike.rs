//! D3-B phase-1 runtime spike, before production implementation.
use style::{
    properties::{LonghandId, PropertyDeclarationId},
    servo_arc::Arc,
    stylesheets::{CssRule, StylesheetInDocument},
};
use wega_adapter_blitz::{BlitzDocument, DocumentSetup, ToNodeId};
use wega_dom::{DocumentView, MutationClock};
fn doc(html: &str) -> BlitzDocument {
    BlitzDocument::from_html(html, DocumentSetup::default())
}
#[test]
fn cssom_and_cached_computed_styles_are_executable_without_inspector_layout() {
    let mut d = doc(
        "<!doctype html><style>body{color:rgb(1,2,3)}#x{width:42px!important}@media(min-width:1px){#x{height:9px}}</style><div id=x></div>",
    );
    let x = d.element_by_id("x").unwrap().node_id();
    assert!(d.get_node(x).unwrap().primary_styles().is_none());
    let owner = d.stylesheet_owner_nodes().next().unwrap();
    assert_eq!(d.stylesheet_rule_count(owner, &[]), Some(3));
    assert!(
        d.stylesheet_rule_info(owner, &[1])
            .unwrap()
            .css_text
            .contains("important")
    );
    assert_eq!(d.stylesheet_rule_count(owner, &[2]), Some(1));
    // Rendering, explicitly owned by fixture setup; the inspection itself only borrows.
    d.set_viewport(blitz_traits::shell::Viewport {
        window_size: (800, 600),
        hidpi_scale: 1.,
        zoom: 1.,
        color_scheme: blitz_traits::shell::ColorScheme::Light,
    });
    d.resolve(0.);
    let epoch = d.mutation_epoch();
    let s = d.get_node(x).unwrap().primary_styles().unwrap();
    assert_eq!(
        s.computed_value_to_string(PropertyDeclarationId::Longhand(LonghandId::Width)),
        "42px"
    );
    assert_eq!(
        s.computed_value_to_string(PropertyDeclarationId::Longhand(LonghandId::Color)),
        "rgb(1, 2, 3)"
    );
    assert!(
        s.rules()
            .self_and_ancestors()
            .any(|r| r.style_source().is_some())
    );
    drop(s);
    assert_eq!(d.mutation_epoch(), epoch);
}
#[test]
fn retained_blocks_cannot_unambiguously_identify_matched_rules_or_selectors() {
    let mut d = doc(
        "<!doctype html><style>.a{color:red}.never{color:blue}</style><div id=x class=a></div>",
    );
    let sheet = d.author_stylesheets().next().unwrap().clone();
    let rules = {
        let g = d.guard().read();
        sheet.contents(&g).rules.read_with(&g).0.clone()
    };
    let (CssRule::Style(a), CssRule::Style(b)) = (&rules[0], &rules[1]) else {
        panic!()
    };
    // Valid CSSOM rule objects can share their immutable declaration block. RuleTree
    // deliberately stores only this block identity, not which selector matched.
    {
        let block = {
            let g = d.guard().read();
            a.read_with(&g).block.clone()
        };
        let mut g = d.guard().write();
        b.write_with(&mut g).block = block;
    }
    d.resolve(0.);
    let x = d.element_by_id("x").unwrap().node_id();
    let s = d.get_node(x).unwrap().primary_styles().unwrap();
    let g = d.guard().read();
    let block = &a.read_with(&g).block;
    assert!(
        s.rules()
            .self_and_ancestors()
            .filter_map(|r| r.style_source())
            .any(|r| Arc::ptr_eq(r.get(), block))
    );
    assert!(Arc::ptr_eq(&b.read_with(&g).block, block));
    assert!(!d.matches_selector(x, ".never").unwrap());
    println!(
        "SPIKE: retained block identity maps to both .a and non-matching .never; matched selector identity is unavailable"
    );
}
#[test]
fn legacy_group_serialization_and_cssom_epoch_gap_are_measured() {
    let mut d = doc(&format!(
        "<!doctype html><style>@media(all){{{}}}</style><div></div>",
        ".a{--large:abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz}".repeat(2000)
    ));
    let owner = d.stylesheet_owner_nodes().next().unwrap();
    let start = std::time::Instant::now();
    let info = d.stylesheet_rule_info(owner, &[0]).unwrap();
    println!(
        "SPIKE: legacy group cssText={} bytes, elapsed_us={}",
        info.css_text.len(),
        start.elapsed().as_micros()
    );
    assert!(info.css_text.len() > 64 * 1024);
    let before = d.stylesheet_generation();
    d.stylesheet_insert_rule(owner, &[], "p{color:red}", 1)
        .unwrap();
    assert_eq!(
        d.stylesheet_generation(),
        before,
        "baseline gap, close before stamped rule refs"
    );
}
