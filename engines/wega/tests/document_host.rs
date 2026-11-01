//! Parser, realm teardown and task-source regressions for Step 06.
mod support;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use support::{Reply, Server, engine, open, settle, wait};
use wega_engine::{Engine, Limits, NetworkPolicy, Url};
#[test]
fn parser_checkpoints_run_microtasks_without_dispatching_timer_tasks() {
    let engine = engine();
    let mut view = open(&engine, "about:blank");
    view.set_html("<script>globalThis.order=['first'];Promise.resolve().then(()=>order.push('microtask'));setTimeout(()=>order.push('timer'),0)</script><script>order.push('second')</script>").unwrap();
    assert_eq!(
        view.evaluate_json("order").unwrap(),
        "[\"first\",\"microtask\",\"second\"]"
    );
    wait(&mut view, "order.includes('timer')||null");
    assert_eq!(
        view.evaluate_json("order").unwrap(),
        "[\"first\",\"microtask\",\"second\",\"timer\"]"
    );
}
#[test]
fn provisional_navigation_suspends_the_old_realm_and_replacement_drops_it() {
    let server = Server::new(|request| match request.path.as_str() {
        "/new" => Reply::html("<title>new</title><script>globalThis.fresh=true</script>")
            .delay(Duration::from_millis(150)),
        _ => Reply::html("<title>old</title>"),
    });
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/old"));
    view.evaluate("setTimeout(()=>fetch('/old-callback'),0)")
        .unwrap();
    view.load(Url::parse(&server.url("127.0.0.1", "/new")).unwrap())
        .unwrap();
    view.poll();
    assert!(view.evaluate_json("1").unwrap_err().contains("suspended"));
    settle(&mut view);
    assert_eq!(view.evaluate_json("fresh").unwrap(), "true");
    assert_eq!(server.requests("/old-callback").len(), 0);
}
#[test]
fn explicit_document_replacement_cancels_a_pending_navigation() {
    let server =
        Server::new(|_| Reply::html("<title>late</title>").delay(Duration::from_millis(100)));
    let engine = engine();
    let mut view = open(&engine, "about:blank");
    view.load(Url::parse(&server.url("127.0.0.1", "/")).unwrap())
        .unwrap();
    view.set_html("<title>replacement</title><script>globalThis.value=42</script>")
        .unwrap();
    let until = Instant::now() + Duration::from_millis(200);
    while Instant::now() < until {
        view.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(view.title(), "replacement");
    assert_eq!(view.evaluate_json("value").unwrap(), "42");
}
#[test]
fn ordinary_script_failure_does_not_disable_the_following_script_or_timer() {
    let engine = engine();
    let mut view = open(&engine, "about:blank");
    view.set_html("<script>Promise.resolve().then(()=>globalThis.checkpoint=true);throw Error('ordinary')</script><script>globalThis.after=checkpoint;setTimeout(()=>globalThis.timer=true,0)</script>").unwrap();
    settle(&mut view);
    assert_eq!(view.evaluate_json("after").unwrap(), "true");
    assert_eq!(wait(&mut view, "globalThis.timer??null"), "true");
    assert!(view.take_script_failure().is_none());
}
#[test]
fn hostile_to_json_is_terminated_and_requires_explicit_recovery() {
    let engine = Engine::new(
        Limits {
            script_time: Duration::from_millis(50),
            ..Default::default()
        },
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap();
    let mut view = open(&engine, "about:blank");
    assert!(view.evaluate_json("({toJSON(){while(true){}}})").is_err());
    assert!(
        view.take_script_failure()
            .unwrap()
            .contains("JSON serialization")
    );
    assert!(view.evaluate_json("1").is_err());
    view.set_html("<p>recovered</p>").unwrap();
    assert_eq!(view.evaluate_json("1+1").unwrap(), "2");
}

#[test]
fn insecure_crypto_is_restricted_on_the_instance_and_its_prototype() {
    let engine = engine();
    let mut view = open(&engine, "about:blank");
    assert_eq!(view.evaluate_json("[isSecureContext, typeof crypto.subtle, typeof Object.getPrototypeOf(crypto).subtle, typeof Object.getPrototypeOf(crypto).randomUUID]").unwrap(), "[false,\"undefined\",\"undefined\",\"undefined\"]");
}
