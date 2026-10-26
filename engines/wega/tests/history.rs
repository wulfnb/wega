//! Session history inside a document: pushState traversal, fragments, hashchange.
mod support;
use support::{Reply, Server, engine, open};
use wega_engine::PageEvent;

fn page() -> Server {
    Server::new(|_| {
        Reply::html(
            "<title>history</title><style>body{margin:0}a{display:block;height:30px}#section{margin-top:3000px}</style>\
             <a id=link href=#section>jump</a><div id=section>target</div>\
             <script>globalThis.log=[];addEventListener('popstate',e=>log.push('pop:'+JSON.stringify(e.state)));\
             addEventListener('hashchange',e=>log.push('hash:'+new URL(e.newURL).hash))</script>",
        )
    })
}

#[test]
fn browser_back_walks_pushstate_entries_before_leaving_the_document() {
    let server = page();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.evaluate("history.pushState({step:1},'','/one');history.pushState({step:2},'','/two')")
        .unwrap();
    view.poll();
    assert_eq!(view.url.path(), "/two");
    view.traverse(-1);
    assert_eq!(view.url.path(), "/one");
    assert_eq!(
        view.evaluate_json("log").unwrap(),
        "[\"pop:{\\\"step\\\":1}\"]"
    );
    view.traverse(-1);
    view.traverse(-1);
    assert_eq!(
        view.events.pop_front(),
        Some(PageEvent::Traverse(-1)),
        "leaving the document goes to the browser"
    );
    view.traverse(2);
    assert_eq!(view.url.path(), "/two");
    assert_eq!(
        server.requests("/").len(),
        1,
        "traversal stays in the document"
    );
}

#[test]
fn fragments_scroll_fire_hashchange_and_never_refetch() {
    let server = page();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    view.click(20.0, 10.0);
    view.poll();
    assert_eq!(view.url.fragment(), Some("section"));
    assert!(
        view.evaluate_json("scrollY")
            .unwrap()
            .parse::<f64>()
            .unwrap()
            > 1000.0
    );
    view.load(server.url("127.0.0.1", "/#top").parse().unwrap())
        .unwrap();
    view.poll();
    view.evaluate("location.hash='again'").unwrap();
    view.poll();
    assert_eq!(
        view.evaluate_json("log").unwrap(),
        "[\"hash:#section\",\"hash:#top\",\"hash:#again\"]"
    );
    view.traverse(-1);
    assert_eq!(view.url.fragment(), Some("top"));
    assert_eq!(server.requests("/").len(), 1);
}
