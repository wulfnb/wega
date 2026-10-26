//! Script loading: parser-blocking, defer, async, script-inserted scripts,
//! document.write, currentScript, load timing, size and time limits.
mod support;
use std::time::Duration;
use support::{Reply, Server, engine, open, wait};

fn scripts() -> Server {
    Server::new(|seen| match seen.path.as_str() {
        "/dep.js" => Reply::ok(
            "text/javascript",
            "globalThis.depValue=7;globalThis.depScript=document.currentScript&&document.currentScript.id;",
        ),
        "/chunk.js" => Reply::ok(
            "text/javascript",
            "(self.webpackChunk=self.webpackChunk||[]).push([[1],{42:()=>'chunk loaded'}]);",
        ),
        "/slow-defer.js" => {
            Reply::ok("text/javascript", "order.push('defer1')").delay(Duration::from_millis(250))
        }
        "/fast-defer.js" => Reply::ok("text/javascript", "order.push('defer2')"),
        "/blocking.js" => Reply::ok(
            "text/javascript",
            "order.push('blocking:'+(document.getElementById('later')===null))",
        )
        .delay(Duration::from_millis(100)),
        "/async.js" => Reply::ok("text/javascript", "order.push('async')"),
        "/ordered-a.js" => {
            Reply::ok("text/javascript", "order.push('a')").delay(Duration::from_millis(200))
        }
        "/ordered-b.js" => Reply::ok("text/javascript", "order.push('b')"),
        "/large.js" => {
            let mut body = String::from("globalThis.large=");
            body.push_str(&format!("'{}'.length;", "x".repeat(3 * 1024 * 1024)));
            Reply::ok("text/javascript", body)
        }
        "/missing.js" => Reply::ok("text/plain", "").status("404 Not Found"),
        "/slow.png" => Reply::ok("image/png", include_bytes!("fixtures/pixel.png").to_vec())
            .delay(Duration::from_millis(300)),
        "/defer-page" => Reply::html(
            "<script>globalThis.order=[];document.addEventListener('DOMContentLoaded',()=>order.push('DOMContentLoaded'))</script>\
             <script defer src=/slow-defer.js></script><script src=/blocking.js></script>\
             <script>order.push('inline')</script><script defer src=/fast-defer.js></script><p id=later>",
        ),
        "/image-page" => Reply::html(
            "<script>globalThis.events=[];document.addEventListener('DOMContentLoaded',()=>events.push('ready'));\
             addEventListener('load',()=>events.push('load'))</script><img id=pic src=/slow.png onload=\"events.push('img')\">",
        ),
        _ => Reply::html("<title>scripts</title>"),
    })
}

#[test]
fn script_inserted_external_scripts_run_and_fire_load() {
    let server = scripts();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.evaluate("const s=document.createElement('script');s.id='dynamic';s.src='/dep.js';s.onload=()=>globalThis.loaded=depValue;document.head.appendChild(s);globalThis.isAsync=s.async").unwrap();
    assert_eq!(wait(&mut view, "globalThis.loaded??null"), "7");
    assert_eq!(
        view.evaluate_json("[isAsync,depScript]").unwrap(),
        "[true,\"dynamic\"]"
    );
    // A webpack-style JSONP chunk loader.
    view.evaluate("const c=document.createElement('script');c.src='/chunk.js';c.onload=()=>globalThis.chunk=self.webpackChunk[0][1][42]();document.body.appendChild(c)").unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.chunk??null"),
        "\"chunk loaded\""
    );
    // A failed fetch fires error, not load.
    view.evaluate("const m=document.createElement('script');m.src='/missing.js';m.onerror=()=>globalThis.failed=true;m.onload=()=>globalThis.failed=false;document.body.appendChild(m)").unwrap();
    assert_eq!(wait(&mut view, "globalThis.failed??null"), "true");
    // Each script element runs once, even when moved.
    view.evaluate("document.body.appendChild(s)").unwrap();
    view.poll();
    assert_eq!(server.requests("/dep.js").len(), 1);
}

#[test]
fn script_inserted_scripts_keep_order_only_when_async_is_false() {
    let server = scripts();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.evaluate("globalThis.order=[];for(const name of ['a','b']){const s=document.createElement('script');s.async=false;s.src='/ordered-'+name+'.js';document.head.appendChild(s)}").unwrap();
    wait(&mut view, "order.length===2||null");
    assert_eq!(view.evaluate_json("order").unwrap(), "[\"a\",\"b\"]");
    view.evaluate("globalThis.order=[];for(const name of ['a','b']){const s=document.createElement('script');s.src='/ordered-'+name+'.js';document.head.appendChild(s)}").unwrap();
    wait(&mut view, "order.length===2||null");
    assert_eq!(
        view.evaluate_json("order").unwrap(),
        "[\"b\",\"a\"]",
        "async scripts run as they arrive"
    );
}

#[test]
fn inline_inserted_scripts_run_synchronously_as_classic_scripts() {
    let server = scripts();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    assert_eq!(view.evaluate_json("(()=>{const s=document.createElement('script');s.textContent='let shared=5;globalThis.ranWith=document.currentScript===s0';globalThis.s0=s;document.body.appendChild(s);return [globalThis.ranWith,typeof shared]})()").unwrap(), "[true,\"number\"]");
    // innerHTML never runs scripts.
    assert_eq!(view.evaluate_json("(()=>{document.body.insertAdjacentHTML('beforeend','<script>globalThis.fromHtml=true<\\/script>');return globalThis.fromHtml??false})()").unwrap(), "false");
}

#[test]
fn parser_scripts_see_the_partial_document_and_document_write() {
    let engine = engine();
    let server = scripts();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.set_html("<div id=a></div><script>globalThis.partial=[!!document.getElementById('a'),document.getElementById('b')===null]</script><div id=b></div>\
                   <script>document.write('<p id=written>w</p>')</script><p id=after>a</p>").unwrap();
    view.poll();
    assert_eq!(view.evaluate_json("partial").unwrap(), "[true,true]");
    assert_eq!(
        view.evaluate_json("[...document.querySelectorAll('p')].map(p=>p.id)")
            .unwrap(),
        "[\"written\",\"after\"]"
    );
}

#[test]
fn defer_and_blocking_scripts_run_in_document_order_before_domcontentloaded() {
    let server = scripts();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/defer-page"));
    assert_eq!(
        view.evaluate_json("order").unwrap(),
        "[\"blocking:true\",\"inline\",\"defer1\",\"defer2\",\"DOMContentLoaded\"]"
    );
}

#[test]
fn load_waits_for_images_and_fires_after_domcontentloaded() {
    let server = scripts();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/image-page"));
    assert_eq!(
        view.evaluate_json("events").unwrap(),
        "[\"ready\",\"img\",\"load\"]"
    );
}

#[test]
fn large_scripts_run_and_slow_scripts_are_reported_not_killed() {
    let server = scripts();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.evaluate(
        "const s=document.createElement('script');s.src='/large.js';document.head.appendChild(s)",
    )
    .unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.large??null"),
        (3 * 1024 * 1024).to_string()
    );
    view.set_html("<script>const end=Date.now()+300;while(Date.now()<end){}globalThis.first=true</script><script>globalThis.second=true</script>").unwrap();
    view.poll();
    assert_eq!(view.evaluate_json("[first,second]").unwrap(), "[true,true]");
    assert!(
        view.notices.iter().any(|n| n.contains("slow script")),
        "{:?}",
        view.notices
    );
}

#[test]
fn more_than_sixty_four_scripts_all_run() {
    let engine = engine();
    let server = scripts();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.set_html(&"<script>globalThis.count=(globalThis.count||0)+1</script>".repeat(100))
        .unwrap();
    view.poll();
    assert_eq!(view.evaluate_json("count").unwrap(), "100");
}
