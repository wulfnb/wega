//! The mutation epoch as the engine sees it: script host ops, the HTML parser and read-only
//! work, on real V8 and the real adapter.
use std::sync::Arc;
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

fn blank() -> View {
    let mut view = Engine::new(
        Limits::default(),
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
    view.url = Url::parse("https://fixture.test/e.html").unwrap();
    view
}
fn page(html: &str) -> View {
    let mut view = blank();
    view.set_html(html).unwrap();
    for _ in 0..6 {
        view.poll();
    }
    view
}
fn stepped(view: &mut View, script: &str) -> u64 {
    let before = view.mutation_epoch();
    view.evaluate_json(script).unwrap();
    view.mutation_epoch().wrapping_sub(before)
}

#[test]
fn parsing_a_document_counts_and_a_replacement_starts_again() {
    let big = page("<!doctype html><body><div id=a></div><p>one</p><p>two</p><p>three</p></body>");
    let small = page("<!doctype html><body><i></i></body>");
    assert!(
        big.mutation_epoch() > small.mutation_epoch(),
        "more parsing, more steps"
    );
    assert!(
        small.mutation_epoch() > 0,
        "the parser mutates through the contract"
    );
    // Replacing the document in place gives the new document's own count, not a continuation.
    let mut replaced =
        page("<!doctype html><body><div id=a></div><p>one</p><p>two</p><p>three</p></body>");
    let before = replaced.mutation_epoch();
    replaced
        .set_html("<!doctype html><body><i></i></body>")
        .unwrap();
    for _ in 0..6 {
        replaced.poll();
    }
    assert_eq!(replaced.mutation_epoch(), small.mutation_epoch());
    assert_ne!(replaced.mutation_epoch(), before);
    assert_eq!(
        replaced.document_generation(),
        replaced.document_generation()
    );
}

#[test]
fn script_mutations_step_the_epoch_and_reads_do_not() {
    let mut view = page("<!doctype html><body><div id=a class=x>hello</div></body>");
    // Reads, including ones that flush layout and compute styles.
    for read in [
        "document.getElementById('a').outerHTML",
        "document.getElementById('a').getBoundingClientRect().width",
        "getComputedStyle(document.getElementById('a')).color",
        "document.querySelectorAll('*').length",
        "document.getElementById('a').getAttribute('class')",
        "document.getElementById('a').style.cssText",
    ] {
        assert_eq!(stepped(&mut view, read), 0, "{read}");
    }
    // Real changes.
    for change in [
        "document.getElementById('a').setAttribute('data-n','1')",
        "document.getElementById('a').textContent='changed'",
        "document.body.appendChild(document.createElement('span'))",
        "document.getElementById('a').style.setProperty('color','red')",
        "document.getElementById('a').remove()",
        "document.body.innerHTML='<b>new</b>'",
    ] {
        assert!(stepped(&mut view, change) >= 1, "{change}");
    }
}

#[test]
fn script_no_ops_do_not_step_the_epoch() {
    let mut view = page("<!doctype html><body><div id=a class=x>hello</div></body>");
    for no_op in [
        "document.getElementById('a').setAttribute('class','x')",
        "document.getElementById('a').removeAttribute('missing')",
        "document.createElement('p')",
        "document.createTextNode('detached')",
        "document.getElementById('a').cloneNode(true)",
        "document.getElementById('a').firstChild.data='hello'",
    ] {
        assert_eq!(stepped(&mut view, no_op), 0, "{no_op}");
    }
    // The first real change still registers.
    assert_eq!(
        stepped(
            &mut view,
            "document.getElementById('a').setAttribute('class','y')"
        ),
        1
    );
}

#[test]
fn a_failed_script_mutation_leaves_the_epoch_alone() {
    let mut view = page("<!doctype html><body><div id=a></div></body>");
    let before = view.mutation_epoch();
    let outcome = view.evaluate_json(
        "(()=>{const a=document.getElementById('a');try{a.appendChild(a)}catch(e){return e.name}return 'no error'})()",
    );
    assert!(outcome.unwrap().contains("HierarchyRequestError"));
    assert_eq!(view.mutation_epoch(), before);
}

#[test]
fn layout_pending_tracks_flushes_and_mutations_without_ever_flushing() {
    let mut view = page("<!doctype html><body><div id=a>hello</div></body>");
    // Reading the flag must not perform the flush that clears it.
    assert!(view.layout_pending(), "nothing has been laid out yet");
    assert!(
        view.layout_pending(),
        "and asking again does not change that"
    );
    view.evaluate_json("document.body.getBoundingClientRect().height")
        .unwrap();
    assert!(
        !view.layout_pending(),
        "script asked for geometry, which flushed"
    );
    view.evaluate_json("document.getElementById('a').setAttribute('style','height:50px')")
        .unwrap();
    assert!(
        view.layout_pending(),
        "a mutation makes the layout stale again"
    );
    assert!(view.layout_pending());
}
