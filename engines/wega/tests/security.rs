//! Same-origin policy, CORS, SameSite cookies, CSP and Referer, against real servers.
mod support;
use support::{Reply, Server, engine, open, wait};

/// A cross-origin API on a second port: same site, different origin.
fn api(allow_origin: Option<&'static str>) -> Server {
    Server::new(move |seen| {
        let mut reply = match seen.path.as_str() {
            "/set-cookie" => {
                Reply::ok("text/plain", "set").header("Set-Cookie", "session=api; Path=/")
            }
            "/echo-cookie" => Reply::ok(
                "text/plain",
                seen.header("cookie").unwrap_or("none").to_owned(),
            ),
            "/headers" => Reply::ok("application/json", "{}")
                .header("X-Secret", "hidden")
                .header("X-Public", "shown")
                .header("Access-Control-Expose-Headers", "X-Public"),
            _ => Reply::ok("application/json", "{\"answer\":42}"),
        };
        if let Some(origin) = allow_origin {
            reply = reply.header("Access-Control-Allow-Origin", origin);
            if origin != "*" {
                reply = reply.header("Access-Control-Allow-Credentials", "true");
            }
        }
        if seen.method == "OPTIONS" && seen.path == "/preflight-ok" {
            reply = reply
                .header("Access-Control-Allow-Methods", "PUT")
                .header("Access-Control-Allow-Headers", "x-custom");
        }
        reply
    })
}
fn site() -> Server {
    Server::new(|_| Reply::html("<title>site</title>"))
}

#[test]
fn cross_origin_reads_need_access_control_allow_origin() {
    let page = site();
    let closed = api(None);
    let engine = engine();
    let mut view = open(&engine, &page.url("127.0.0.1", "/"));
    let target = closed.url("127.0.0.1", "/data");
    view.evaluate(&format!(
        "fetch('{target}').then(r=>r.json()).then(v=>globalThis.result='read '+v.answer,e=>globalThis.result='blocked: '+e.message)"
    ))
    .unwrap();
    let result = wait(&mut view, "globalThis.result??null");
    assert!(result.contains("blocked"), "{result}");
    assert!(result.contains("Access-Control-Allow-Origin"), "{result}");
    // The request itself was sent (CORS protects the response, not the server).
    assert_eq!(closed.requests("/data").len(), 1);

    let open_api = api(Some("*"));
    let target = open_api.url("127.0.0.1", "/data");
    view.evaluate(&format!(
        "fetch('{target}').then(r=>r.json()).then(v=>globalThis.open='read '+v.answer,e=>globalThis.open='blocked: '+e.message)"
    ))
    .unwrap();
    assert_eq!(wait(&mut view, "globalThis.open??null"), "\"read 42\"");
    // A wildcard origin never allows credentials.
    view.evaluate(&format!(
        "fetch('{target}',{{credentials:'include'}}).then(()=>globalThis.credentialed='read',e=>globalThis.credentialed='blocked: '+e.message)"
    ))
    .unwrap();
    assert!(wait(&mut view, "globalThis.credentialed??null").contains("wildcard"));
}

#[test]
fn credentials_cross_origin_only_with_an_exact_origin_and_allow_credentials() {
    let page = site();
    let origin = page.url("127.0.0.1", "").trim_end_matches('/').to_owned();
    let leaked: &'static str = Box::leak(origin.clone().into_boxed_str());
    let api = api(Some(leaked));
    let engine = engine();
    // The API sets its own first-party cookie by a visit.
    open(&engine, &api.url("127.0.0.1", "/set-cookie"));
    let mut view = open(&engine, &page.url("127.0.0.1", "/"));
    let echo = api.url("127.0.0.1", "/echo-cookie");
    view.evaluate(&format!(
        "fetch('{echo}').then(r=>r.text()).then(v=>globalThis.plain=v,e=>globalThis.plain='error '+e.message)"
    ))
    .unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.plain??null"),
        "\"none\"",
        "same-origin credentials mode sends no cookies cross-origin"
    );
    view.evaluate(&format!(
        "fetch('{echo}',{{credentials:'include'}}).then(r=>r.text()).then(v=>globalThis.included=v,e=>globalThis.included='error '+e.message)"
    ))
    .unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.included??null"),
        "\"session=api\""
    );
    // XHR withCredentials uses the same rules.
    view.evaluate(&format!(
        "const x=new XMLHttpRequest();x.open('GET','{echo}');x.withCredentials=true;x.onload=()=>globalThis.xhr=x.responseText;x.onerror=()=>globalThis.xhr='error';x.send()"
    ))
    .unwrap();
    assert_eq!(wait(&mut view, "globalThis.xhr??null"), "\"session=api\"");
}

#[test]
fn non_simple_requests_are_preflighted_and_respect_the_answer() {
    let page = site();
    let api = api(Some("*"));
    let engine = engine();
    let mut view = open(&engine, &page.url("127.0.0.1", "/"));
    let denied = api.url("127.0.0.1", "/preflight-denied");
    view.evaluate(&format!(
        "fetch('{denied}',{{method:'PUT',headers:{{'X-Custom':'1'}}}}).then(()=>globalThis.denied='sent',e=>globalThis.denied=e.message)"
    ))
    .unwrap();
    assert!(wait(&mut view, "globalThis.denied??null").contains("preflight"));
    let requests = api.requests("/preflight-denied");
    assert_eq!(requests.len(), 1, "only the preflight may reach the server");
    assert_eq!(requests[0].method, "OPTIONS");
    assert_eq!(
        requests[0].header("access-control-request-method"),
        Some("PUT")
    );
    assert_eq!(
        requests[0].header("access-control-request-headers"),
        Some("x-custom")
    );

    let allowed = api.url("127.0.0.1", "/preflight-ok");
    view.evaluate(&format!(
        "fetch('{allowed}',{{method:'PUT',headers:{{'X-Custom':'1'}}}}).then(r=>globalThis.allowed=r.status,e=>globalThis.allowed=e.message)"
    ))
    .unwrap();
    assert_eq!(wait(&mut view, "globalThis.allowed??null"), "200");
    let methods: Vec<String> = api
        .requests("/preflight-ok")
        .iter()
        .map(|r| r.method.clone())
        .collect();
    assert_eq!(methods, ["OPTIONS", "PUT"]);
    // A simple request is not preflighted, and carries Origin.
    let simple = api.url("127.0.0.1", "/simple");
    view.evaluate(&format!("fetch('{simple}').then(()=>globalThis.simple=1)"))
        .unwrap();
    wait(&mut view, "globalThis.simple??null");
    let requests = api.requests("/simple");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(
        requests[0].header("origin"),
        Some(page.url("127.0.0.1", "").trim_end_matches('/'))
    );
}

#[test]
fn modes_opaque_responses_and_exposed_headers() {
    let page = site();
    let api = api(Some("*"));
    let closed = self::api(None);
    let engine = engine();
    let mut view = open(&engine, &page.url("127.0.0.1", "/"));
    let target = closed.url("127.0.0.1", "/data");
    view.evaluate(&format!(
        "fetch('{target}',{{mode:'no-cors'}}).then(async r=>globalThis.opaque=[r.type,r.status,(await r.text()).length],e=>globalThis.opaque=e.message)"
    ))
    .unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.opaque??null"),
        "[\"opaque\",0,0]"
    );
    view.evaluate(&format!(
        "fetch('{target}',{{mode:'same-origin'}}).then(()=>globalThis.same='read',e=>globalThis.same=e.message)"
    ))
    .unwrap();
    assert!(wait(&mut view, "globalThis.same??null").contains("same-origin"));
    let headers = api.url("127.0.0.1", "/headers");
    view.evaluate(&format!(
        "fetch('{headers}').then(r=>globalThis.exposed=[r.type,r.headers.get('x-secret'),r.headers.get('x-public'),r.headers.get('content-type')])"
    ))
    .unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.exposed??null"),
        "[\"cors\",null,\"shown\",\"application/json\"]"
    );
}

#[test]
fn samesite_cookies_stay_on_their_site() {
    // localhost and 127.0.0.1 are different sites.
    let api = Server::new(|seen| match seen.path.as_str() {
        "/set" => Reply::ok("text/plain", "set")
            .header("Set-Cookie", "lax=1; Path=/")
            .header("Set-Cookie", "strict=1; Path=/; SameSite=Strict"),
        _ => Reply::ok(
            "text/plain",
            seen.header("cookie").unwrap_or("none").to_owned(),
        )
        .header(
            "Access-Control-Allow-Origin",
            seen.header("origin").unwrap_or("*"),
        )
        .header("Access-Control-Allow-Credentials", "true"),
    });
    let page = site();
    let engine = engine();
    open(&engine, &api.url("localhost", "/set"));
    let first_party = open(&engine, &api.url("localhost", "/echo"));
    let body = first_party.text("pre").unwrap_or_default();
    assert!(
        body.contains("lax=1") && body.contains("strict=1"),
        "{body}"
    );
    let mut view = open(&engine, &page.url("127.0.0.1", "/"));
    let echo = api.url("localhost", "/echo");
    view.evaluate(&format!(
        "fetch('{echo}',{{credentials:'include'}}).then(r=>r.text(),e=>'blocked').then(v=>globalThis.cross=v)"
    ))
    .unwrap();
    wait(&mut view, "globalThis.cross??null");
    let sent = api.requests("/echo");
    assert_eq!(
        sent.last().unwrap().header("cookie"),
        None,
        "Lax and Strict cookies are not sent cross-site"
    );
    assert_eq!(view.evaluate_json("globalThis.cross").unwrap(), "\"none\"");
}

#[test]
fn content_security_policy_governs_scripts_eval_and_connections() {
    let other = api(Some("*"));
    let target = other.url("127.0.0.1", "/data");
    let html = format!(
        "<title>csp</title><script nonce=abc>globalThis.nonced=true</script><script>globalThis.unnonced=true</script>\
         <script nonce=abc>try{{eval('1');globalThis.evaled=true}}catch(e){{globalThis.evaled=e.name}}\
         fetch('{target}').then(()=>globalThis.connect='allowed',e=>globalThis.connect=e.message)</script>"
    );
    let page = Server::new(move |_| {
        Reply::html(&html).header(
            "Content-Security-Policy",
            "script-src 'nonce-abc'; connect-src 'self'",
        )
    });
    let engine = engine();
    let mut view = open(&engine, &page.url("127.0.0.1", "/"));
    assert_eq!(
        view.evaluate_json(
            "[globalThis.nonced??false,globalThis.unnonced??false,globalThis.evaled]"
        )
        .unwrap(),
        "[true,false,\"EvalError\"]"
    );
    assert!(wait(&mut view, "globalThis.connect??null").contains("connect-src"));
    assert!(other.requests("/data").is_empty());
    assert!(
        view.notices
            .iter()
            .any(|n| n.contains("Content Security Policy"))
    );
}

#[test]
fn referer_follows_strict_origin_when_cross_origin() {
    let api = api(Some("*"));
    let page = site();
    let engine = engine();
    let mut view = open(&engine, &page.url("127.0.0.1", "/path/page?q=1"));
    let same = page.url("127.0.0.1", "/same");
    let cross = api.url("127.0.0.1", "/cross");
    view.evaluate(&format!(
        "Promise.all([fetch('{same}'),fetch('{cross}')]).then(()=>globalThis.done=1)"
    ))
    .unwrap();
    wait(&mut view, "globalThis.done??null");
    assert_eq!(
        page.requests("/same")[0].header("referer"),
        Some(page.url("127.0.0.1", "/path/page?q=1").as_str())
    );
    assert_eq!(
        api.requests("/cross")[0].header("referer"),
        Some(page.url("127.0.0.1", "/").as_str())
    );
}

#[test]
fn broker_honors_fetch_cache_modes_and_live_engine_policy_without_reloading() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use wega_engine::{Engine, Limits, NetworkPolicy};
    let count = Arc::new(AtomicUsize::new(0));
    let requests = count.clone();
    let server = Server::new(move |seen| {
        if seen.path == "/public" {
            return Reply::ok(
                "text/plain",
                requests.fetch_add(1, Ordering::SeqCst).to_string(),
            )
            .header("Cache-Control", "public, max-age=60");
        }
        Reply::html("<title>Broker policy</title><input id=draft value=initial>")
    });
    let directory = tempfile::tempdir().unwrap();
    let broker = wega_network::Broker::new().unwrap();
    let session = broker
        .open_session(
            directory.path().to_owned(),
            NetworkPolicy {
                allow_loopback: true,
                ..Default::default()
            },
        )
        .unwrap();
    let (address, token) = session.endpoint();
    let keep = (session, directory);
    let engine = Engine::brokered(
        Limits::default(),
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        Arc::new(move || {
            let _ = &keep;
        }),
        &address,
        token,
    )
    .unwrap();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.evaluate("document.getElementById('draft').value='unsaved';(async()=>{let results=[];for(const cache of ['default','default','no-store','no-store'])results.push(await(await fetch('/public',{cache})).text());globalThis.cacheValues=results})()").unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.cacheValues??null"),
        "[\"0\",\"0\",\"1\",\"2\"]"
    );
    assert_eq!(count.load(Ordering::SeqCst), 3);
    // The library's local policy must also apply when the broker's session
    // still allows the URL (the native browser updates both).
    engine.set_policy(NetworkPolicy {
        offline: true,
        ..Default::default()
    });
    view.evaluate("fetch('/blocked').then(()=>globalThis.policyResult='allowed',()=>globalThis.policyResult='blocked')").unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.policyResult??null"),
        "\"blocked\""
    );
    assert_eq!(server.requests("/blocked").len(), 0);
    assert_eq!(
        view.evaluate_json("document.getElementById('draft').value")
            .unwrap(),
        "\"unsaved\""
    );
}

#[test]
fn contextual_fragment_scripts_obey_csp_nonce_checks() {
    let page = Server::new(|_| {
        Reply::html("<main></main>").header("Content-Security-Policy", "script-src 'nonce-allowed'")
    });
    let engine = engine();
    let mut view = open(&engine, &page.url("127.0.0.1", "/"));
    view.evaluate(r#"document.querySelector('main').append(document.createRange().createContextualFragment('<script>globalThis.blockedFragment=true</script><script nonce="allowed">globalThis.allowedFragment=true</script>'))"#).unwrap();
    assert_eq!(
        view.evaluate_json("[globalThis.blockedFragment??false,globalThis.allowedFragment??false]")
            .unwrap(),
        "[false,true]"
    );
}
