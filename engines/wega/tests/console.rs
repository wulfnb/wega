//! Console capture, value previews and inspector evaluation against real V8 in-process.
//! Hostile pages are the point: previews must never run page code.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use wega_engine::{Engine, Limits, NetworkPolicy, PagePort, Url, View};
use wega_protocol::inspector::{
    CONSOLE_RING_BYTES, CONSOLE_RING_ENTRIES, ConsoleLevel, ConsoleMessage, EvalOutcome,
    ExceptionKind, MAX_MESSAGE_BYTES, Preview, PreviewKind,
};

fn page_with(html: &str, limits: Limits) -> View {
    let mut view = Engine::new(
        limits,
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
    view.url = Url::parse("https://fixture.test/console.html").unwrap();
    view.set_html(html).unwrap();
    for _ in 0..8 {
        view.poll();
    }
    view
}
fn page(html: &str) -> View {
    page_with(html, Limits::default())
}
fn messages(view: &View) -> Vec<ConsoleMessage> {
    view.console_read(0, usize::MAX, usize::MAX)
}
fn value(view: &mut View, expression: &str) -> Preview {
    match PagePort::evaluate(view, expression).unwrap_or_else(|e| panic!("{expression}: {e}")) {
        EvalOutcome::Value { preview } => {
            assert!(preview.is_bounded(), "{expression}: {preview:?}");
            preview
        }
        EvalOutcome::Exception { details } => panic!("{expression} threw {details:?}"),
    }
}
fn prop<'a>(preview: &'a Preview, key: &str) -> &'a Preview {
    &preview
        .props
        .iter()
        .find(|p| p.key == key)
        .unwrap_or_else(|| panic!("no {key} in {preview:?}"))
        .value
}
fn hits(view: &mut View) -> String {
    view.evaluate_json("JSON.stringify(globalThis.hits)")
        .unwrap()
}
const HOSTILE: &str = "globalThis.hits={getter:0,trap:0,toJSON:0,toString:0,valueOf:0,prim:0,tamper:0};\
 globalThis.o={get g(){hits.getter++;return 1},toJSON(){hits.toJSON++},toString(){hits.toString++;return 'x'},valueOf(){hits.valueOf++;return 1},[Symbol.toPrimitive](){hits.prim++;return 1},plain:1};\
 globalThis.p=new Proxy({a:1},new Proxy({},{get(t,k){hits.trap++;return undefined}}));\
 globalThis.pf=new Proxy(function f(){},{get(){hits.trap++},getPrototypeOf(){hits.trap++;return null},ownKeys(){hits.trap++;return []},getOwnPropertyDescriptor(){hits.trap++}});\
 class K{get acc(){hits.getter++}};globalThis.k=new K();\
 globalThis.e=new Error('boom');Object.defineProperty(e,'message',{get(){hits.getter++;return 'm'}});\
 globalThis.arr=[1,2,3];Object.defineProperty(arr,1,{get(){hits.getter++;return 2}});\
 globalThis.nested={a:{b:{c:{d:1}}}};true";

#[test]
fn every_console_method_is_captured_with_arguments_level_and_source() {
    let mut view = page(
        "<script>console.log('a',1,{x:2});console.info('i');console.warn('w');console.error('e');console.debug('d')</script>",
    );
    let list = messages(&view);
    let by = |method: &str| {
        list.iter()
            .find(|m| m.method == method)
            .unwrap_or_else(|| panic!("no {method}: {list:?}"))
    };
    assert_eq!(
        (
            by("log").level,
            by("info").level,
            by("warn").level,
            by("error").level,
            by("debug").level
        ),
        (
            ConsoleLevel::Log,
            ConsoleLevel::Info,
            ConsoleLevel::Warn,
            ConsoleLevel::Error,
            ConsoleLevel::Debug
        )
    );
    let log = by("log");
    assert_eq!(log.args.len(), 3);
    assert_eq!(
        (log.args[0].kind, log.args[0].text.as_str()),
        (PreviewKind::String, "a")
    );
    assert_eq!(
        (log.args[1].kind, log.args[1].text.as_str()),
        (PreviewKind::Number, "1")
    );
    assert_eq!(log.args[2].kind, PreviewKind::Object);
    assert_eq!(prop(&log.args[2], "x").text, "2");
    assert!(log.text.starts_with("a 1"), "{}", log.text);
    let source = log.source.as_ref().expect("call site");
    assert!(source.line >= 1 && !source.url.is_empty(), "{source:?}");
    assert!(
        list.windows(2)
            .all(|w| w[0].seq < w[1].seq && w[0].timestamp_ms <= w[1].timestamp_ms)
    );
    // Output produced before anyone asked is retained, and a later evaluation adds to it.
    view.evaluate_json("console.log('later')").unwrap();
    assert_eq!(messages(&view).len(), list.len() + 1);
}

#[test]
fn console_behaviour_the_page_can_observe_is_unchanged() {
    let mut view = page("<p>x</p>");
    let probe = view
        .evaluate_json(
            "JSON.stringify([console.log.name,console.warn.name,typeof console.table,typeof console.group,\
             console.log('x')===undefined,(()=>{const f=console.log;f('detached');return 'ok'})(),\
             (()=>{console.log('%s is %d','n',5);return 'ok'})(),typeof console.time,typeof console.count])",
        )
        .unwrap();
    assert_eq!(
        probe,
        "\"[\\\"log\\\",\\\"warn\\\",\\\"function\\\",\\\"function\\\",true,\\\"ok\\\",\\\"ok\\\",\\\"function\\\",\\\"function\\\"]\""
    );
    // Formatter-derived output keeps working and is captured as text.
    view.evaluate_json("console.count('c');console.group('g');console.table([{a:1}]);console.groupEnd();console.assert(false,'nope')").unwrap();
    let texts: Vec<String> = messages(&view).into_iter().map(|m| m.text).collect();
    assert!(texts.iter().any(|t| t.contains("c: 1")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("nope")), "{texts:?}");
}

#[test]
fn previews_cover_every_value_kind_within_budgets() {
    let mut view = page("<p>x</p>");
    let k = |view: &mut View, e: &str| value(view, e).kind;
    assert_eq!(k(&mut view, "undefined"), PreviewKind::Undefined);
    assert_eq!(k(&mut view, "null"), PreviewKind::Null);
    assert_eq!(value(&mut view, "true").text, "true");
    assert_eq!(value(&mut view, "-0").text, "-0");
    assert_eq!(value(&mut view, "NaN").text, "NaN");
    assert_eq!(value(&mut view, "1/0").text, "Infinity");
    assert_eq!(value(&mut view, "10n**20n").text, "100000000000000000000n");
    assert_eq!(value(&mut view, "Symbol('s')").text, "Symbol(s)");
    assert_eq!(
        value(&mut view, "(function named(){})").text,
        "\u{192} named()"
    );
    assert_eq!(k(&mut view, "new Date(0)"), PreviewKind::Date);
    assert_eq!(
        value(&mut view, "new Date(0)").text,
        "1970-01-01T00:00:00.000Z"
    );
    assert_eq!(value(&mut view, "new Date(NaN)").text, "Invalid Date");
    assert_eq!(value(&mut view, "/a+b/gi").text, "/a+b/");
    assert_eq!(value(&mut view, "new Map([[1,2],[3,4]])").text, "Map(2)");
    assert_eq!(value(&mut view, "new Set([1])").text, "Set(1)");
    assert_eq!(value(&mut view, "new Error('bad')").text, "Error: bad");
    assert_eq!(
        value(&mut view, "new TypeError('worse')").text,
        "TypeError: worse"
    );
    assert_eq!(
        value(&mut view, "Promise.resolve(1)").text,
        "Promise {<fulfilled>}"
    );
    assert_eq!(
        value(&mut view, "new Promise(()=>{})").text,
        "Promise {<pending>}"
    );
    assert_eq!(
        value(&mut view, "new Uint8Array(1000000)").text,
        "Uint8Array(1000000)"
    );
    let array = value(&mut view, "[1,'two',[3]]");
    assert_eq!(
        (array.kind, array.text.as_str()),
        (PreviewKind::Array, "Array(3)")
    );
    assert_eq!(prop(&array, "1").text, "two");
    let object = value(&mut view, "({a:1,b:{c:2}})");
    assert_eq!(prop(prop(&object, "b"), "c").text, "2");
    // Budgets: width, depth, text, sparse giants and cycles.
    let wide = value(
        &mut view,
        "Object.fromEntries(Array.from({length:100},(_, i)=>['k'+i,i]))",
    );
    assert_eq!((wide.props.len(), wide.more), (8, 92));
    let giant = value(&mut view, "new Array(1e9)");
    assert_eq!(
        (giant.text.as_str(), giant.props.len()),
        ("Array(1000000000)", 8)
    );
    let deep = value(&mut view, "({a:{b:{c:{d:{e:1}}}}})");
    assert!(deep.is_bounded());
    assert!(
        prop(prop(&deep, "a"), "b").props.is_empty(),
        "depth is cut: {deep:?}"
    );
    let long = value(&mut view, "'x'.repeat(10000000)");
    assert!(
        (long.text.len(), long.truncated) == (256, true),
        "{}",
        long.text.len()
    );
    let cycle = value(
        &mut view,
        "(()=>{const c={name:'c'};c.self=c;c.list=[c];return c})()",
    );
    assert_eq!(prop(&cycle, "self").kind, PreviewKind::Circular);
    assert_eq!(prop(prop(&cycle, "list"), "0").kind, PreviewKind::Circular);
    let many = value(
        &mut view,
        "Array.from({length:8},()=>Object.fromEntries(Array.from({length:8},(_, i)=>['k'+i,{v:i}])))",
    );
    assert!(many.is_bounded(), "node budget: {many:?}");
}

#[test]
fn previews_never_run_page_code_or_read_accessors_or_enter_proxies() {
    let mut view = page("<p>x</p>");
    view.evaluate_json(HOSTILE).unwrap();
    let expected = r#""{\"getter\":0,\"trap\":0,\"toJSON\":0,\"toString\":0,\"valueOf\":0,\"prim\":0,\"tamper\":0}""#;
    for expression in [
        "o",
        "p",
        "pf",
        "k",
        "e",
        "arr",
        "nested",
        "[o,p,pf,k,e,arr]",
        "({inner:o,list:[p,e]})",
    ] {
        value(&mut view, expression);
        assert_eq!(hits(&mut view), expected, "after previewing {expression}");
    }
    // What the previews say about them: type-only for the exotic, labelled accessors elsewhere.
    assert_eq!(value(&mut view, "p").kind, PreviewKind::Proxy);
    assert_eq!(value(&mut view, "pf").kind, PreviewKind::Proxy);
    let o = value(&mut view, "o");
    assert_eq!(prop(&o, "g").text, "(getter)");
    assert_eq!(prop(&o, "plain").text, "1");
    let e = value(&mut view, "e");
    assert_eq!(
        (e.kind, e.text.as_str()),
        (PreviewKind::Error, "Error"),
        "an accessor message is not read"
    );
    let arr = value(&mut view, "arr");
    assert_eq!(prop(&arr, "1").text, "(getter)");
    assert_eq!(prop(&arr, "2").text, "3");
    // Console capture of the same objects also stays clear of them.
    view.evaluate_json("console.log(o,p,pf,k,nested)").unwrap();
    assert_eq!(hits(&mut view), expected, "after console.log");
    let captured = messages(&view).pop().unwrap();
    assert!(captured.args.iter().any(|a| a.kind == PreviewKind::Proxy));
}

#[test]
fn a_page_cannot_tamper_with_the_preview_generator() {
    let mut view = page("<p>x</p>");
    view.evaluate_json(
        "globalThis.hits={tamper:0};const t=()=>{hits.tamper++;return 'pwned'};\
         JSON.stringify=t;Object.getOwnPropertyNames=t;Object.getOwnPropertyDescriptor=t;Object.getPrototypeOf=t;\
         Object.prototype.toJSON=t;Array.prototype.toJSON=t;Array.prototype.push=t;Array.prototype[Symbol.iterator]=t;\
         String.prototype.slice=t;String.prototype.concat=t;Object.hasOwn=t;Reflect.apply=t;Array.isArray=t;\
         Object.defineProperty(Object.prototype,'kind',{get:t,configurable:true});\
         Object.defineProperty(Object.prototype,'get',{get:t,configurable:true});\
         Object.defineProperty(Object.prototype,'value',{get:t,configurable:true});true",
    )
    .unwrap();
    let preview = value(&mut view, "({a:[1,2],s:'x'.repeat(1000)})");
    assert_eq!(preview.kind, PreviewKind::Object);
    assert_eq!(prop(prop(&preview, "a"), "1").text, "2");
    assert!(prop(&preview, "s").truncated);
    view.evaluate_json("console.log({z:[1]},'t')").unwrap();
    let last = messages(&view).pop().unwrap();
    assert_eq!(prop(&last.args[0], "z").kind, PreviewKind::Array);
    assert_eq!(
        view.evaluate_json("hits.tamper").unwrap(),
        "0",
        "page replacements were called"
    );
    // The generator is not reachable from the page at all.
    assert_eq!(
        view.evaluate_json("typeof globalThis.__wega_preview")
            .unwrap(),
        "\"undefined\""
    );
    view.evaluate_json("globalThis.__wega_preview=()=>'forged'")
        .unwrap();
    let again = value(&mut view, "[1]");
    assert_eq!(
        again.kind,
        PreviewKind::Array,
        "a forged global must not replace the generator"
    );
}

#[test]
fn host_operation_handles_are_unreachable_from_page_scripts() {
    // Bootstrap deletes every handle that could reach a host op; a page that tries gets
    // `undefined`, not a working channel. (D2's version of this test called `Deno.core...`
    // and swallowed the resulting ReferenceError, so it asserted nothing.)
    let mut view = page("<p>x</p>");
    let probe = view
        .evaluate_json(
            "JSON.stringify(['Deno','__wega_web','__wega_call','__wega_install_web','__wega_dom',\
             '__wega_semantics','__wega_canvas','__wega_bootstrap_worker','__wega_preview']\
             .map(name => name + ':' + typeof globalThis[name]))",
        )
        .unwrap();
    let list: Vec<String> =
        serde_json::from_str(&serde_json::from_str::<String>(&probe).unwrap()).unwrap();
    for entry in &list {
        assert!(
            entry.ends_with(":undefined"),
            "reachable from page script: {entry}"
        );
    }
    assert_eq!(list.len(), 9);
    // The function it actually tried to call is not there either: this must be a ReferenceError,
    // asserted as such rather than ignored.
    let error = view
        .evaluate_json("Deno.core.ops.op_dom_read('console','{}','','')")
        .expect_err("a page has no route to host operations");
    assert!(error.contains("Deno is not defined"), "{error}");
}

#[test]
fn the_page_callable_error_report_path_cannot_forge_unbounded_or_non_error_entries() {
    // `__wega_report` is page-callable (it backs error reporting) and is the one genuinely
    // reachable way for a page to write an *exception* entry: attack it.
    let mut view = page("<p>x</p>");
    assert_eq!(
        view.evaluate_json("typeof __wega_report").unwrap(),
        "\"function\""
    );
    view.evaluate_json(
        "const attempts=[\
           ()=>new Error('m'.repeat(5e6)),\
           ()=>{const e=new Error('boom');e.stack='    at a (https://other.example/x.js:1:1)\\n'.repeat(200000);return e},\
           ()=>{const e=new Error('x');e.name='Uncaught Fake: injected\\nError';return e},\
           ()=>'plain string thrown',\
           ()=>({toString(){throw new Error('hostile toString')}}),\
           ()=>null, ()=>undefined, ()=>Symbol('s'), ()=>({}), ()=>[1,2,3]];\
         globalThis.outcomes=[];\
         for(const make of attempts){try{__wega_report(make());outcomes.push('reported')}catch(e){outcomes.push('threw')}}\
         for(let i=0;i<5000;i++){try{__wega_report(new Error('flood '+i))}catch(e){}}\
         true",
    )
    .unwrap();
    // The page survives, and everything it managed to write is bounded and honest about its kind.
    assert_eq!(
        view.evaluate_json("typeof document.body").unwrap(),
        "\"object\""
    );
    let entries = messages(&view);
    assert!(!entries.is_empty() && entries.len() <= CONSOLE_RING_ENTRIES);
    assert!(view.console_len() <= CONSOLE_RING_ENTRIES);
    for entry in &entries {
        assert!(entry.is_bounded(), "{} bytes", entry.approx_bytes());
        assert!(entry.approx_bytes() <= MAX_MESSAGE_BYTES);
        assert_eq!(
            entry.level,
            ConsoleLevel::Error,
            "an error report cannot claim another level"
        );
        let exception = entry.exception.as_ref().expect("reports are exceptions");
        assert_eq!(
            exception.kind,
            ExceptionKind::Uncaught,
            "a report cannot claim to be a rejection"
        );
        assert!(exception.name.len() <= 128 && exception.message.len() <= 2048);
        assert!(
            exception
                .stack
                .as_ref()
                .is_none_or(|s| s.len() <= 16 * 1024)
        );
    }
    // The flood was counted, not stored.
    assert_eq!(
        view.console_dropped() as usize + view.console_len(),
        view.console_next_seq() as usize - 1
    );
    assert!(view.console_dropped() > 0);
}

#[test]
fn uncaught_errors_and_rejections_before_devtools_opens_are_retained_with_details() {
    let mut view = page(
        "<script>console.log('start')</script>\
         <script>throw new TypeError('top level failure')</script>\
         <script>setTimeout(()=>{throw new RangeError('late failure')},0);Promise.reject(new Error('rejected'));</script>",
    );
    for _ in 0..40 {
        view.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    let list = messages(&view);
    let exception = |text: &str| {
        list.iter()
            .filter_map(|m| m.exception.as_ref())
            .find(|e| e.message.contains(text))
            .unwrap_or_else(|| panic!("no exception containing {text}: {list:#?}"))
    };
    let top = exception("top level failure");
    assert_eq!(top.kind, ExceptionKind::Uncaught);
    assert_eq!(top.name, "TypeError");
    let late = exception("late failure");
    assert_eq!(
        (late.kind, late.name.as_str()),
        (ExceptionKind::Uncaught, "RangeError")
    );
    let rejected = exception("rejected");
    assert_eq!(rejected.kind, ExceptionKind::UnhandledRejection);
    for e in [top, late, rejected] {
        let source = e.source.as_ref().expect("a source location");
        assert!(!source.url.is_empty());
    }
    assert!(late.source.as_ref().unwrap().line >= 1, "{late:?}");
    assert!(late.stack.as_deref().unwrap_or("").contains("late failure") || late.stack.is_some());
    // Ordinary output and exceptions interleave in order in the one ring.
    let order: Vec<_> = list.iter().map(|m| m.seq).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(list[0].text, "start");
    assert!(
        list.iter()
            .all(|m| m.level == ConsoleLevel::Error || m.exception.is_none())
    );
}

#[test]
fn a_ten_thousand_message_flood_stays_within_every_budget() {
    let mut view = page("<p>x</p>");
    let started = Instant::now();
    // Ten bounded script runs, not one: a single 10,000-call run can exceed the 5 s script
    // watchdog when the host is CPU-starved (found by the D3 final investigation, where this test
    // failed with "Script deadline exceeded" under compiler load). Totals and budgets are the same.
    for batch in 0..10 {
        view.evaluate_json(&format!(
            "for(let i={};i<{};i++)console.log('flood',i,{{i,s:'z'.repeat(i%500)}},[i,i+1])",
            batch * 1000,
            (batch + 1) * 1000
        ))
        .unwrap();
    }
    let elapsed = started.elapsed();
    assert!(view.console_len() <= CONSOLE_RING_ENTRIES);
    let all = messages(&view);
    assert!(all.iter().map(|m| m.approx_bytes()).sum::<usize>() <= CONSOLE_RING_BYTES);
    assert_eq!(view.console_dropped() as usize + view.console_len(), 10_000);
    assert_eq!(view.console_next_seq(), 10_001);
    assert_eq!(all.last().unwrap().args[1].text, "9999");
    assert!(elapsed < Duration::from_secs(30), "{elapsed:?}");
    // Resident memory is measured by the isolated benchmark: other tests share this process.
}

#[test]
fn oversized_hostile_values_cannot_make_a_message_exceed_its_budget() {
    let mut view = page("<p>x</p>");
    view.evaluate_json(
        "globalThis.big='y'.repeat(2e6);globalThis.wide=Object.fromEntries(Array.from({length:300},(_, i)=>['k'+i,big.slice(0,1000)]));\
         globalThis.cyc={};cyc.me=cyc;cyc.list=Array.from({length:1000},()=>cyc);true",
    )
    .unwrap();
    // One logging call per script run (see the flood test: a single long run can hit the
    // watchdog on a starved host).
    for _ in 0..8 {
        view.evaluate_json(
            "console.log(big,wide,cyc,new Array(1e8),new Uint8Array(1e6),big,wide,cyc,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20)",
        )
        .unwrap();
    }
    let all = messages(&view);
    assert_eq!(all.len(), 8);
    for m in &all {
        assert!(
            m.approx_bytes() <= MAX_MESSAGE_BYTES,
            "{}",
            m.approx_bytes()
        );
        assert!(m.is_bounded());
        assert!(m.args.len() <= 16);
    }
    assert!(view.console_len() <= CONSOLE_RING_ENTRIES);
}

#[test]
fn a_navigation_starts_a_fresh_history_and_the_old_one_is_gone() {
    let mut view = page("<script>console.log('first document')</script>");
    assert_eq!(messages(&view).len(), 1);
    view.set_html("<script>console.log('second document')</script>")
        .unwrap();
    for _ in 0..4 {
        view.poll();
    }
    let list = messages(&view);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].text, "second document");
    assert_eq!(list[0].seq, 1, "sequence restarts with the document");
    assert_eq!(view.console_dropped(), 0);
}

#[test]
fn evaluation_returns_values_and_exceptions_without_leaking_page_exceptions_into_the_log() {
    let mut view = page("<p>x</p>");
    assert_eq!(value(&mut view, "1+1").text, "2");
    assert_eq!(value(&mut view, "var declared=5; declared*2").text, "10");
    assert_eq!(
        value(&mut view, "document.title='t'; document.title").text,
        "t"
    );
    let EvalOutcome::Exception { details } =
        PagePort::evaluate(&mut view, "(function inner(){ null.x })()").unwrap()
    else {
        panic!()
    };
    assert_eq!(details.kind, ExceptionKind::Evaluation);
    assert_eq!(details.name, "TypeError");
    assert!(details.message.contains("null"), "{}", details.message);
    assert!(
        details.stack.as_deref().unwrap_or("").contains("inner"),
        "{details:?}"
    );
    let source = details.source.expect("location of the throw");
    assert!(source.line >= 1);
    let EvalOutcome::Exception { details } =
        PagePort::evaluate(&mut view, "throw 'plain string'").unwrap()
    else {
        panic!()
    };
    assert!(details.message.contains("plain string"), "{details:?}");
    let EvalOutcome::Exception { details } =
        PagePort::evaluate(&mut view, "syntax error here(").unwrap()
    else {
        panic!()
    };
    assert_eq!(details.name, "SyntaxError");
    // The page continues to work, and a user's evaluation exception is not an "uncaught error".
    assert_eq!(value(&mut view, "typeof document.body").text, "object");
    assert!(
        messages(&view).iter().all(|m| m.exception.is_none()),
        "{:?}",
        messages(&view)
    );
    // A returned promise reports state only: it is neither awaited nor inspected.
    assert_eq!(
        value(&mut view, "Promise.resolve(42)").text,
        "Promise {<fulfilled>}"
    );
    // A hostile result never runs its code during previewing.
    view.evaluate_json("globalThis.hits={a:0}").unwrap();
    value(
        &mut view,
        "({get x(){hits.a++},toJSON(){hits.a++},toString(){hits.a++}})",
    );
    assert_eq!(view.evaluate_json("hits.a").unwrap(), "0");
}

#[test]
fn the_script_watchdog_still_bounds_evaluation_and_reports_the_termination_it_confirmed() {
    let limits = Limits {
        script_time: Duration::from_millis(300),
        ..Default::default()
    };
    let mut view = page_with("<p>x</p>", limits);
    let started = Instant::now();
    let error =
        PagePort::evaluate(&mut view, "while(true){}").expect_err("an endless loop cannot return");
    assert!(error.contains("terminated"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(10));
    let failure = view
        .take_script_failure()
        .expect("the engine recorded the fatal stop");
    assert!(failure.contains("deadline"), "{failure}");
    // A fatal stop is also kept in the console history for the next DevTools session.
    assert!(messages(&view).iter().any(|m| m.exception.is_some()));
}

#[test]
fn the_engines_own_document_origin_follows_the_brokers_http_https_rule() {
    // `file:` is *not* a tuple origin for the broker's principal; the engine's view must agree,
    // or a file page would be treated as evaluable here and opaque in the browser.
    let mut view = page("<p>x</p>");
    assert_eq!(value(&mut view, "1").text, "1");
    let origin = |view: &mut View, url: &str| {
        view.url = Url::parse(url).unwrap();
        view.document_origin()
    };
    assert_eq!(
        origin(&mut view, "https://fixture.test/a?b#c").as_deref(),
        Some("https://fixture.test")
    );
    assert_eq!(
        origin(&mut view, "http://127.0.0.1:8080/x").as_deref(),
        Some("http://127.0.0.1:8080")
    );
    assert_eq!(
        origin(&mut view, "https://fixture.test:8443/").as_deref(),
        Some("https://fixture.test:8443")
    );
    for opaque in [
        "about:blank",
        "data:text/html,x",
        "file:///tmp/a.html",
        "blob:https://fixture.test/uuid",
        "javascript:1",
        "wega://settings",
        "ftp://example.test/",
    ] {
        assert_eq!(origin(&mut view, opaque), None, "{opaque}");
    }
}
