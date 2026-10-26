//! Bounded HTTP/cache/cookie broker. Capabilities travel only over private worker IPC.
//! The loopback socket speaks a binary-framed protocol, never HTTP or WebSocket.
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, RwLock, Weak, mpsc},
    time::{Duration, Instant},
};
use url::Url;
mod cookies;
pub use cookies::{Cookies, SiteContext};
const MAX_HEADER: usize = 64 * 1024;
pub const MAX_BODY: usize = 8 * 1024 * 1024;
const CACHE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Default, Debug)]
pub struct Policy {
    pub offline: bool,
    pub blocked_hosts: String,
    pub allowed_hosts: String,
}
impl Policy {
    pub fn permits(&self, url: &Url) -> bool {
        if self.offline || !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        let host = url
            .host_str()
            .unwrap_or_default()
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let matches = |rules: &str| {
            rules.split_whitespace().any(|rule| {
                let rule = rule
                    .trim_start_matches("*.")
                    .trim_end_matches('.')
                    .to_ascii_lowercase();
                !rule.is_empty() && (host == rule || host.ends_with(&format!(".{rule}")))
            })
        };
        !matches(&self.blocked_hosts)
            && (self.allowed_hosts.trim().is_empty() || matches(&self.allowed_hosts))
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Request {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    #[serde(skip)]
    pub body: Vec<u8>,
    pub origin: String,
    #[serde(default)]
    pub initiator: Option<Url>,
    #[serde(default)]
    pub top_level: bool,
    pub credentials: String,
    pub redirect: String,
    pub cache: String,
    pub limit: usize,
}
impl Request {
    pub fn get(url: &Url, limit: usize) -> Self {
        Self {
            url: url.to_string(),
            method: "GET".into(),
            headers: vec![],
            body: vec![],
            origin: url.origin().ascii_serialization(),
            initiator: None,
            top_level: true,
            credentials: "include".into(),
            redirect: "follow".into(),
            cache: "default".into(),
            limit,
        }
    }
    fn validate(&self) -> Result<(), String> {
        if self.limit == 0
            || self.limit > MAX_BODY
            || self.body.len() > MAX_BODY
            || self.headers.len() > 128
            || self
                .headers
                .iter()
                .map(|(name, value)| name.len() + value.len())
                .sum::<usize>()
                > MAX_HEADER / 2
            || self.origin.len() > 16 * 1024
            || self.url.len() > 16 * 1024
            || !matches!(
                self.method.as_str(),
                "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS"
            )
        {
            return Err("Invalid or oversized network request".into());
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Response {
    pub url: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub redirected: bool,
    #[serde(skip)]
    pub body: Vec<u8>,
}
impl Response {
    pub fn status_text(&self) -> &'static str {
        reqwest::StatusCode::from_u16(self.status)
            .ok()
            .and_then(|status| status.canonical_reason())
            .unwrap_or("")
    }
}
fn cache_cost(key: &str, response: &Response) -> usize {
    key.len()
        + response.url.len()
        + response.body.len()
        + response
            .headers
            .iter()
            .map(|(name, value)| name.len() + value.len())
            .sum::<usize>()
        + std::mem::size_of::<Entry>()
}
struct Entry {
    key: String,
    response: Response,
    until: Instant,
}
#[derive(Default)]
struct Cache {
    entries: VecDeque<Entry>,
    bytes: usize,
}
impl Cache {
    fn get(&mut self, key: &str, limit: usize) -> Result<Option<Response>, String> {
        let now = Instant::now();
        self.entries.retain(|entry| entry.until > now);
        self.bytes = self
            .entries
            .iter()
            .map(|entry| cache_cost(&entry.key, &entry.response))
            .sum();
        let Some(index) = self.entries.iter().position(|entry| entry.key == key) else {
            return Ok(None);
        };
        let entry = self.entries.remove(index).unwrap();
        let result = if entry.response.body.len() > limit {
            Err("Response exceeds its size limit".into())
        } else {
            Ok(Some(entry.response.clone()))
        };
        self.entries.push_back(entry);
        result
    }
    fn insert(&mut self, key: String, response: &Response) {
        if response.status != 200
            || response.body.len() > 2 * 1024 * 1024
            || response.headers.iter().any(|(name, _)| {
                name.eq_ignore_ascii_case("set-cookie") || name.eq_ignore_ascii_case("vary")
            })
        {
            return;
        }
        let control = response
            .headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("cache-control"))
            .map(|(_, value)| value.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(",");
        let tokens: Vec<_> = control.split(',').map(str::trim).collect();
        if !tokens.contains(&"public")
            || tokens
                .iter()
                .any(|value| matches!(*value, "no-cache" | "no-store" | "private"))
        {
            return;
        }
        let ttl = tokens
            .iter()
            .find_map(|token| token.strip_prefix("max-age=")?.parse::<u64>().ok())
            .unwrap_or(0)
            .min(300);
        if ttl == 0 {
            return;
        }
        let cost = cache_cost(&key, response);
        if cost > CACHE_BYTES {
            return;
        }
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key) {
            let old = self.entries.remove(index).unwrap();
            self.bytes -= cache_cost(&old.key, &old.response);
        }
        while self.bytes + cost > CACHE_BYTES || self.entries.len() >= 128 {
            let Some(old) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= cache_cost(&old.key, &old.response);
        }
        self.bytes += cost;
        self.entries.push_back(Entry {
            key,
            response: response.clone(),
            until: Instant::now() + Duration::from_secs(ttl),
        });
    }
}
#[derive(Clone)]
pub struct LocalClient(reqwest::blocking::Client);
impl LocalClient {
    pub fn new() -> Result<Self, String> {
        reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .pool_max_idle_per_host(2)
            .user_agent("Wega/0.1 (Blitz/V8)")
            .build()
            .map(Self)
            .map_err(|e| e.to_string())
    }
    pub fn fetch(
        &self,
        request: &Request,
        policy: &RwLock<Policy>,
        cookies: &Cookies,
    ) -> Result<Response, String> {
        self.fetch_cached(request, policy, cookies, None)
    }
    fn fetch_cached(
        &self,
        request: &Request,
        policy: &RwLock<Policy>,
        cookies: &Cookies,
        cache: Option<(&Mutex<Cache>, &str)>,
    ) -> Result<Response, String> {
        request.validate()?;
        let mut url = Url::parse(&request.url).map_err(|e| e.to_string())?;
        let mut method = request.method.clone();
        let mut body = request.body.clone();
        let mut headers = request.headers.clone();
        for hop in 0..=8 {
            if !policy
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .permits(&url)
            {
                return Err("Network request blocked by browsing policy".into());
            }
            let credentials = request.credentials == "include"
                || (request.credentials != "omit"
                    && url.origin().ascii_serialization() == request.origin);
            let cookie = if credentials {
                cookies.get(
                    &url,
                    false,
                    Cookies::context(
                        &url,
                        request.initiator.as_ref(),
                        request.top_level && matches!(method.as_str(), "GET" | "HEAD"),
                    ),
                )
            } else {
                String::new()
            };
            let cacheable = method == "GET"
                && cookie.is_empty()
                && !headers
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                && request.cache != "no-store";
            let key = format!(
                "{}\n{}\n{:?}",
                cache.map(|(_, scope)| scope).unwrap_or_default(),
                url,
                headers
            );
            if cacheable
                && !matches!(request.cache.as_str(), "no-cache" | "reload")
                && let Some((cache, _)) = cache
                && let Some(response) = cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&key, request.limit)?
            {
                let mut response = response;
                response.redirected = hop > 0;
                return Ok(response);
            }
            if request.cache == "only-if-cached" {
                return Err("only-if-cached cache miss".into());
            }
            let mut builder = self.0.request(
                method
                    .parse::<reqwest::Method>()
                    .map_err(|e| e.to_string())?,
                url.clone(),
            );
            for (name, value) in &headers {
                if !["cookie", "host", "connection", "content-length"]
                    .iter()
                    .any(|blocked| name.eq_ignore_ascii_case(blocked))
                {
                    builder = builder.header(name, value);
                }
            }
            if !cookie.is_empty() {
                builder = builder.header("cookie", cookie);
            }
            if !body.is_empty() {
                builder = builder.body(body.clone());
            }
            let response = builder.send().map_err(|e| e.to_string())?;
            let status = response.status().as_u16();
            let response_headers: Vec<_> = response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    Some((name.to_string(), value.to_str().ok()?.to_owned()))
                })
                .collect();
            if credentials {
                for value in response.headers().get_all(reqwest::header::SET_COOKIE) {
                    if let Ok(value) = value.to_str() {
                        cookies.set(
                            &url,
                            value,
                            false,
                            Cookies::context(
                                &url,
                                request.initiator.as_ref(),
                                request.top_level && matches!(method.as_str(), "GET" | "HEAD"),
                            ),
                        );
                    }
                }
            }
            if matches!(status, 301 | 302 | 303 | 307 | 308)
                && let Some(location) = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
            {
                if request.redirect == "error" {
                    return Err("Redirect forbidden by request".into());
                }
                if request.redirect != "manual" {
                    if hop == 8 {
                        return Err("Too many redirects".into());
                    }
                    let next = url.join(location).map_err(|e| e.to_string())?;
                    if next.origin() != url.origin() {
                        headers.retain(|(name, _)| !name.eq_ignore_ascii_case("authorization"));
                    }
                    if status == 303 || ((status == 301 || status == 302) && method == "POST") {
                        method = "GET".into();
                        body.clear();
                        headers.retain(|(name, _)| !name.eq_ignore_ascii_case("content-type"));
                    }
                    url = next;
                    continue;
                }
            }
            if response
                .content_length()
                .is_some_and(|bytes| bytes > request.limit as u64)
            {
                return Err("Response exceeds its size limit".into());
            }
            let mut bytes = Vec::new();
            response
                .take(request.limit as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > request.limit {
                return Err("Response exceeds its size limit".into());
            }
            let response = Response {
                url: url.to_string(),
                status,
                headers: response_headers,
                redirected: hop > 0,
                body: bytes,
            };
            if cacheable && let Some((cache, _)) = cache {
                cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(key, &response);
            }
            return Ok(response);
        }
        Err("Too many redirects".into())
    }
}

struct Session {
    policy: RwLock<Policy>,
    cookies: Arc<Cookies>,
    scope: String,
}
pub struct SessionHandle {
    broker: Arc<Broker>,
    session: Arc<Session>,
    token: String,
}
impl SessionHandle {
    pub fn endpoint(&self) -> (String, String) {
        (self.broker.address.to_string(), self.token.clone())
    }
    pub fn set_policy(&self, policy: Policy) {
        *self
            .session
            .policy
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = policy;
    }
}
type Job = Box<dyn FnOnce() + Send>;
pub struct Broker {
    address: SocketAddr,
    sessions: Mutex<HashMap<String, Weak<Session>>>,
    jars: Mutex<HashMap<PathBuf, Weak<Cookies>>>,
    cache: Mutex<Cache>,
    jobs: mpsc::SyncSender<Job>,
    client: LocalClient,
}
impl Drop for Broker {
    fn drop(&mut self) {
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(100));
    }
}
impl Broker {
    pub fn shared() -> Result<Arc<Self>, String> {
        static SHARED: OnceLock<Result<Arc<Broker>, String>> = OnceLock::new();
        SHARED.get_or_init(Self::new).clone()
    }
    pub fn new() -> Result<Arc<Self>, String> {
        let listener =
            TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
        let (jobs, receiver) = mpsc::sync_channel::<Job>(32);
        let receiver = Arc::new(Mutex::new(receiver));
        for index in 0..4 {
            let receiver = receiver.clone();
            std::thread::Builder::new()
                .name(format!("wega-broker-{index}"))
                .spawn(move || {
                    loop {
                        let Ok(job) = receiver
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .recv()
                        else {
                            break;
                        };
                        job();
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        let broker = Arc::new(Self {
            address: listener.local_addr().map_err(|e| e.to_string())?,
            sessions: Default::default(),
            jars: Default::default(),
            cache: Default::default(),
            jobs,
            client: LocalClient::new()?,
        });
        let weak = Arc::downgrade(&broker);
        std::thread::Builder::new()
            .name("wega-broker-accept".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Some(broker) = weak.upgrade() else { break };
                    if let Ok(mut stream) = stream {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(20)));
                        let result = broker.accept(&mut stream);
                        if let Err(error) = result {
                            let _ = write_packet(&mut stream, &Reply::Error(error), &[]);
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(broker)
    }
    pub fn open_session(
        self: &Arc<Self>,
        directory: PathBuf,
        policy: Policy,
    ) -> Result<SessionHandle, String> {
        let mut jars = self
            .jars
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        jars.retain(|_, jar| jar.strong_count() > 0);
        let cookies = if let Some(jar) = jars.get(&directory).and_then(Weak::upgrade) {
            jar
        } else {
            let jar = Arc::new(Cookies::default());
            jar.set_directory(directory.clone());
            jars.insert(directory.clone(), Arc::downgrade(&jar));
            jar
        };
        let mut bytes = [0; 32];
        getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
        let token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let session = Arc::new(Session {
            policy: RwLock::new(policy),
            cookies,
            scope: directory.to_string_lossy().into_owned(),
        });
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions.retain(|_, session| session.strong_count() > 0);
        sessions.insert(token.clone(), Arc::downgrade(&session));
        Ok(SessionHandle {
            broker: self.clone(),
            session,
            token,
        })
    }
    fn accept(self: &Arc<Self>, stream: &mut TcpStream) -> Result<(), String> {
        let (request, bytes): (WireRequest, usize) = read_header(stream)?;
        let token = match &request {
            WireRequest::Fetch { token, .. }
            | WireRequest::CookieGet { token, .. }
            | WireRequest::CookieSet { token, .. } => token,
        };
        let session = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(token)
            .and_then(Weak::upgrade)
            .ok_or("Invalid broker capability")?;
        match request {
            WireRequest::CookieGet {
                url,
                script,
                context,
                netscape,
                ..
            } => {
                if bytes != 0 {
                    return Err("Unexpected cookie body".into());
                };
                let url = Url::parse(&url).map_err(|e| e.to_string())?;
                let mut policy_url = url.clone();
                if url.scheme() == "ws" {
                    let _ = policy_url.set_scheme("http");
                }
                if url.scheme() == "wss" {
                    let _ = policy_url.set_scheme("https");
                }
                let mut policy = session
                    .policy
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                // Offline stops network I/O, not local document.cookie access.
                if script {
                    policy.offline = false;
                }
                let value = if policy.permits(&policy_url) {
                    if netscape {
                        session.cookies.netscape(&url)
                    } else {
                        session.cookies.get(&url, script, context)
                    }
                } else {
                    String::new()
                };
                write_packet(stream, &Reply::Cookie(String::new()), value.as_bytes())
            }
            WireRequest::CookieSet {
                url,
                value,
                script,
                context,
                ..
            } => {
                if bytes != 0 || value.len() > 4096 {
                    return Err("Invalid cookie request".into());
                };
                let url = Url::parse(&url).map_err(|e| e.to_string())?;
                let mut policy = session
                    .policy
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                if script {
                    policy.offline = false;
                }
                if policy.permits(&url) {
                    session.cookies.set(&url, &value, script, context)
                };
                write_packet(stream, &Reply::Done, &[])
            }
            WireRequest::Fetch { mut request, .. } => {
                request.validate()?;
                if bytes > MAX_BODY {
                    return Err("Upload exceeds its size limit".into());
                };
                let mut stream = stream.try_clone().map_err(|e| e.to_string())?;
                let client = self.client.clone();
                // Cache/session data outlives the native page only for this bounded job.
                let broker = self.clone();
                self.jobs
                    .try_send(Box::new(move || {
                        let result = (|| {
                            request.body = vec![0; bytes];
                            stream
                                .read_exact(&mut request.body)
                                .map_err(|e| e.to_string())?;
                            client.fetch_cached(
                                &request,
                                &session.policy,
                                &session.cookies,
                                Some((&broker.cache, &session.scope)),
                            )
                        })();
                        match result {
                            Ok(mut response) => {
                                let body = std::mem::take(&mut response.body);
                                let _ =
                                    write_packet(&mut stream, &Reply::Response(response), &body);
                            }
                            Err(error) => {
                                let _ = write_packet(&mut stream, &Reply::Error(error), &[]);
                            }
                        }
                    }))
                    .map_err(|_| "Shared network queue is full".to_string())
            }
        }
    }
}

#[derive(Clone)]
pub struct Remote {
    address: SocketAddr,
    token: String,
}
impl Remote {
    pub fn new(address: &str, token: String) -> Result<Self, String> {
        let address: SocketAddr = address.parse().map_err(|_| "Invalid broker address")?;
        if !address.ip().is_loopback() || token.len() != 64 {
            return Err("Invalid broker capability".into());
        };
        Ok(Self { address, token })
    }
    fn call(&self, request: impl Serialize, body: &[u8]) -> Result<(Reply, Vec<u8>), String> {
        let mut stream = TcpStream::connect_timeout(&self.address, Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        write_packet(&mut stream, &request, body)?;
        let (reply, len) = read_header(&mut stream)?;
        let mut body = vec![0; len];
        stream.read_exact(&mut body).map_err(|e| e.to_string())?;
        if let Reply::Error(error) = reply {
            return Err(error);
        };
        Ok((reply, body))
    }
    pub fn fetch(&self, request: &Request) -> Result<Response, String> {
        request.validate()?;
        let (reply, body) = self.call(
            FetchCall::Fetch {
                token: &self.token,
                request,
            },
            &request.body,
        )?;
        if let Reply::Response(mut response) = reply {
            if body.len() > request.limit {
                return Err("Broker response exceeds request limit".into());
            }
            response.body = body;
            Ok(response)
        } else {
            Err("Invalid broker response".into())
        }
    }
    pub fn cookie_get(&self, url: &Url, script: bool) -> Result<String, String> {
        self.cookie_read(url, script, SiteContext::SameSite, false)
    }
    pub fn cookie_read(
        &self,
        url: &Url,
        script: bool,
        context: SiteContext,
        netscape: bool,
    ) -> Result<String, String> {
        let (reply, body) = self.call(
            WireRequest::CookieGet {
                token: self.token.clone(),
                url: url.to_string(),
                script,
                context,
                netscape,
            },
            &[],
        )?;
        if let Reply::Cookie(value) = reply {
            if body.is_empty() {
                Ok(value)
            } else {
                String::from_utf8(body).map_err(|e| e.to_string())
            }
        } else {
            Err("Invalid cookie response".into())
        }
    }
    pub fn cookie_set(&self, url: &Url, value: &str, script: bool) -> Result<(), String> {
        self.cookie_write(url, value, script, SiteContext::SameSite)
    }
    pub fn cookie_write(
        &self,
        url: &Url,
        value: &str,
        script: bool,
        context: SiteContext,
    ) -> Result<(), String> {
        let (reply, _) = self.call(
            WireRequest::CookieSet {
                token: self.token.clone(),
                url: url.to_string(),
                value: value.into(),
                script,
                context,
            },
            &[],
        )?;
        if matches!(reply, Reply::Done) {
            Ok(())
        } else {
            Err("Invalid cookie response".into())
        }
    }
}
#[derive(Serialize)]
enum FetchCall<'a> {
    Fetch {
        token: &'a str,
        request: &'a Request,
    },
}
#[derive(Serialize, Deserialize)]
enum WireRequest {
    Fetch {
        token: String,
        request: Box<Request>,
    },
    CookieGet {
        token: String,
        url: String,
        script: bool,
        context: SiteContext,
        netscape: bool,
    },
    CookieSet {
        token: String,
        url: String,
        value: String,
        script: bool,
        context: SiteContext,
    },
}
#[derive(Serialize, Deserialize)]
enum Reply {
    Response(Response),
    Cookie(String),
    Done,
    Error(String),
}
fn write_packet<T: Serialize>(
    stream: &mut TcpStream,
    value: &T,
    body: &[u8],
) -> Result<(), String> {
    let header = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if header.len() > MAX_HEADER || body.len() > MAX_BODY {
        return Err("Oversized broker packet".into());
    };
    stream
        .write_all(b"WGNB")
        .and_then(|_| stream.write_all(&(header.len() as u32).to_le_bytes()))
        .and_then(|_| stream.write_all(&(body.len() as u32).to_le_bytes()))
        .and_then(|_| stream.write_all(&header))
        .and_then(|_| stream.write_all(body))
        .map_err(|e| e.to_string())
}
fn read_header<T: for<'de> Deserialize<'de>>(stream: &mut TcpStream) -> Result<(T, usize), String> {
    let mut fixed = [0; 12];
    stream.read_exact(&mut fixed).map_err(|e| e.to_string())?;
    let len = u32::from_le_bytes(fixed[4..8].try_into().unwrap()) as usize;
    let bytes = u32::from_le_bytes(fixed[8..12].try_into().unwrap()) as usize;
    if &fixed[..4] != b"WGNB" || len == 0 || len > MAX_HEADER || bytes > MAX_BODY {
        return Err("Invalid broker packet".into());
    };
    let mut header = vec![0; len];
    stream.read_exact(&mut header).map_err(|e| e.to_string())?;
    Ok((
        serde_json::from_slice(&header).map_err(|e| e.to_string())?,
        bytes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capabilities_partition_profiles_share_live_cookies_and_track_policy() {
        let broker = Broker::new().unwrap();
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let first = broker
            .open_session(a.path().into(), Policy::default())
            .unwrap();
        let second = broker
            .open_session(a.path().into(), Policy::default())
            .unwrap();
        let other = broker
            .open_session(b.path().into(), Policy::default())
            .unwrap();
        let remote = |session: &SessionHandle| {
            let (addr, token) = session.endpoint();
            Remote::new(&addr, token).unwrap()
        };
        let url = Url::parse("https://example.test/").unwrap();
        remote(&first)
            .cookie_set(&url, "shared=yes; Path=/", true)
            .unwrap();
        assert_eq!(
            remote(&second).cookie_get(&url, true).unwrap(),
            "shared=yes"
        );
        assert_eq!(remote(&other).cookie_get(&url, true).unwrap(), "");
        first.set_policy(Policy {
            offline: true,
            ..Default::default()
        });
        assert_eq!(remote(&first).cookie_get(&url, true).unwrap(), "shared=yes");
        remote(&first)
            .cookie_set(&url, "offline=kept; Path=/", true)
            .unwrap();
        assert!(
            remote(&first)
                .cookie_get(&url, true)
                .unwrap()
                .contains("offline=kept")
        );
        assert_eq!(remote(&first).cookie_get(&url, false).unwrap(), "");
        let (addr, token) = second.endpoint();
        let revoked = Remote::new(&addr, token).unwrap();
        drop(second);
        assert!(revoked.cookie_get(&url, true).is_err());
        assert!(Remote::new("203.0.113.1:1234", "0".repeat(64)).is_err());
    }
    #[test]
    fn cache_requires_explicit_public_freshness_and_retains_response_limits() {
        let mut cache = Cache::default();
        let mut response = Response {
            url: "https://example.test/a".into(),
            status: 200,
            headers: vec![("cache-control".into(), "public, max-age=60".into())],
            redirected: false,
            body: vec![7; 64],
        };
        cache.insert("a".into(), &response);
        assert!(cache.get("a", 32).is_err());
        assert_eq!(cache.get("a", 64).unwrap().unwrap().body.len(), 64);
        response
            .headers
            .push(("set-cookie".into(), "secret=yes".into()));
        cache.insert("b".into(), &response);
        assert!(cache.get("b", 64).unwrap().is_none());
        response.headers = vec![("cache-control".into(), "private, max-age=60".into())];
        cache.insert("c".into(), &response);
        assert!(cache.get("c", 64).unwrap().is_none());
        response.headers = vec![("cache-control".into(), "public, max-age=60".into())];
        response.body.clear();
        for i in 0..300 {
            cache.insert(i.to_string(), &response)
        }
        assert!(cache.entries.len() <= 128);
    }
}
