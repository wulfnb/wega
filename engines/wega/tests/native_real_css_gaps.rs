//! Regressions for CSS that real sites rely on and the native engine dropped:
//! unknown pseudos invalidating whole rules, modern color syntax, UA defaults,
//! shrink-to-fit with atomic inlines, floats, named grid areas and the
//! `background` shorthand.
use std::{cell::RefCell, rc::Rc};
use wega_dom::DocumentView;
use wega_engine::{
    document::{DocumentSetup, WegaDocument},
    native_engine::*,
};
fn document(html: &str) -> NativeDocument<WegaDocument> {
    let mut d = NativeDocument::new(Rc::new(RefCell::new(WegaDocument::from_html(
        html,
        DocumentSetup::default(),
    ))));
    d.flush_layout();
    d
}
fn rect(d: &NativeDocument<WegaDocument>, id: &str) -> wega_dom::layout::Rect {
    let key = d.doc.borrow().element_by_id(id).unwrap();
    d.bounding_rect(key).unwrap()
}
fn style(d: &NativeDocument<WegaDocument>, id: &str) -> ComputedStyle {
    let key = d.doc.borrow().element_by_id(id).unwrap();
    d.find_box(key).unwrap().style.clone()
}
fn rgba(c: Color) -> (u8, u8, u8, u8) {
    (c.r, c.g, c.b, c.a)
}

#[test]
fn unknown_pseudo_classes_and_elements_keep_the_rest_of_the_rule() {
    let d = document(
        "<style>a:link,a:visited{color:rgb(1,2,3)} p:first-of-type,p::before{width:50px} \
         li:nth-of-type(2){width:20px} :is(.y, :unknown-thing){width:40px}</style>\
         <body><a id=a href=#>a</a><p id=p>p</p><p id=p2>q</p><ul><li>1</li><li id=li>2</li></ul>\
         <div id=y class=y></div></body>",
    );
    assert_eq!(rgba(style(&d, "a").color), (1, 2, 3, 255));
    assert_eq!(rect(&d, "p").width, 50.0);
    assert_ne!(rect(&d, "p2").width, 50.0);
    assert_eq!(rect(&d, "li").width, 20.0);
    assert_eq!(rect(&d, "y").width, 40.0);
}

#[test]
fn modern_color_syntax_and_currentcolor() {
    let d = document(
        "<body><div id=a style='color:rgb(10 20 30 / 50%)'></div><div id=b style='color:hsl(120deg 100% 25%)'></div>\
         <div id=c style='color:rebeccapurple;border:1px solid currentcolor'></div><div id=d style='background:#fff url(x.png) 0 -10px no-repeat'></div></body>",
    );
    assert_eq!(rgba(style(&d, "a").color), (10, 20, 30, 128));
    assert_eq!(rgba(style(&d, "b").color), (0, 128, 0, 255));
    assert_eq!(rgba(style(&d, "c").borders.top.color), (102, 51, 153, 255));
    let bg = style(&d, "d");
    assert_eq!(rgba(bg.background_color), (255, 255, 255, 255));
    assert_eq!(bg.background_repeat, (false, false));
    assert_eq!(bg.background_position, (Length::Px(0.0), Length::Px(-10.0)));
}

#[test]
fn ua_line_height_is_normal_and_units_parse() {
    let d = document(
        "<body><h1 id=h>Title</h1><p id=p style='font-size:12pt;width:min(50%, 100px)'>x</p></body>",
    );
    // `line-height: normal` scales with the heading's font, not a fixed 20px.
    assert_eq!(style(&d, "h").line_height, 40.0);
    assert_eq!(style(&d, "p").font_size, 16.0);
    assert_eq!(rect(&d, "p").width, 100.0);
}

#[test]
fn shrink_to_fit_measures_inline_blocks_and_inline_flex() {
    let d = document(
        "<body style='margin:0'><div style='display:flex'><div id=item><span style='display:inline-block;width:50px'></span><span style='display:inline-block;width:60px'></span></div></div>\
         <span id=btn style='display:inline-flex;padding:0 5px'><span style='display:inline-block;width:40px;height:10px'></span></span></body>",
    );
    assert!(rect(&d, "item").width >= 110.0, "{:?}", rect(&d, "item"));
    assert_eq!(rect(&d, "btn").width, 50.0);
}

#[test]
fn floats_place_beside_and_clear_below() {
    let d = document(
        "<body style='margin:0'><div style='width:400px'><div id=r style='float:right;width:100px;height:50px'></div>\
         <div id=l style='float:left;width:80px;height:30px'></div><p id=text style='margin:0'>text</p>\
         <p id=clear style='clear:both;margin:0'>below</p></div></body>",
    );
    assert_eq!(rect(&d, "r").x, 300.0);
    assert_eq!(rect(&d, "l").x, 0.0);
    assert_eq!(rect(&d, "text").x, 80.0);
    assert_eq!(rect(&d, "text").width, 220.0);
    assert_eq!(rect(&d, "clear").y, 50.0);
}

#[test]
fn named_grid_areas_negative_lines_and_auto_fill() {
    let d = document(
        "<body style='margin:0'><div style='display:grid;width:300px;grid-template:\"head head\" 20px \"side main\" 30px / 100px 1fr'>\
         <div id=main style='grid-area:main'></div><div id=head style='grid-area:head'></div><div id=side style='grid-area:side'></div></div>\
         <div style='display:grid;width:300px;grid-template-columns:repeat(auto-fill,minmax(100px,1fr))'><div id=g1></div><div id=g2></div><div id=g3></div><div id=g4></div><div id=full style='grid-column:1 / -1'></div></div></body>",
    );
    assert_eq!((rect(&d, "head").y, rect(&d, "head").width), (0.0, 300.0));
    assert_eq!((rect(&d, "side").x, rect(&d, "side").y), (0.0, 20.0));
    assert_eq!((rect(&d, "main").x, rect(&d, "main").width), (100.0, 200.0));
    assert_eq!(rect(&d, "g3").x, 200.0);
    assert_eq!(rect(&d, "g4").x, 0.0);
    assert_eq!(rect(&d, "full").width, 300.0);
}

#[test]
fn presentational_attributes_and_trailing_br() {
    let d = document(
        "<body style='margin:0'><table id=t cellpadding=0 cellspacing=0><tr><td id=a width=50>a</td><td id=b>bbbb</td></tr></table>\
         <div id=br style='line-height:20px'>text<br></div></body>",
    );
    assert_eq!(rect(&d, "a").width, 50.0);
    assert!(rect(&d, "t").width < 200.0, "auto tables shrink to fit");
    assert_eq!(rect(&d, "br").height, 20.0);
}

#[test]
fn tailwind_v4_layers_supports_property_and_host_variables() {
    let d = document(
        "<style>@layer theme, base, utilities;\
         @layer utilities{.w-10{width:calc(var(--spacing) * 10)} .red{color:rgb(255 0 0)}}\
         @layer base{*{color:rgb(0 0 255)} div{width:5px}}\
         @layer theme{:root,:host{--spacing:.25rem}}\
         @supports (display:grid){.s{height:7px}} @supports (not-a-property:1){.s{height:9px}}\
         @property --tw-border-style{syntax:\"*\";inherits:false;initial-value:solid}\
         .b{border-bottom-style:var(--tw-border-style);border-bottom-width:2px;border-color:rgb(1 2 3)}\
         p{color:rgb(0 128 0)}</style>\
         <body><div id=w class=\"w-10 red\"></div><p id=p class=red>t</p><div id=s class=s></div><div id=b class=b></div></body>",
    );
    // utilities (later layer) beat base for the same element.
    assert_eq!(rect(&d, "w").width, 40.0);
    assert_eq!(rgba(style(&d, "w").color), (255, 0, 0, 255));
    // Unlayered rules beat every layer, whatever the specificity.
    assert_eq!(rgba(style(&d, "p").color), (0, 128, 0, 255));
    assert_eq!(rect(&d, "s").height, 7.0);
    let b = style(&d, "b");
    assert_eq!(b.borders.bottom.width, 2.0);
    assert_eq!(rgba(b.borders.bottom.color), (1, 2, 3, 255));
    assert!(!b.border_hidden[2]);
}

#[test]
fn logical_properties_range_media_and_modern_colors() {
    let d = document(
        "<style>.md,.never{display:none} @media (width >= 48rem){.md{display:flex}} @media (400px <= width < 500px){.never{display:flex}}</style>\
         <body style='margin:0'><div id=c style='width:200px;margin-inline:auto;padding-inline:10px;padding-block:3px'></div>\
         <div id=md class=md></div><div id=nv class=never></div>\
         <div id=o style='color:oklch(62.8% 0.2577 29.23)'></div>\
         <div id=m style='color:color-mix(in oklab, rgb(255 0 0) 50%, transparent)'></div>\
         <div id=z style='position:relative;z-index:calc(10 * -1)'></div></body>",
    );
    let c = rect(&d, "c");
    assert_eq!(c.width, 220.0);
    assert!(c.x > 0.0, "margin-inline:auto centers");
    assert_eq!(c.height, 6.0);
    assert!(
        d.doc
            .borrow()
            .element_by_id("md")
            .is_some_and(|k| d.find_box(k).is_some())
    );
    assert!(
        d.doc
            .borrow()
            .element_by_id("nv")
            .is_some_and(|k| d.find_box(k).is_none())
    );
    let (r, g, b, _) = rgba(style(&d, "o").color);
    assert!(r > 240 && g < 20 && b < 20, "oklch red: {r},{g},{b}");
    assert_eq!(rgba(style(&d, "m").color), (255, 0, 0, 128));
    assert_eq!(style(&d, "z").z_index, Some(-10));
}

#[test]
fn flex_text_items_and_button_content_size() {
    let d = document(
        "<body style='margin:0'><div style='display:flex;flex-direction:column;align-items:center'>\
         <button id=btn style='display:flex;gap:8px;padding:0;border:0;font-size:18px'><span id=s style='display:flex;gap:8px'>Get Started <i style='display:block;width:20px;height:20px'></i></span></button></div></body>",
    );
    // One line of text: the anonymous flex item is not squeezed.
    assert!(rect(&d, "s").height < 30.0, "{:?}", rect(&d, "s"));
    assert!(rect(&d, "btn").width >= rect(&d, "s").width);
}

#[test]
fn percent_heights_against_auto_containers_behave_as_auto() {
    // CSS 2.1 §10.5. The root resolves against the viewport, so a
    // `html,body{height:100%}` chain is definite all the way down.
    let d = document(
        "<style>html,body{height:100%;margin:0}</style><body><div id=full style='height:50%'></div>\
         <div style='display:inline-block'><div id=card style='height:100%;overflow:hidden'><div style='height:70px'></div></div></div></body>",
    );
    assert!(rect(&d, "full").height > 0.0);
    assert_eq!(rect(&d, "card").height, 70.0);
}

#[test]
fn overflow_clips_only_the_containing_block_chain() {
    // Google's consent dialog: `body{overflow:hidden}` belongs to the viewport, and a
    // fixed overlay escapes an `overflow:hidden;height:0` wrapper.
    let engine = wega_engine::Engine::new(
        wega_engine::Limits::default(),
        wega_engine::NetworkPolicy {
            offline: true,
            ..Default::default()
        },
        std::sync::Arc::new(|| {}),
    )
    .unwrap();
    let mut view = engine
        .create_view(wega_engine::Url::parse("about:blank").unwrap())
        .unwrap();
    view.set_html(
        "<style>html,body{height:100%;margin:0}body{overflow:hidden}</style><body><p>page</p>\
         <div style='height:0;overflow:hidden'><div style='position:fixed;inset:0;z-index:5;background:rgb(0 0 255)'></div></div></body>",
    )
    .unwrap();
    view.poll();
    let frame = view.paint(200, 100, 1.0, 1.0, false).unwrap();
    let at = |x: usize, y: usize| {
        let i = (y * 200 + x) * 4;
        (frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2])
    };
    assert_eq!(at(100, 50), (0, 0, 255));
    assert_eq!(at(5, 95), (0, 0, 255));
}
#[test]
fn author_rules_stylo_rejects_reach_the_native_engine() {
    // Phase 3: the native engine parses the original stylesheet text, so `:has()` (which
    // Stylo drops) applies. After a CSSOM edit the sheet falls back to Stylo's rules.
    let d = document(
        "<style id=s>.card:has(img){width:120px} .card{height:10px}</style>\
         <body><div id=with class=card><img></div><div id=without class=card></div></body>",
    );
    assert_eq!(rect(&d, "with").width, 120.0);
    assert_ne!(rect(&d, "without").width, 120.0);
    assert_eq!(rect(&d, "without").height, 10.0);
}
#[test]
fn border_radius_is_per_corner_and_overlapping_radii_scale_down() {
    use wega_engine::native_engine::layout::LayoutBox;
    let d = document(
        "<body><div id=a style='width:100px;height:40px;border-radius:4px 8px 12px 16px'></div>\
         <div id=b style='width:100px;height:40px;border-radius:30px;border-top-left-radius:0'></div>\
         <div id=c style='width:100px;height:40px;border-radius:50%'></div></body>",
    );
    let radii = |id: &str| -> [f64; 4] {
        let key = d.doc.borrow().element_by_id(id).unwrap();
        let b: &LayoutBox = d.find_box(key).unwrap();
        b.radii()
    };
    assert_eq!(radii("a"), [4.0, 8.0, 12.0, 16.0]);
    // 30 + 30 exceeds the 40px height on the right side: everything scales by 40/60.
    let b = radii("b");
    assert_eq!(b[0], 0.0);
    assert!(
        (b[1] - 20.0).abs() < 1e-9 && (b[2] - 20.0).abs() < 1e-9,
        "{b:?}"
    );
    // 50% of the shorter side (40px) is 20px on every corner.
    assert_eq!(radii("c"), [20.0; 4]);
}
