//! `querySelector`, `matches` and `closest` on Wega's selector engine.
use std::sync::Arc;
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

fn page(html: &str) -> View {
    let mut view = Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {}))
        .unwrap()
        .create_view(Url::parse("https://example.test/").unwrap())
        .unwrap();
    view.set_html(html).unwrap();
    view.poll();
    view
}

fn eval(view: &mut View, script: &str) -> String {
    view.evaluate_json(script).unwrap()
}

const DOC: &str = r#"<!doctype html><html><body>
<ul id=list><li class=item>a</li><li>b</li><li class=item>c</li><li class=item>d</li></ul>
<div id=outer><p id=inner><span id=leaf></span></p></div>
<input id=box type=CheckBox lang=EN-us data-Mixed=1>
<svg id=pic><linearGradient id=grad></linearGradient><rect viewBox="0 0 1 1"/></svg>
<div id=empty><!-- only a comment --></div>
</body></html>"#;

#[test]
fn scope_closest_and_nth_child_of() {
    let mut view = page(DOC);
    // `:scope` is the element the query runs on.
    assert_eq!(
        eval(
            &mut view,
            "document.getElementById('outer').querySelectorAll(':scope > p').length"
        ),
        "1"
    );
    assert_eq!(
        eval(
            &mut view,
            "document.getElementById('outer').querySelectorAll(':scope > span').length"
        ),
        "0"
    );
    // On the document, `:scope` is the document element.
    assert_eq!(
        eval(&mut view, "document.querySelector(':scope').tagName"),
        "\"HTML\""
    );
    assert_eq!(
        eval(
            &mut view,
            "document.getElementById('leaf').closest('div').id"
        ),
        "\"outer\""
    );
    assert_eq!(
        eval(
            &mut view,
            "document.getElementById('leaf').closest('#outer > p').id"
        ),
        "\"inner\""
    );
    // `:nth-child(An+B of S)` counts only the siblings matching S.
    assert_eq!(
        eval(
            &mut view,
            "[...document.querySelectorAll('li:nth-child(2 of .item)')].map(e => e.textContent).join()"
        ),
        "\"c\""
    );
    assert_eq!(
        eval(
            &mut view,
            "document.querySelectorAll('#empty:empty').length"
        ),
        "1"
    );
}

#[test]
fn case_namespaces_and_invalid_selectors() {
    let mut view = page(DOC);
    // HTML type and attribute names are case-insensitive; SVG names are not.
    assert_eq!(
        eval(&mut view, "document.querySelectorAll('LI').length"),
        "4"
    );
    assert_eq!(
        eval(
            &mut view,
            "document.querySelectorAll('[DATA-mixed]').length"
        ),
        "1"
    );
    assert_eq!(
        eval(
            &mut view,
            "document.querySelectorAll('linearGradient').length"
        ),
        "1"
    );
    assert_eq!(
        eval(
            &mut view,
            "document.querySelectorAll('lineargradient').length"
        ),
        "0"
    );
    assert_eq!(
        eval(&mut view, "document.querySelectorAll('[viewBox]').length"),
        "1"
    );
    assert_eq!(
        eval(&mut view, "document.querySelectorAll('[viewbox]').length"),
        "0"
    );
    // `type` and `lang` values compare case-insensitively unless `s` is given.
    assert_eq!(
        eval(
            &mut view,
            "document.querySelectorAll('[type=checkbox]').length"
        ),
        "1"
    );
    assert_eq!(
        eval(
            &mut view,
            "document.querySelectorAll('[type=checkbox s]').length"
        ),
        "0"
    );
    assert_eq!(
        eval(&mut view, "document.querySelectorAll('[id=BOX]').length"),
        "0"
    );
    // Namespace prefixes: any namespace, and no namespace (HTML elements have one).
    assert_eq!(
        eval(&mut view, "document.querySelectorAll('*|li').length"),
        "4"
    );
    assert_eq!(
        eval(&mut view, "document.querySelectorAll('|li').length"),
        "0"
    );
    for invalid in [
        "'li:nth-child(2 of)'",
        "'svg|rect'",
        "'::x-unknown'",
        "'a,'",
        "''",
    ] {
        assert_eq!(
            eval(
                &mut view,
                &format!(
                    "(()=>{{try{{document.querySelector({invalid});return 'ok'}}catch(e){{return e.name}}}})()"
                )
            ),
            "\"SyntaxError\"",
            "{invalid}"
        );
    }
    assert_eq!(
        eval(
            &mut view,
            "document.getElementById('box').matches('input:not(:checked)')"
        ),
        "true"
    );
}
