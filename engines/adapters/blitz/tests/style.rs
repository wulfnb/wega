//! The style contract (`wega_dom::style`) against a real Blitz document.
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use wega_adapter_blitz::BlitzDocument;
use wega_dom::style::{StyleAccess, StyleControl, StyleError};
use wega_dom::{DocumentView, MutationError, Mutator, NodeKey, NodeView, mutate};

const HTML: &str = "<!doctype html><style>body{margin:0}div{color:red}.a{color:blue}#x{color:green}\
    p{margin:0}#hidden{display:none}</style>\
    <div id=x class=a style='width: 10px; COLOR: Purple; bogus: 1; --gap: 4px'><p id=p>text</p></div>\
    <div id=y class=a></div><div id=hidden><span id=inner>x</span></div>";

fn doc() -> BlitzDocument {
    let mut doc =
        BlitzDocument::new(HtmlDocument::from_html(HTML, DocumentConfig::default()).into_inner());
    doc.set_viewport(blitz_traits::shell::Viewport {
        window_size: (800, 600),
        hidpi_scale: 1.0,
        zoom: 1.0,
        color_scheme: blitz_traits::shell::ColorScheme::Light,
    });
    doc.resolve(0.0);
    doc
}
fn id(doc: &BlitzDocument, id: &str) -> NodeKey {
    doc.element_by_id(id).unwrap()
}
fn inline(doc: &BlitzDocument, key: NodeKey) -> String {
    let mut css = String::new();
    doc.inline_style(key, &mut css).unwrap();
    css
}
fn property(doc: &BlitzDocument, key: NodeKey, name: &str) -> String {
    let mut css = String::new();
    doc.inline_property(key, name, &mut css).unwrap();
    css
}
fn resolved(doc: &BlitzDocument, key: NodeKey, name: &str) -> String {
    let mut css = String::new();
    doc.resolved_value(key, name, &mut css).unwrap();
    css
}
fn text_child(doc: &BlitzDocument, key: NodeKey) -> NodeKey {
    doc.node(key).unwrap().children().next().unwrap()
}

#[test]
fn inline_style_is_the_canonical_block_without_invalid_declarations() {
    let doc = doc();
    let x = id(&doc, "x");
    assert_eq!(inline(&doc, x), "width: 10px; color: purple; --gap: 4px;");
    assert_eq!(inline(&doc, id(&doc, "y")), "", "no style attribute");
    assert_eq!(property(&doc, x, "width"), "10px");
    assert_eq!(
        property(&doc, x, "COLOR"),
        "purple",
        "names are case-insensitive"
    );
    assert_eq!(property(&doc, x, "--gap"), "4px");
    assert_eq!(property(&doc, x, "height"), "", "not set");
    assert_eq!(property(&doc, x, "not-a-property"), "");
}

#[test]
fn reads_append_to_the_callers_buffer() {
    let doc = doc();
    let mut out = String::from("> ");
    doc.inline_property(id(&doc, "x"), "width", &mut out)
        .unwrap();
    doc.resolved_value(id(&doc, "x"), "width", &mut out)
        .unwrap();
    assert_eq!(out, "> 10px10px");
}

#[test]
fn set_inline_property_follows_cssom_and_keeps_the_attribute_canonical() {
    let mut doc = doc();
    let y = id(&doc, "y");
    assert_eq!(doc.set_inline_property(y, "width", "5px", false), Ok(true));
    assert_eq!(doc.set_inline_property(y, "height", "7px", false), Ok(true));
    assert_eq!(
        doc.node(y).unwrap().attr("style"),
        Some("width: 5px; height: 7px;")
    );
    assert_eq!(
        doc.set_inline_property(y, "width", "5px", false),
        Ok(false),
        "the same declaration changes nothing"
    );
    assert_eq!(doc.set_inline_property(y, "width", "6px", false), Ok(true));
    assert_eq!(doc.set_inline_property(y, "width", "5px", false), Ok(true));
    assert_eq!(
        inline(&doc, y),
        "width: 5px; height: 7px;",
        "an updated declaration keeps its position"
    );
    // Invalid values, unknown properties and `!important` inside the value are ignored.
    for (name, value) in [
        ("color", "notacolor"),
        ("width", "5"),
        ("nonsense", "1px"),
        ("color", "red !important"),
    ] {
        assert_eq!(
            doc.set_inline_property(y, name, value, false),
            Ok(false),
            "{name}: {value}"
        );
    }
    assert_eq!(inline(&doc, y), "width: 5px; height: 7px;");
    assert_eq!(
        doc.node(y).unwrap().attr("style"),
        Some("width: 5px; height: 7px;")
    );
    // Priority, shorthand expansion and custom properties.
    assert_eq!(doc.set_inline_property(y, "color", "red", true), Ok(true));
    assert_eq!(
        doc.set_inline_property(y, "margin", "1px 2px", false),
        Ok(true)
    );
    assert_eq!(doc.set_inline_property(y, "--k", " 3px ", false), Ok(true));
    assert_eq!(
        inline(&doc, y),
        "width: 5px; height: 7px; color: red !important; margin: 1px 2px; --k: 3px;"
    );
    assert_eq!(property(&doc, y, "margin-left"), "2px");
    assert_eq!(property(&doc, y, "margin"), "1px 2px");
    // Removal: an empty value; a shorthand removes its longhands; absent properties are a no-op.
    assert_eq!(doc.set_inline_property(y, "margin", "", false), Ok(true));
    assert_eq!(doc.set_inline_property(y, "margin", "", false), Ok(false));
    assert_eq!(doc.set_inline_property(y, "width", "", false), Ok(true));
    assert_eq!(
        inline(&doc, y),
        "height: 7px; color: red !important; --k: 3px;"
    );
}

#[test]
fn an_inline_write_is_seen_by_the_cascade_after_the_next_flush() {
    let mut doc = doc();
    let (x, y) = (id(&doc, "x"), id(&doc, "y"));
    assert_eq!(resolved(&doc, y, "color"), "rgb(0, 0, 255)", ".a beats div");
    assert_eq!(
        resolved(&doc, x, "color"),
        "rgb(128, 0, 128)",
        "inline beats #x"
    );
    assert_eq!(
        resolved(&doc, id(&doc, "p"), "color"),
        "rgb(128, 0, 128)",
        "inherited"
    );
    doc.set_inline_property(y, "color", "rgb(1, 2, 3)", false)
        .unwrap();
    doc.set_inline_property(x, "color", "", false).unwrap();
    doc.resolve(0.0);
    assert_eq!(resolved(&doc, y, "color"), "rgb(1, 2, 3)");
    assert_eq!(resolved(&doc, x, "color"), "rgb(0, 128, 0)", "back to #x");
    assert_eq!(resolved(&doc, id(&doc, "p"), "color"), "rgb(0, 128, 0)");
}

#[test]
fn resolved_values_cover_used_values_custom_properties_and_undisplayed_elements() {
    let doc = doc();
    let x = id(&doc, "x");
    assert_eq!(resolved(&doc, x, "width"), "10px");
    assert_eq!(resolved(&doc, x, "--gap"), "4px");
    assert_eq!(resolved(&doc, x, "display"), "block");
    assert_eq!(resolved(&doc, x, "not-a-property"), "");
    assert_eq!(resolved(&doc, id(&doc, "hidden"), "display"), "none");
    assert_eq!(
        resolved(&doc, id(&doc, "inner"), "display"),
        "inline",
        "styles inside display:none are computed on demand"
    );
}

#[test]
fn detached_elements_have_inline_but_no_resolved_style() {
    let mut doc = doc();
    let detached = {
        let mut m = mutate(&mut doc);
        m.create_element(
            html5ever::QualName::new(
                None,
                html5ever::ns!(html),
                html5ever::LocalName::from("div"),
            ),
            [],
        )
    };
    assert_eq!(
        doc.set_inline_property(detached, "color", "red", false),
        Ok(true)
    );
    doc.resolve(0.0);
    assert_eq!(inline(&doc, detached), "color: red;");
    assert_eq!(resolved(&doc, detached, "color"), "");
}

#[test]
fn stale_keys_and_non_elements_are_rejected_without_touching_the_document() {
    let mut doc = doc();
    let (y, p) = (id(&doc, "y"), id(&doc, "p"));
    let text = text_child(&doc, p);
    let mut out = String::new();
    assert_eq!(
        doc.inline_style(text, &mut out),
        Err(StyleError::NotElement(text))
    );
    assert_eq!(
        doc.inline_property(text, "color", &mut out),
        Err(StyleError::NotElement(text))
    );
    assert_eq!(
        doc.resolved_value(text, "color", &mut out),
        Err(StyleError::NotElement(text))
    );
    assert_eq!(
        doc.set_inline_property(text, "color", "red", false),
        Err(MutationError::NotElement(text))
    );
    mutate(&mut doc).remove_and_drop(y).unwrap();
    assert_eq!(doc.inline_style(y, &mut out), Err(StyleError::Stale(y)));
    assert_eq!(
        doc.inline_property(y, "color", &mut out),
        Err(StyleError::Stale(y))
    );
    assert_eq!(
        doc.resolved_value(y, "color", &mut out),
        Err(StyleError::Stale(y))
    );
    assert_eq!(
        doc.set_inline_property(y, "color", "red", false),
        Err(MutationError::Stale(y))
    );
    assert_eq!(out, "", "rejected reads write nothing");
}

#[test]
fn support_and_media_queries() {
    let mut doc = doc();
    assert!(doc.supports_declaration("display", "grid"));
    assert!(doc.supports_declaration("--anything", "1"));
    assert!(!doc.supports_declaration("display", "nope"));
    assert!(!doc.supports_declaration("nope", "1"));
    assert!(doc.supports_condition("(display: grid) and (not (color: nope))"));
    assert!(doc.supports_condition("color: red"));
    assert!(!doc.supports_condition("(display: nope)"));
    assert!(!doc.supports_condition("garbage("));
    assert!(doc.media_matches("(min-width: 1px)"));
    assert!(doc.media_matches("screen"));
    assert!(doc.media_matches(""), "an empty list matches");
    assert!(!doc.media_matches("(max-width: 1px)"));
    assert!(!doc.media_matches("print"));
    assert!(!doc.media_matches("garbage("));
}
