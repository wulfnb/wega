use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use wega_engine::{Engine, Limits, NetworkPolicy, Url};
fn page(limits: Limits) -> wega_engine::View {
    Engine::new(
        limits,
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .create_view(Url::parse("about:blank").unwrap())
    .unwrap()
}
#[test]
fn document_titles_collapse_html_whitespace_but_preserve_nonbreaking_spaces() {
    let mut view = page(Limits::default());
    view.set_html("<title>\n    Rust\t Programming\r\n Language&nbsp;!  </title>")
        .unwrap();
    assert_eq!(view.title(), "Rust Programming Language\u{a0}!");
    assert_eq!(
        view.evaluate_json("document.title").unwrap(),
        "\"Rust Programming Language\u{a0}!\""
    );
    view.evaluate("document.title='  Updated\\n title  '")
        .unwrap();
    assert_eq!(view.title(), "Updated title");
}
#[test]
fn visible_frames_report_compatibility_once_but_hidden_feature_probes_do_not() {
    let mut view = page(Limits::default());
    view.set_html("<iframe id=f style='display:none'></iframe><iframe width=1 height=1></iframe>")
        .unwrap();
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    assert!(view.compatibility.is_none());
    view.evaluate("document.getElementById('f').style.display='block'")
        .unwrap();
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    assert!(
        view.compatibility
            .take()
            .unwrap()
            .contains("embedded frames")
    );
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    assert!(view.compatibility.is_none());
    view.set_html("<iframe></iframe>").unwrap();
    view.paint(300, 200, 1.0, 1.0, false).unwrap();
    assert!(
        view.compatibility.is_some(),
        "new documents get their own compatibility report"
    );
}
#[test]
fn real_dom_javascript_and_cpu_pixels_share_one_document() {
    let mut view = page(Limits::default());
    view.set_html(r#"<!doctype html><title>Before</title><style>body{margin:0}button{border:0;background:#00ff00;width:180px;height:80px}</style><button id="counter">0</button><script>
      const button=document.getElementById('counter');
      if(button!==document.querySelector('#counter'))throw Error('Wrapper identity lost');
      button.addEventListener('click',()=>{button.textContent=String(Number(button.textContent)+1);document.title='Clicked';});
      document.addEventListener('DOMContentLoaded',()=>document.title='Ready');
    </script>"#).unwrap();
    view.poll();
    assert_eq!(view.title(), "Ready", "{:?}", view.notices);
    let frame = view.paint(200, 120, 1.0, 1.0, false).unwrap();
    let pixel = &frame.rgba[(20 * 200 + 20) * 4..(20 * 200 + 20) * 4 + 4];
    assert!(
        pixel[1] > 240 && pixel[0] < 10 && pixel[2] < 10,
        "Actual green button pixels: {pixel:?}"
    );
    view.click(30.0, 30.0);
    view.poll();
    assert_eq!(view.text("#counter").as_deref(), Some("1"));
    assert_eq!(view.title(), "Clicked");
    assert!(view.heap_bytes() > 0);
    assert!(view.needs_paint());
    view.freeze(true);
    assert!(!view.needs_paint());
    assert!(view.next_wakeup().is_none());
    view.freeze(false);
    assert!(view.needs_paint());
}
#[test]
fn runaway_script_is_interrupted_and_a_fresh_page_recovers() {
    let mut view = page(Limits {
        script_time: Duration::from_millis(30),
        ..Limits::default()
    });
    view.set_html("<title>Busy</title><script>while(true){}</script>")
        .unwrap();
    let start = Instant::now();
    view.poll();
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(!view.notices.is_empty());
    view.set_html("<title>Recovered</title><p>Fresh page</p>")
        .unwrap();
    view.poll();
    assert_eq!(view.title(), "Recovered");
    assert!(view.paint(100, 100, 1.0, 1.0, false).is_ok());
}
#[test]
fn native_dom_budget_and_cycles_are_rejected() {
    let mut view = page(Limits {
        dom_nodes: 24,
        ..Limits::default()
    });
    view.set_html("<body><div id=a><div id=b></div></div>")
        .unwrap();
    view.poll();
    view.evaluate("let rejected=false;try{document.getElementById('b').appendChild(document.getElementById('a'));}catch{rejected=true;}if(!rejected)throw Error('DOM cycle accepted');").unwrap();
    view.set_html(&"<div>x</div>".repeat(30)).unwrap();
    view.poll();
    assert_eq!(view.title(), "Page unavailable");
    assert!(
        view.notices
            .iter()
            .any(|n| n.contains("DOM node limit exceeded"))
    );
    assert!(view.paint(u32::MAX, u32::MAX, 1.0, 1.0, false).is_err());
}
#[test]
fn policy_blocks_host_boundaries_and_offline_requests() {
    let p = NetworkPolicy {
        blocked_hosts: "example.test".into(),
        ..Default::default()
    };
    assert!(!p.permits(&Url::parse("https://child.example.test/").unwrap()));
    assert!(p.permits(&Url::parse("https://notexample.test/").unwrap()));
    assert!(!p.permits(&Url::parse("file:///tmp/private").unwrap()));
    assert!(
        !NetworkPolicy {
            offline: true,
            ..Default::default()
        }
        .permits(&Url::parse("https://safe.test/").unwrap())
    );
}
#[test]
fn heap_exhaustion_is_local_and_timers_pause_in_background() {
    let mut view = page(Limits {
        js_heap_bytes: 512 * 1024,
        ..Limits::default()
    });
    view.set_html(
        "<title>Heap cap</title><script>globalThis.leak=new Array(5000000).fill(7)</script>",
    )
    .unwrap();
    view.poll();
    assert!(!view.notices.is_empty());
    view.set_html(
        "<title>Waiting</title><script>setTimeout(()=>document.title='Timer fired',50)</script>",
    )
    .unwrap();
    view.poll();
    view.freeze(true);
    // Leave enough margin for loaded CI hosts to finish the initial task,
    // then wait beyond the timer deadline while the view is frozen.
    std::thread::sleep(Duration::from_millis(70));
    view.poll();
    assert_eq!(view.title(), "Waiting");
    view.freeze(false);
    view.poll();
    assert_eq!(view.title(), "Timer fired", "{:?}", view.notices);
}
#[test]
fn failed_navigation_replaces_old_content_and_policy_updates_take_effect() {
    let engine = Engine::new(
        Limits::default(),
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap();
    let mut view = engine
        .create_view(Url::parse("about:blank").unwrap())
        .unwrap();
    view.set_html("<title>Old content</title><p>Private old page</p>")
        .unwrap();
    engine.set_policy(NetworkPolicy {
        offline: true,
        ..Default::default()
    });
    view.load(Url::parse("https://blocked.test/").unwrap())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while view.loading && Instant::now() < deadline {
        view.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(view.title(), "Page unavailable");
    assert!(!view.text("body").unwrap().contains("Private old page"));
    assert!(
        view.notices
            .iter()
            .any(|message| message.contains("policy"))
    );
}

#[test]
fn startup_snapshot_is_private_per_document_and_freezing_preserves_live_state() {
    let mut first = page(Limits::default());
    first
        .set_html("<input id=draft value=initial><script>globalThis.privateValue=42</script>")
        .unwrap();
    first.poll();
    first
        .evaluate("document.getElementById('draft').value='unsaved 文'")
        .unwrap();
    first.freeze(true);
    first.poll();
    first.freeze(false);
    first.poll();
    assert_eq!(
        first
            .evaluate_json("[privateValue,document.getElementById('draft').value]")
            .unwrap(),
        "[42,\"unsaved 文\"]"
    );
    // Production views own independent DOM threads/processes. Rusty V8 enters
    // isolates at creation and requires nested isolates to be dropped in reverse
    // creation order on a thread; do not reset an outer isolate while one is live.
    let isolated = std::thread::spawn(|| {
        let mut second = page(Limits::default());second.poll();
        second.evaluate_json("[typeof privateValue,Object.getOwnPropertyNames(globalThis).filter(key=>key.startsWith('__wega_bootstrap_')).length]").unwrap()
    }).join().unwrap();
    assert_eq!(isolated, "[\"undefined\",0]");
    first.set_html("<title>New document</title>").unwrap();
    first.poll();
    assert_eq!(
        first.evaluate_json("typeof privateValue").unwrap(),
        "\"undefined\""
    );
}

#[test]
fn parallel_style_threshold_preserves_updates_on_large_documents() {
    let mut view = page(Limits::default());
    view.set_html(&format!(
        "<style>body{{margin:0}}#first{{height:16px;background:red}}</style><div id=first></div>{}",
        "<div><span>large tree</span></div>".repeat(600)
    ))
    .unwrap();
    view.poll();
    assert!(view.node_count() > 512);
    view.paint(64, 64, 1.0, 1.0, false).unwrap();
    {
        let pool = style::global_style_data::STYLE_THREAD_POOL.pool();
        assert_eq!(
            pool.as_ref().map(|pool| pool.current_num_threads()),
            Some(2)
        );
    }
    view.evaluate("document.getElementById('first').style.backgroundColor='lime'")
        .unwrap();
    let frame = view.paint(64, 64, 1.0, 1.0, false).unwrap();
    assert_eq!(
        &frame.rgba[(8 * 64 + 8) * 4..(8 * 64 + 8) * 4 + 4],
        &[0, 255, 0, 255]
    );
}

#[test]
fn replacing_content_returns_its_node_and_text_budget() {
    // 50 x 100 KB of text would exceed a 1 MiB budget if replaced text were never credited.
    let mut view = page(Limits {
        dom_text_bytes: 1024 * 1024,
        dom_nodes: 200,
        ..Limits::default()
    });
    view.set_html("<div id=box></div>").unwrap();
    view.poll();
    // Text node data, element textContent and innerHTML replacement; script holds
    // no wrappers for the replaced nodes (held nodes stay charged until collected).
    view.evaluate("const box=document.getElementById('box');const text='x'.repeat(100000);const node=document.createTextNode('');box.append(node);for(let i=0;i<50;i++)node.data=text+i;for(let i=0;i<50;i++)box.textContent=text+i;for(let i=0;i<500;i++)box.textContent='n'+i;for(let i=0;i<50;i++)box.innerHTML='<p>'+text+'</p>';globalThis.done=true").unwrap();
    assert_eq!(
        view.evaluate_json("done").unwrap(),
        "true",
        "{:?}",
        view.notices
    );
    let (nodes, bytes) = view.dom_budget().unwrap();
    assert!(nodes < 200, "replaced nodes stay counted: {nodes}");
    assert!(bytes < 512 * 1024, "replaced text stays charged: {bytes}");
}
