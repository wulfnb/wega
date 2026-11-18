use wega_adapter_blitz::{BlitzDocument, DocumentSetup};
use wega_dom::DocumentView;
use wega_engine::native_engine::css::*;
fn matches(html: &str, selector: &str, id: &str) -> bool {
    let doc = BlitzDocument::from_html(html, DocumentSetup::default());
    let key = doc.element_by_id(id).unwrap();
    parse_complex_selector(selector).is_some_and(|s| s.matches(&doc, key))
}
#[test]
fn actual_dom_selector_matching() {
    let html = r#"<section class=outer><div class=outer><p><i id=a></i>text<b id=b class="chosen" data-label="hello world"></b><b id=c></b></p></div></section>"#;
    for (selector, id) in [
        ("i + b", "b"),
        ("i ~ b", "c"),
        ("b:nth-child(2)", "b"),
        ("b:nth-last-child(1)", "c"),
        ("i:first-child", "a"),
        ("b:last-child", "c"),
        ("b:not(.absent)", "b"),
        ("b:is(.absent,.chosen)", "b"),
        ("b:where(#b)", "b"),
        ("[data-label='hello world']", "b"),
        ("[data-label~=world]", "b"),
        ("[data-label^=HELLO i]", "b"),
        ("section > .outer b", "b"),
    ] {
        assert!(matches(html, selector, id), "{selector} -> {id}");
    }
    assert!(!matches(html, "b:first-child", "b"));
    assert!(!matches(html, "b:not(.chosen)", "b"));
    assert!(!matches(html, "i + b", "c"));
    assert!(!matches(html, "b:hover", "b"));
    assert!(matches("<p id='a:b'></p>", r"#a\:b", "a:b"));
    assert_eq!(
        parse_complex_selector(":where(#x).c")
            .unwrap()
            .specificity(),
        (0, 1, 0)
    );
    assert_eq!(
        parse_complex_selector(":is(#x,.c) b")
            .unwrap()
            .specificity(),
        (1, 0, 1)
    );
    // Matching must backtrack when the nearest matching ancestor fails the
    // earlier child combinator.
    assert!(matches(
        "<main><div><div><b id=x></b></div></div></main>",
        "main > div b",
        "x"
    ));
}
#[test]
fn syntax_delimiters_comments_and_invalid_recovery() {
    let sheet = StyleSheet::parse(
        r#"/* comment { ; } */ p { color:red; --url: url('a;b:c'); --quoted: 'a;}'; width:calc(100% - (2px * 3)); broken; height:20px ! IMPORTANT } @unknown { p {color:green} } p:unknown {color:blue} p {color:black}"#,
    );
    assert_eq!(sheet.rules.len(), 2);
    assert!(sheet.unsupported.iter().any(|s| s == "@unknown"));
    let decl = &sheet.rules[0].declarations;
    assert_eq!(decl.len(), 5);
    assert!(decl.last().unwrap().important);
    assert!(decl.iter().any(|d| d.value == "url('a;b:c')"));
    assert!(Length::parse("5").is_none());
    assert_eq!(Length::parse("2rem"), Some(Length::Rem(2.0)));
    assert!(
        (Length::parse("calc(100% - (2px * 3))")
            .unwrap()
            .resolve(200.0, 16.0, 16.0)
            - 194.0)
            .abs()
            < 0.01
    );
    assert!(Length::parse("calc(1px / 0)").is_none());
}
#[test]
fn cascade_variables_importance_and_inheritance() {
    let doc = BlitzDocument::from_html(
        "<div id=p><p id=x style='width:1px !important;color:inherit'></p></div>",
        DocumentSetup::default(),
    );
    let sheet = StyleSheet::parse(
        "#p {color:blue;--Size:calc(50% - 2px)} #x {width:200px !important; color:red; height:var(--missing, 12px);min-width:var(--Size);--loop:var(--loop);max-height:var(--loop,20px)}",
    );
    let parent =
        resolve_computed_style(&doc, doc.element_by_id("p").unwrap(), None, &[&sheet], None);
    let child = resolve_computed_style(
        &doc,
        doc.element_by_id("x").unwrap(),
        Some(&parent),
        &[&sheet],
        None,
    );
    assert_eq!(child.width, Length::Px(1.0));
    assert_eq!(child.height, Length::Px(12.0));
    assert_eq!(child.color, Color::parse("blue").unwrap());
    assert!((child.min_width.resolve(200.0, 16.0, 16.0) - 98.0).abs() < 0.01);
    assert_eq!(child.max_height, Length::Px(20.0));
    let d = parse_declarations("--Size:1px;--size:2px");
    assert_ne!(d[0].property, d[1].property);
}

#[test]
fn non_finite_transform_values_are_rejected() {
    assert!(Transform::parse("rotate(1e309deg)").is_none());
    assert!(Transform::parse("scale(NaN)").is_none());
    assert!(Length::parse("1e309px").is_none());
    assert!(
        Length::parse("calc(100000000000000000000px * 100000000000000000000)")
            .unwrap()
            .resolve(100.0, 16.0, 16.0)
            .is_finite()
    );
}
