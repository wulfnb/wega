use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};
struct Server {
    url: String,
    requests: Arc<AtomicUsize>,
    done: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let port = socket.local_addr().unwrap().port();
        let done = Arc::new(AtomicBool::new(false));
        let stop = done.clone();
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let thread = thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match socket.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut bytes = [0u8; 16384];
                        let mut size = 0;
                        while size < bytes.len() {
                            let n = match stream.read(&mut bytes[size..]) {
                                Ok(n) => n,
                                Err(error) => {
                                    eprintln!("fixture read: {error:?}");
                                    0
                                }
                            };
                            if n == 0 {
                                break;
                            }
                            size += n;
                            if bytes[..size].windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }

                        if size == 0 {
                            continue;
                        }
                        let text = String::from_utf8_lossy(&bytes[..size]);
                        let path = text.split_whitespace().nth(1).unwrap_or("/");
                        count.fetch_add(1, Ordering::Relaxed);
                        let (status, headers, body) = if path == "/redirect" {
                            (
                                "302 Found",
                                format!("Location: http://localhost:{port}/data\r\n"),
                                String::new(),
                            )
                        } else if path == "/dep.js" {
                            ("200 OK", String::new(), "export const value=42;".into())
                        } else if path == "/dynamic.js" {
                            ("200 OK", String::new(), "export default 7;".into())
                        } else if path == "/cookie-set" {
                            ("200 OK","Set-Cookie: secret=server; Path=/; HttpOnly; Max-Age=3600\r\nSet-Cookie: visible=yes; Path=/; Max-Age=3600\r\n".into(),"set".into())
                        } else if path == "/cookie-redirect" {
                            (
                                "302 Found",
                                "Location: /cookie-echo\r\nSet-Cookie: hop=redirect; Path=/\r\n"
                                    .into(),
                                String::new(),
                            )
                        } else if path == "/cookie-echo" {
                            let cookie = text
                                .lines()
                                .find_map(|line| {
                                    line.split_once(':')
                                        .filter(|(name, _)| name.eq_ignore_ascii_case("cookie"))
                                        .map(|(_, value)| value.trim().to_owned())
                                })
                                .unwrap_or_default();
                            ("200 OK", String::new(), cookie)
                        } else if path == "/large" {
                            ("200 OK", String::new(), "x".repeat(2048))
                        } else {
                            ("200 OK", String::new(), "{\"answer\":42}".into())
                        };
                        let _ = write!(
                            stream,
                            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        });
        Self {
            url: format!("http://127.0.0.1:{port}/"),
            requests,
            done,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn page(engine: &Engine, url: &str) -> View {
    let mut view = engine
        .create_view(Url::parse("about:blank").unwrap())
        .unwrap();
    view.url = Url::parse(url).unwrap();
    view.set_html("<title>waiting</title>").unwrap();
    view.poll();
    view
}
fn wait(view: &mut View, script: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        view.poll();
        let value = view.evaluate_json(script).unwrap();
        if value != "null" {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "async timeout: {:?}",
            view.notices
        );
        thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn fetch_and_xhr_use_the_local_server() {
    let server = Server::new();
    let engine = Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
    let mut view = page(&engine, &server.url);
    view.evaluate("fetch('/data').then(r=>r.json()).then(v=>globalThis.fetched=v.answer).catch(e=>globalThis.fetched=String(e)+' cause: '+String(e.cause)+' stack: '+e.stack)").unwrap();
    assert_eq!(wait(&mut view, "globalThis.fetched??null"), "42");
    view.evaluate("const xhr=new XMLHttpRequest();xhr.open('GET','/data');xhr.responseType='json';xhr.onload=()=>globalThis.xhrResult=[xhr.status,xhr.response.answer,xhr.getResponseHeader('content-type')];xhr.onerror=()=>globalThis.xhrResult='error';xhr.send()").unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.xhrResult??null"),
        "[200,42,\"application/json\"]"
    );
    assert_eq!(server.requests.load(Ordering::Relaxed), 2);
}
#[test]
fn live_policy_blocks_fetch_websocket_redirects_and_files() {
    let server = Server::new();
    let engine = Engine::new(
        Limits::default(),
        NetworkPolicy {
            allowed_hosts: "127.0.0.1".into(),
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap();
    let mut view = page(&engine, &server.url);
    view.evaluate("fetch('/redirect').then(()=>globalThis.redirectResult='allowed',()=>globalThis.redirectResult='blocked')").unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.redirectResult??null"),
        "\"blocked\""
    );
    assert_eq!(
        server.requests.load(Ordering::Relaxed),
        1,
        "redirect must not reach blocked localhost"
    );
    assert_eq!(view.evaluate_json("(()=>{try{new WebSocket('ws://blocked.test/');return 'allowed'}catch{return 'blocked'}})()").unwrap(),"\"blocked\"");
    engine.set_policy(NetworkPolicy {
        offline: true,
        ..Default::default()
    });
    view.evaluate(
        "fetch('/data').then(()=>globalThis.offline='allowed',()=>globalThis.offline='blocked')",
    )
    .unwrap();
    assert_eq!(wait(&mut view, "globalThis.offline??null"), "\"blocked\"");
    assert_eq!(view.evaluate_json("(()=>{try{new WebSocket('ws://127.0.0.1/');return 'allowed'}catch{return 'blocked'}})()").unwrap(),"\"blocked\"");
    view.evaluate("fetch('file:///etc/passwd').then(()=>globalThis.file='allowed',()=>globalThis.file='blocked')").unwrap();
    assert_eq!(wait(&mut view, "globalThis.file??null"), "\"blocked\"");
    assert_eq!(server.requests.load(Ordering::Relaxed), 1);
}
#[test]
fn response_size_limit_is_retained() {
    let server = Server::new();
    let engine = Engine::new(
        Limits {
            resource_bytes: 1024,
            ..Limits::default()
        },
        NetworkPolicy::default(),
        Arc::new(|| {}),
    )
    .unwrap();
    let mut view = page(&engine, &server.url);
    view.evaluate("fetch('/large').then(r=>r.text()).then(()=>globalThis.large='allowed',()=>globalThis.large='blocked')").unwrap();
    assert_eq!(wait(&mut view, "globalThis.large??null"), "\"blocked\"");
}
// Opening the on-disk storage and cookie files runs inside the script budget.
// On a busy CI runner a debug build can spend more than the default 100 ms there.
fn persistent_limits() -> Limits {
    Limits {
        script_time: Duration::from_secs(2),
        ..Limits::default()
    }
}
#[test]
fn storage_is_persistent_origin_partitioned_and_has_a_quota() {
    let directory = std::env::temp_dir().join(format!("wega-web-storage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let engine = Engine::new(
        persistent_limits(),
        NetworkPolicy::default(),
        Arc::new(|| {}),
    )
    .unwrap()
    .with_storage_dir(directory.clone());
    {
        let mut first = page(&engine, "https://one.test/");
        first.evaluate("localStorage.setItem('key','one')").unwrap();
    }
    let mut first = page(&engine, "https://one.test/next");
    assert_eq!(
        first.evaluate_json("localStorage.getItem('key')").unwrap(),
        "\"one\""
    );
    let mut other = page(&engine, "https://two.test/");
    assert_eq!(
        other.evaluate_json("localStorage.getItem('key')").unwrap(),
        "null"
    );
    assert!(
        other
            .evaluate("localStorage.setItem('large','x'.repeat(11*1024*1024))")
            .is_err()
    );
    drop(other);
    drop(first);
    drop(engine);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn deno_web_primitives_are_available_and_core_ops_are_private() {
    let engine = Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
    let mut view = page(&engine, "https://fixture.test/");
    assert_eq!(view.evaluate_json("[new URL('/x',location.href).pathname,new TextDecoder().decode(new TextEncoder().encode('hello')),new Blob(['test']).size,structuredClone({x:2}).x,typeof crypto.subtle,typeof ReadableStream,typeof Deno]").unwrap(),"[\"/x\",\"hello\",4,2,\"object\",\"function\",\"undefined\"]");
}

#[test]
fn real_jquery_ajax_uses_the_bounded_xhr_path() {
    let server = Server::new();
    let engine = Engine::new(
        Limits {
            script_time: Duration::from_secs(2),
            ..Limits::default()
        },
        NetworkPolicy::default(),
        Arc::new(|| {}),
    )
    .unwrap();
    let mut view = page(&engine, &server.url);
    view.evaluate(include_str!("fixtures/jquery.js")).unwrap();
    view.evaluate("jQuery.ajax({url:'/data',dataType:'json'}).done(data=>globalThis.ajaxResult=data.answer).fail((xhr,status,error)=>globalThis.ajaxResult=String(error))").unwrap();
    assert_eq!(wait(&mut view, "globalThis.ajaxResult??null"), "42");
    assert_eq!(server.requests.load(Ordering::Relaxed), 1);
}

#[test]
fn modules_load_graphs_dynamic_import_and_top_level_await() {
    let server = Server::new();
    let engine = Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
    let mut view = page(&engine, &server.url);
    view.set_html("<p id='result'>waiting</p><script type='module'>import {value} from './dep.js';const lazy=await import('./dynamic.js');await new Promise(resolve=>setTimeout(resolve,5));document.getElementById('result').textContent=String(value+lazy.default);</script>").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while view.text("#result").as_deref() != Some("49") && Instant::now() < deadline {
        view.poll();
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        view.text("#result").as_deref(),
        Some("49"),
        "{:?}",
        view.notices
    );
    view.poll();
    assert!(!view.loading);
    engine.set_policy(NetworkPolicy {
        offline: true,
        ..Default::default()
    });
    view.evaluate(&format!("import('{}blocked.js').then(()=>globalThis.modulePolicy='allowed',()=>globalThis.modulePolicy='blocked')",server.url)).unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.modulePolicy??null"),
        "\"blocked\""
    );
    assert_eq!(server.requests.load(Ordering::Relaxed), 2);
}
#[test]
fn cookies_share_http_fetch_and_script_rules_and_persist() {
    let server = Server::new();
    let directory = std::env::temp_dir().join(format!("wega-cookies-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let engine = Engine::new(
        persistent_limits(),
        NetworkPolicy::default(),
        Arc::new(|| {}),
    )
    .unwrap()
    .with_storage_dir(directory.clone());
    let mut view = engine
        .create_view(Url::parse(&format!("{}cookie-set", server.url)).unwrap())
        .unwrap();
    let load_deadline = Instant::now() + Duration::from_secs(5);
    while view.loading {
        view.poll();
        assert!(Instant::now() < load_deadline, "{:?}", view.notices);
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        view.evaluate_json("document.cookie.includes('visible=yes')")
            .unwrap(),
        "true"
    );
    view.evaluate(
        "fetch('/cookie-set').then(r=>globalThis.cookieHeaders=r.headers.get('set-cookie'))",
    )
    .unwrap();
    // null is the expected private-header result, so use a separate completion marker.
    view.evaluate("fetch('/cookie-echo').then(r=>r.text()).then(v=>globalThis.cookieEcho=v)")
        .unwrap();
    let echo = wait(&mut view, "globalThis.cookieEcho??null");
    // Requests may overlap; wait until the Set-Cookie response has been processed.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !view
        .evaluate_json("document.cookie.includes('visible=yes')")
        .is_ok_and(|s| s == "true")
    {
        view.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    let _ = echo;
    assert_eq!(
        view.evaluate_json("globalThis.cookieHeaders").unwrap(),
        "null"
    );
    assert_eq!(
        view.evaluate_json("document.cookie.includes('secret=')")
            .unwrap(),
        "false"
    );
    view.evaluate("document.cookie='secret=stolen; Path=/';document.cookie='forged=x; HttpOnly';document.cookie='wrong=x; Domain=other.test';document.cookie='local=ok; Path=/; Max-Age=3600';document.cookie='scoped=path; Path=/private'").unwrap();
    view.evaluate("fetch('/cookie-echo').then(r=>r.text()).then(v=>globalThis.checkedCookies=v)")
        .unwrap();
    let echo = wait(&mut view, "globalThis.checkedCookies??null");
    assert!(
        echo.contains("secret=server")
            && echo.contains("local=ok")
            && !echo.contains("stolen")
            && !echo.contains("scoped")
            && !echo.contains("forged")
            && !echo.contains("wrong"),
        "{echo}"
    );
    view.evaluate("fetch('/cookie-echo',{credentials:'omit'}).then(r=>r.text()).then(v=>globalThis.omitted=v)").unwrap();
    assert_eq!(wait(&mut view, "globalThis.omitted??null"), "\"\"");
    view.evaluate(
        "fetch('/cookie-redirect').then(r=>r.text()).then(v=>globalThis.redirectCookies=v)",
    )
    .unwrap();
    assert!(wait(&mut view, "globalThis.redirectCookies??null").contains("hop=redirect"));
    drop(view);
    drop(engine);
    let engine = Engine::new(
        persistent_limits(),
        NetworkPolicy::default(),
        Arc::new(|| {}),
    )
    .unwrap()
    .with_storage_dir(directory.clone());
    let mut view = page(&engine, &server.url);
    assert_eq!(
        view.evaluate_json("document.cookie.includes('local=ok')")
            .unwrap(),
        "true"
    );
    view.evaluate("fetch('/cookie-echo').then(r=>r.text()).then(v=>globalThis.restored=v)")
        .unwrap();
    assert!(wait(&mut view, "globalThis.restored??null").contains("secret=server"));
    drop(view);
    drop(engine);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn broker_fetch_retains_response_filtering_immutability_and_redirect_metadata() {
    let server = Server::new();
    let engine = Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
    let mut view = page(&engine, &server.url);
    view.evaluate("fetch('/redirect',{redirect:'manual'}).then(r=>globalThis.manual=[r.type,r.status,[...r.headers].length,r.body===null])").unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.manual??null"),
        "[\"opaqueredirect\",0,0,true]"
    );
    view.evaluate("fetch('/redirect').then(r=>{let immutable=false;try{r.headers.set('test','value')}catch{immutable=true}globalThis.follow=[r.type,r.status,r.redirected,immutable]})").unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.follow??null"),
        "[\"basic\",200,true,true]"
    );
}

#[test]
fn fetch_preserves_abort_reason_and_status_text() {
    let server = Server::new();
    let engine = Engine::new(Limits::default(), NetworkPolicy::default(), Arc::new(|| {})).unwrap();
    let mut view = page(&engine, &server.url);
    view.evaluate("const stopped=new AbortController();stopped.abort('custom reason');fetch('/data',{signal:stopped.signal}).catch(reason=>globalThis.aborted=reason)").unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.aborted??null"),
        "\"custom reason\""
    );
    view.evaluate("fetch('/data').then(r=>globalThis.statusTextValue=r.statusText)")
        .unwrap();
    assert_eq!(
        wait(&mut view, "globalThis.statusTextValue??null"),
        "\"OK\""
    );
}
