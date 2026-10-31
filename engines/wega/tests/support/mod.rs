//! A small multi-route HTTP/1.1 server for engine integration tests.
#![allow(dead_code)]
pub mod harness;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

/// One received request.
#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}
/// A response: status line, extra header lines, body.
pub struct Reply {
    pub status: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub delay: Duration,
}
impl Reply {
    pub fn ok(content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: "200 OK",
            headers: vec![("Content-Type".into(), content_type.into())],
            body: body.into(),
            delay: Duration::ZERO,
        }
    }
    pub fn html(body: &str) -> Self {
        Self::ok("text/html; charset=utf-8", body)
    }
    pub fn status(mut self, status: &'static str) -> Self {
        self.status = status;
        self
    }
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}
type Route = dyn Fn(&Seen) -> Reply + Send + Sync;
pub struct Server {
    pub port: u16,
    pub seen: Arc<Mutex<Vec<Seen>>>,
    done: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    pub fn new(route: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Self {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let port = socket.local_addr().unwrap().port();
        let done = Arc::new(AtomicBool::new(false));
        let seen: Arc<Mutex<Vec<Seen>>> = Default::default();
        let route: Arc<Route> = Arc::new(route);
        let (stop, log) = (done.clone(), seen.clone());
        let thread = thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match socket.accept() {
                    Ok((stream, _)) => {
                        let (route, log) = (route.clone(), log.clone());
                        thread::spawn(move || serve(stream, &*route, &log));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        });
        Self {
            port,
            seen,
            done,
            thread: Some(thread),
        }
    }
    /// The server as `http://<host>:<port>/<path>`; `host` picks the origin/site.
    pub fn url(&self, host: &str, path: &str) -> String {
        format!(
            "http://{host}:{}/{}",
            self.port,
            path.trim_start_matches('/')
        )
    }
    pub fn requests(&self, path: &str) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.path == path)
            .cloned()
            .collect()
    }
}
fn serve(mut stream: std::net::TcpStream, route: &Route, log: &Mutex<Vec<Seen>>) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 16384];
    let header_end = loop {
        let n = stream.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            return;
        }
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
    };
    let head = String::from_utf8_lossy(&bytes[..header_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default().to_owned();
    let path = first.next().unwrap_or("/").to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| (n.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let length = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = bytes[header_end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    let seen = Seen {
        method,
        path,
        headers,
        body,
    };
    log.lock().unwrap().push(seen.clone());
    let reply = route(&seen);
    thread::sleep(reply.delay);
    let mut response = format!("HTTP/1.1 {}\r\n", reply.status);
    for (name, value) in &reply.headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        reply.body.len()
    ));
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(&reply.body);
}
impl Drop for Server {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn engine() -> Engine {
    let limits = Limits {
        script_time: Duration::from_secs(3),
        ..Limits::default()
    };
    if std::env::var_os("WEGA_TEST_BROKER").is_some() {
        let directory = tempfile::tempdir().unwrap();
        let broker = wega_network::Broker::new().unwrap();
        let session = broker
            .open_session(directory.path().to_owned(), NetworkPolicy::default())
            .unwrap();
        let (address, token) = session.endpoint();
        // Hold the capability and private profile for exactly this engine's lifetime.
        let wake = Arc::new(move || {
            let _ = (&directory, &broker, &session);
        });
        Engine::brokered(limits, NetworkPolicy::default(), wake, &address, token).unwrap()
    } else {
        Engine::new(limits, NetworkPolicy::default(), Arc::new(|| {})).unwrap()
    }
}
/// Load a URL and poll until the page's load event fired.
pub fn open(engine: &Engine, url: &str) -> View {
    let mut view = engine.create_view(Url::parse(url).unwrap()).unwrap();
    settle(&mut view);
    view
}
pub fn settle(view: &mut View) {
    let deadline = Instant::now() + Duration::from_secs(10);
    view.poll();
    while view.loading && Instant::now() < deadline {
        view.poll();
        thread::sleep(Duration::from_millis(2));
    }
    view.poll();
    assert!(
        !view.loading,
        "page did not finish loading: {:?}",
        view.notices
    );
}
/// Poll until `script` evaluates to something other than null.
pub fn wait(view: &mut View, script: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        view.poll();
        let value = view.evaluate_json(script).unwrap();
        if value != "null" {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {script}: {:?}",
            view.notices
        );
        thread::sleep(Duration::from_millis(2));
    }
}
