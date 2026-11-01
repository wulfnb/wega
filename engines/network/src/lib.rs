//! Bounded HTTP/cache/cookie broker. Capabilities travel only over private worker IPC.
//! The loopback socket speaks a binary-framed protocol, never HTTP or WebSocket.
use serde::{Deserialize, Serialize};
mod connection;
use connection::Connection;
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock, RwLock, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use url::Url;
use wega_protocol::{
    authority::{Origin, Principal},
    identity::{PartitionId, ServiceIdentity, ServiceSessionId},
};
mod cookies;
pub use cookies::{Cookies, SiteContext};
const MAX_HEADER: usize = 64 * 1024;
pub const MAX_BODY: usize = 8 * 1024 * 1024;
const CACHE_BYTES: usize = 32 * 1024 * 1024;

fn is_private_or_loopback(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        match ip {
            std::net::IpAddr::V4(v4) => {
                let octets = v4.octets();
                octets[0] == 127
                    || octets[0] == 10
                    || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                    || (octets[0] == 192 && octets[1] == 168)
                    || (octets[0] == 169 && octets[1] == 254)
                    || octets[0] == 0
            }
            std::net::IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
        }
    } else {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ResourceDestination {
    #[default]
    Empty,
    Document,
    Script,
    Style,
    Image,
    Font,
    Fetch,
    Xhr,
    WebSocket,
    Subresource,
}

#[derive(Clone, Debug)]
pub struct HstsEntry {
    pub expires_at: Instant,
    pub include_subdomains: bool,
}

#[derive(Default)]
pub struct HstsPolicy {
    entries: HashMap<String, HstsEntry>,
}

impl HstsPolicy {
    pub fn update(&mut self, host: &str, header_val: &str) {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if host.is_empty() || host.len() > 253 || header_val.len() > MAX_HEADER {
            return;
        }
        let mut max_age = None;
        let mut include_subdomains = false;
        for part in header_val.split(';') {
            let part = part.trim();
            if let Some(age_str) = part.strip_prefix("max-age=") {
                if let Ok(age) = age_str.trim().parse::<u64>() {
                    max_age = Some(age);
                }
            } else if part.eq_ignore_ascii_case("includesubdomains") {
                include_subdomains = true;
            }
        }
        if let Some(age) = max_age {
            if age == 0 {
                self.entries.remove(&host);
            } else {
                // Bound server-controlled state and duration arithmetic. A hostile
                // max-age must not panic and permanently remove a broker worker.
                let now = Instant::now();
                let Some(expires_at) =
                    now.checked_add(Duration::from_secs(age.min(365 * 24 * 60 * 60)))
                else {
                    return;
                };
                self.entries.retain(|_, entry| entry.expires_at > now);
                if self.entries.len() >= 4096
                    && !self.entries.contains_key(&host)
                    && let Some(oldest) = self
                        .entries
                        .iter()
                        .min_by_key(|(_, entry)| entry.expires_at)
                        .map(|(host, _)| host.clone())
                {
                    self.entries.remove(&oldest);
                }
                self.entries.insert(
                    host,
                    HstsEntry {
                        expires_at,
                        include_subdomains,
                    },
                );
            }
        }
    }

    pub fn should_upgrade(&mut self, host: &str) -> bool {
        let now = Instant::now();
        self.entries.retain(|_, entry| entry.expires_at > now);
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if let Some(entry) = self.entries.get(&host)
            && entry.expires_at > now
        {
            return true;
        }
        for (domain, entry) in &self.entries {
            if entry.include_subdomains
                && entry.expires_at > now
                && host.ends_with(&format!(".{domain}"))
            {
                return true;
            }
        }
        false
    }
}

#[derive(Clone, Default, Debug)]
pub struct Policy {
    pub offline: bool,
    pub blocked_hosts: String,
    pub allowed_hosts: String,
    pub allow_loopback: bool,
    pub blocked_destinations: Vec<ResourceDestination>,
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

    pub fn permits_request(&self, request: &Request, url: &Url) -> Result<(), String> {
        if !self.permits(url) {
            return Err("Network request blocked by browsing policy".into());
        }
        if self.blocked_destinations.contains(&request.destination) {
            return Err(format!(
                "Resource destination {:?} blocked by policy",
                request.destination
            ));
        }
        let target_host = url.host_str().unwrap_or_default();
        if is_private_or_loopback(target_host) && !self.allow_loopback {
            let allowed_explicitly = self.allowed_hosts.split_whitespace().any(|rule| {
                let rule = rule
                    .trim_start_matches("*.")
                    .trim_end_matches('.')
                    .to_ascii_lowercase();
                !rule.is_empty()
                    && (target_host == rule || target_host.ends_with(&format!(".{rule}")))
            });
            if !allowed_explicitly {
                let initiator_is_private = request
                    .initiator
                    .as_ref()
                    .and_then(|i| i.host_str())
                    .map(is_private_or_loopback)
                    .unwrap_or(false);
                let origin_host = request
                    .origin
                    .trim_start_matches("http://")
                    .trim_start_matches("https://")
                    .split(':')
                    .next()
                    .unwrap_or("");
                let origin_is_private = is_private_or_loopback(origin_host);
                if !initiator_is_private
                    && !origin_is_private
                    && !request.origin.is_empty()
                    && request.origin != "null"
                {
                    return Err(format!(
                        "Private network target '{}' blocked by policy",
                        target_host
                    ));
                }
            }
        }
        Ok(())
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
    #[serde(default)]
    pub destination: ResourceDestination,
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
            destination: ResourceDestination::Document,
            limit,
        }
    }
    pub fn subresource(url: &Url, destination: ResourceDestination, limit: usize) -> Self {
        Self {
            url: url.to_string(),
            method: "GET".into(),
            headers: vec![],
            body: vec![],
            origin: url.origin().ascii_serialization(),
            initiator: Some(url.clone()),
            top_level: false,
            credentials: "include".into(),
            redirect: "follow".into(),
            cache: "default".into(),
            destination,
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
        self.fetch_cached(request, policy, cookies, None, None)
    }
    pub fn fetch_with_hsts(
        &self,
        request: &Request,
        policy: &RwLock<Policy>,
        cookies: &Cookies,
        hsts: Option<&Mutex<HstsPolicy>>,
    ) -> Result<Response, String> {
        self.fetch_cached(request, policy, cookies, None, hsts)
    }
    fn fetch_cached(
        &self,
        request: &Request,
        policy: &RwLock<Policy>,
        cookies: &Cookies,
        cache: Option<(&Mutex<Cache>, &str)>,
        hsts: Option<&Mutex<HstsPolicy>>,
    ) -> Result<Response, String> {
        request.validate()?;
        let mut url = Url::parse(&request.url).map_err(|e| e.to_string())?;
        if url.scheme() == "http"
            && let Some(hsts) = hsts
        {
            let host = url.host_str().unwrap_or_default();
            if hsts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .should_upgrade(host)
            {
                let _ = url.set_scheme("https");
                if url.port() == Some(80) {
                    let _ = url.set_port(None);
                }
            }
        }
        let mut method = request.method.clone();
        let mut body = request.body.clone();
        let mut headers = request.headers.clone();
        for hop in 0..=8 {
            policy
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .permits_request(request, &url)?;
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
                    let mut next = url.join(location).map_err(|e| e.to_string())?;
                    if url.scheme() == "https" && next.scheme() == "http" {
                        let next_host = next.host_str().unwrap_or_default();
                        let upgraded = if let Some(hsts) = hsts {
                            hsts.lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .should_upgrade(next_host)
                        } else {
                            false
                        };
                        if upgraded {
                            let _ = next.set_scheme("https");
                            if next.port() == Some(80) {
                                let _ = next.set_port(None);
                            }
                        } else {
                            return Err(
                                "Insecure redirect downgrade from HTTPS to HTTP forbidden".into()
                            );
                        }
                    }
                    policy
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .permits_request(request, &next)?;
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
            if url.scheme() == "https"
                && let Some(hsts) = hsts
            {
                for (name, val) in &response.headers {
                    if name.eq_ignore_ascii_case("strict-transport-security")
                        && let Some(host) = url.host_str()
                    {
                        hsts.lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .update(host, val);
                    }
                }
            }
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
    identity: ServiceIdentity,
    bound: bool,
    inherited_only: AtomicBool,
    #[cfg(unix)]
    channels: Mutex<Vec<Arc<std::os::unix::net::UnixStream>>>,
    revoked: AtomicBool,
    navigation: Mutex<NavigationReceipt>,
}

#[derive(Default)]
struct NavigationReceipt {
    allowed: Vec<Url>,
    committed: Option<Url>,
    last: Option<Url>,
}

struct Jar {
    cookies: Weak<Cookies>,
    partition: PartitionId,
}

pub fn principal(url: &Url) -> Principal {
    Principal {
        origin: if matches!(url.scheme(), "http" | "https") {
            Origin::Tuple(url.origin().ascii_serialization())
        } else {
            Origin::Opaque
        },
        top_level_site: Cookies::site(url),
    }
}

fn nonce() -> Result<[u8; 16], String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    if bytes == [0; 16] {
        return Err("Invalid identity entropy".into());
    }
    Ok(bytes)
}
pub struct SessionHandle {
    broker: Arc<Broker>,
    session: Arc<Session>,
    token: String,
}
impl SessionHandle {
    pub fn identity(&self) -> ServiceIdentity {
        self.session.identity
    }
    pub fn begin_navigation(&self, url: &Url) {
        let mut receipt = self
            .session
            .navigation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut base = url.clone();
        base.set_fragment(None);
        let committed = receipt
            .last
            .as_ref()
            .filter(|last| {
                let mut last = (*last).clone();
                last.set_fragment(None);
                last == base
            })
            .map(|_| url.clone());
        receipt.allowed = vec![url.clone()];
        receipt.committed = committed;
    }
    /// Host-only attribution from a browser-approved navigation and broker-observed response.
    pub fn take_committed_principal(&self, claimed: Option<&str>) -> Principal {
        let mut receipt = self
            .session
            .navigation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let observed = receipt.committed.take().filter(|observed| {
            claimed
                .and_then(|url| Url::parse(url).ok())
                .is_some_and(|url| url.origin() == observed.origin())
        });
        if let Some(url) = observed {
            let principal = principal(&url);
            receipt.last = Some(url);
            principal
        } else {
            Principal {
                origin: Origin::Opaque,
                top_level_site: None,
            }
        }
    }
    pub fn revoke(&self) {
        self.session.revoked.store(true, Ordering::Release);
        #[cfg(unix)]
        for channel in self
            .session
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
        {
            let _ = channel.shutdown(std::net::Shutdown::Both);
        }
        self.broker
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.token);
    }
    pub fn download_cookies(&self, url: &Url) -> Result<String, String> {
        if self.session.revoked.load(Ordering::Acquire)
            || !self
                .session
                .policy
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .permits(url)
        {
            return Err("Download session is revoked or blocked".into());
        }
        Ok(self.session.cookies.netscape(url))
    }
    pub fn endpoint(&self) -> (String, String) {
        (self.broker.address.to_string(), self.token.clone())
    }
    pub fn hsts(&self) -> &Mutex<HstsPolicy> {
        &self.broker.hsts
    }
    pub fn set_policy(&self, policy: Policy) {
        *self
            .session
            .policy
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = policy;
    }
}
impl Drop for SessionHandle {
    fn drop(&mut self) {
        self.revoke();
    }
}

#[cfg(unix)]
impl SessionHandle {
    /// Each preconnected endpoint is tied to this service, independent of a token
    /// supplied inside the packet. This session can no longer use the TCP listener.
    pub fn inherited_channels(
        &self,
        count: usize,
    ) -> Result<Vec<std::os::unix::net::UnixStream>, String> {
        use std::os::unix::net::UnixStream;
        if !self.session.bound
            || count == 0
            || count > 4
            || self.session.revoked.load(Ordering::Acquire)
        {
            return Err("Invalid inherited broker session".into());
        }
        if self.session.inherited_only.swap(true, Ordering::AcqRel) {
            return Err("Broker transport already selected".into());
        }
        // Serialize publication with revoke(), including the revoked check. A
        // revocation during setup must not miss a newly created idle endpoint.
        let mut published = self
            .session
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.session.revoked.load(Ordering::Acquire) {
            return Err("Revoked broker capability".into());
        }
        let mut children = Vec::new();
        let mut controllers = Vec::new();
        for _ in 0..count {
            let (server, child) = UnixStream::pair().map_err(|error| error.to_string())?;
            server
                .set_write_timeout(Some(Duration::from_secs(5)))
                .map_err(|error| error.to_string())?;
            let controller = Arc::new(server.try_clone().map_err(|error| error.to_string())?);
            controllers.push(controller.clone());
            let broker = self.broker.clone();
            let expected = self.session.clone();
            std::thread::Builder::new()
                .name("wega-broker-channel".into())
                .spawn(move || {
                    let _shutdown = ChannelShutdown(controller);
                    let mut stream = server;
                    loop {
                        let _ = stream.set_read_timeout(None);
                        let mut first = [0_u8];
                        if stream.read_exact(&mut first).is_err() {
                            break;
                        }
                        if stream
                            .set_read_timeout(Some(Duration::from_secs(20)))
                            .is_err()
                        {
                            break;
                        }
                        let mut connection = Connection::Unix(stream);
                        let result =
                            broker.accept(&mut connection, Some(&expected), Some(first[0]));
                        let failed = match result {
                            Ok(Some(done)) => !done.recv().unwrap_or(false),
                            Ok(None) => false,
                            Err(error) => {
                                let _ = write_packet(&mut connection, &Reply::Error(error), &[]);
                                true
                            }
                        };
                        let Connection::Unix(returned) = connection else {
                            unreachable!()
                        };
                        stream = returned;
                        if failed || expected.revoked.load(Ordering::Acquire) {
                            break;
                        }
                    }
                })
                .map_err(|error| error.to_string())?;
            children.push(child);
        }
        *published = controllers;
        Ok(children)
    }
}

struct Completion {
    done: Option<mpsc::Sender<bool>>,
    reusable: bool,
}
#[cfg(unix)]
struct ChannelShutdown(Arc<std::os::unix::net::UnixStream>);
#[cfg(unix)]
impl Drop for ChannelShutdown {
    fn drop(&mut self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}
impl Drop for Completion {
    fn drop(&mut self) {
        if let Some(done) = self.done.take() {
            let _ = done.send(self.reusable);
        }
    }
}
type Job = Box<dyn FnOnce() + Send>;
pub struct Broker {
    address: SocketAddr,
    sessions: Mutex<HashMap<String, Weak<Session>>>,
    jars: Mutex<HashMap<PathBuf, Jar>>,
    cache: Mutex<Cache>,
    pub hsts: Mutex<HstsPolicy>,
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
            hsts: Default::default(),
            jobs,
            client: LocalClient::new()?,
        });
        let weak = Arc::downgrade(&broker);
        std::thread::Builder::new()
            .name("wega-broker-accept".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Some(broker) = weak.upgrade() else { break };
                    if let Ok(stream) = stream {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(20)));
                        let mut stream = Connection::Tcp(stream);
                        let result = broker.accept(&mut stream, None, None);
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
        self.open_session_kind(directory, policy, false)
    }
    pub fn open_bound_session(
        self: &Arc<Self>,
        directory: PathBuf,
        policy: Policy,
    ) -> Result<SessionHandle, String> {
        self.open_session_kind(directory, policy, true)
    }
    fn open_session_kind(
        self: &Arc<Self>,
        directory: PathBuf,
        policy: Policy,
        bound: bool,
    ) -> Result<SessionHandle, String> {
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let directory = directory
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let mut jars = self
            .jars
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        jars.retain(|_, jar| jar.cookies.strong_count() > 0);
        let (cookies, partition) = if let Some((cookies, partition)) =
            jars.get(&directory).and_then(|jar| {
                jar.cookies
                    .upgrade()
                    .map(|cookies| (cookies, jar.partition))
            }) {
            (cookies, partition)
        } else {
            let jar = Arc::new(Cookies::default());
            jar.set_directory(directory.clone());
            let partition = PartitionId(nonce()?);
            jars.insert(
                directory.clone(),
                Jar {
                    cookies: Arc::downgrade(&jar),
                    partition,
                },
            );
            (jar, partition)
        };
        let mut bytes = [0; 32];
        getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
        let token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let session = Arc::new(Session {
            identity: ServiceIdentity {
                service: ServiceSessionId(nonce()?),
                partition,
            },
            bound,
            inherited_only: AtomicBool::new(false),
            #[cfg(unix)]
            channels: Mutex::new(Vec::new()),
            revoked: AtomicBool::new(false),
            navigation: Mutex::new(NavigationReceipt::default()),
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
    fn accept(
        self: &Arc<Self>,
        stream: &mut Connection,
        expected: Option<&Arc<Session>>,
        first: Option<u8>,
    ) -> Result<Option<mpsc::Receiver<bool>>, String> {
        let (request, bytes): (WireRequest, usize) = read_header_prefix(stream, first)?;
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
        if session.revoked.load(Ordering::Acquire) {
            return Err("Revoked broker capability".into());
        }
        if expected.is_some_and(|expected| !Arc::ptr_eq(expected, &session))
            || (expected.is_none() && session.inherited_only.load(Ordering::Acquire))
        {
            return Err("Broker endpoint/session mismatch".into());
        }
        let identity = match &request {
            WireRequest::Fetch { identity, .. }
            | WireRequest::CookieGet { identity, .. }
            | WireRequest::CookieSet { identity, .. } => identity,
        };
        if session.bound && *identity != Some(session.identity) {
            return Err("Broker service/partition mismatch".into());
        }
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
                write_packet(stream, &Reply::Cookie(String::new()), value.as_bytes()).map(|_| None)
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
                write_packet(stream, &Reply::Done, &[]).map(|_| None)
            }
            WireRequest::Fetch { mut request, .. } => {
                request.validate()?;
                if bytes > MAX_BODY {
                    return Err("Upload exceeds its size limit".into());
                };
                if session.bound && !request.top_level {
                    let receipt = session
                        .navigation
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(ref committed) = receipt.last {
                        let committed_origin = committed.origin().ascii_serialization();
                        if !request.origin.is_empty()
                            && request.origin != "null"
                            && request.origin != committed_origin
                        {
                            return Err(format!(
                                "Spoofed request origin '{}' does not match session origin '{}'",
                                request.origin, committed_origin
                            ));
                        }
                        if let Some(ref initiator) = request.initiator
                            && initiator.origin().ascii_serialization() != committed_origin
                        {
                            return Err(format!(
                                "Spoofed request initiator '{}' does not match session origin '{}'",
                                initiator, committed_origin
                            ));
                        }
                    }
                }
                let mut stream = stream.try_clone().map_err(|e| e.to_string())?;
                let client = self.client.clone();
                // Cache/session data outlives the native page only for this bounded job.
                let broker = self.clone();
                let (done, completed) = mpsc::channel();
                self.jobs
                    .try_send(Box::new(move || {
                        let mut completion = Completion {
                            done: Some(done),
                            reusable: false,
                        };
                        let mut body_read = false;
                        let result = (|| {
                            if session.revoked.load(Ordering::Acquire) {
                                return Err("Revoked broker capability".into());
                            }
                            request.body = vec![0; bytes];
                            stream
                                .read_exact(&mut request.body)
                                .map_err(|e| e.to_string())?;
                            body_read = true;
                            let response = client.fetch_cached(
                                &request,
                                &session.policy,
                                &session.cookies,
                                Some((&broker.cache, &session.scope)),
                                Some(&broker.hsts),
                            )?;
                            if request.top_level {
                                let mut receipt = session
                                    .navigation
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                if receipt
                                    .allowed
                                    .iter()
                                    .any(|url| url.as_str() == request.url)
                                {
                                    if (300..400).contains(&response.status) {
                                        if let Some(next) = response
                                            .headers
                                            .iter()
                                            .find(|(name, _)| name.eq_ignore_ascii_case("location"))
                                            .and_then(|(_, location)| {
                                                Url::parse(&response.url).ok()?.join(location).ok()
                                            })
                                            && receipt.allowed.len() < 10
                                        {
                                            receipt.allowed.push(next);
                                        }
                                    } else {
                                        receipt.committed = Url::parse(&response.url).ok();
                                    }
                                }
                            }
                            if session.revoked.load(Ordering::Acquire) {
                                return Err("Revoked broker response".into());
                            }
                            Ok(response)
                        })();
                        let written = match result {
                            Ok(mut response) => {
                                let body = std::mem::take(&mut response.body);
                                write_packet(&mut stream, &Reply::Response(response), &body)
                            }
                            Err(error) => write_packet(&mut stream, &Reply::Error(error), &[]),
                        };
                        // A partial body, failed reply or panic destroys framing.
                        // Never read another request on that channel afterward.
                        completion.reusable = body_read && written.is_ok();
                    }))
                    .map_err(|_| "Shared network queue is full".to_string())?;
                Ok(Some(completed))
            }
        }
    }
}

#[derive(Clone)]
enum Endpoint {
    Tcp(SocketAddr),
    #[cfg(unix)]
    Inherited(Arc<connection::Pool>),
}
#[derive(Clone)]
pub struct Remote {
    endpoint: Endpoint,
    token: String,
    identity: Option<ServiceIdentity>,
}
impl Remote {
    pub fn new(address: &str, token: String) -> Result<Self, String> {
        let address: SocketAddr = address.parse().map_err(|_| "Invalid broker address")?;
        if !address.ip().is_loopback() || token.len() != 64 {
            return Err("Invalid broker capability".into());
        };
        Ok(Self {
            endpoint: Endpoint::Tcp(address),
            token,
            identity: None,
        })
    }
    pub fn bound(address: &str, token: String, identity: ServiceIdentity) -> Result<Self, String> {
        let mut remote = Self::new(address, token)?;
        remote.identity = Some(identity);
        Ok(remote)
    }
    #[cfg(unix)]
    pub fn inherited(
        channels: Vec<std::os::unix::net::UnixStream>,
        token: String,
        identity: ServiceIdentity,
    ) -> Result<Self, String> {
        if token.len() != 64 {
            return Err("Invalid broker capability".into());
        }
        Ok(Self {
            endpoint: Endpoint::Inherited(Arc::new(connection::Pool::new(channels)?)),
            token,
            identity: Some(identity),
        })
    }
    fn call(&self, request: impl Serialize, body: &[u8]) -> Result<(Reply, Vec<u8>), String> {
        #[cfg(unix)]
        if let Endpoint::Inherited(pool) = &self.endpoint {
            let (reply, body) = pool.call(|stream| {
                write_packet(stream, &request, body).map_err(std::io::Error::other)?;
                let (reply, len): (Reply, usize) =
                    read_header(stream).map_err(std::io::Error::other)?;
                let mut body = vec![0; len];
                stream.read_exact(&mut body)?;
                Ok((reply, body))
            })?;
            return if let Reply::Error(error) = reply {
                Err(error)
            } else {
                Ok((reply, body))
            };
        }
        let address = match self.endpoint {
            Endpoint::Tcp(address) => address,
            #[cfg(unix)]
            Endpoint::Inherited(_) => unreachable!(),
        };
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
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
                identity: self.identity,
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
                identity: self.identity,
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
                identity: self.identity,
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
        identity: Option<ServiceIdentity>,
        token: &'a str,
        request: &'a Request,
    },
}
#[derive(Serialize, Deserialize)]
enum WireRequest {
    Fetch {
        #[serde(default)]
        identity: Option<ServiceIdentity>,
        token: String,
        request: Box<Request>,
    },
    CookieGet {
        #[serde(default)]
        identity: Option<ServiceIdentity>,
        token: String,
        url: String,
        script: bool,
        context: SiteContext,
        netscape: bool,
    },
    CookieSet {
        #[serde(default)]
        identity: Option<ServiceIdentity>,
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
    stream: &mut impl Write,
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
fn read_header<T: for<'de> Deserialize<'de>>(stream: &mut impl Read) -> Result<(T, usize), String> {
    read_header_prefix(stream, None)
}
fn read_header_prefix<T: for<'de> Deserialize<'de>>(
    stream: &mut impl Read,
    first: Option<u8>,
) -> Result<(T, usize), String> {
    let mut fixed = [0; 12];
    if let Some(first) = first {
        fixed[0] = first;
        stream
            .read_exact(&mut fixed[1..])
            .map_err(|error| error.to_string())?;
    } else {
        stream
            .read_exact(&mut fixed)
            .map_err(|error| error.to_string())?;
    }
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
    fn server_controlled_hsts_durations_and_entry_counts_are_bounded() {
        let mut policy = HstsPolicy::default();
        policy.update("huge.example", &format!("max-age={}", u64::MAX));
        assert!(policy.should_upgrade("huge.example"));
        assert!(
            policy.entries["huge.example"]
                .expires_at
                .saturating_duration_since(Instant::now())
                <= Duration::from_secs(365 * 24 * 60 * 60)
        );
        for index in 0..4200 {
            policy.update(&format!("{index}.example"), "max-age=100");
        }
        assert!(policy.entries.len() <= 4096);
    }
    #[test]
    fn bound_services_reject_missing_cross_partition_and_revoked_capabilities() {
        let broker = Broker::new().unwrap();
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let first = broker
            .open_bound_session(a.path().into(), Policy::default())
            .unwrap();
        let second = broker
            .open_bound_session(a.path().join("."), Policy::default())
            .unwrap();
        let other = broker
            .open_bound_session(b.path().into(), Policy::default())
            .unwrap();
        assert_eq!(first.identity().partition, second.identity().partition);
        assert_ne!(first.identity().service, second.identity().service);
        assert_ne!(first.identity().partition, other.identity().partition);
        let (address, token) = first.endpoint();
        let url = Url::parse("https://example.test/").unwrap();
        assert!(
            Remote::new(&address, token.clone())
                .unwrap()
                .cookie_get(&url, true)
                .is_err()
        );
        assert!(
            Remote::bound(&address, token.clone(), second.identity())
                .unwrap()
                .cookie_set(&url, "bad=1", true)
                .is_err()
        );
        let wrong = ServiceIdentity {
            partition: other.identity().partition,
            ..first.identity()
        };
        assert!(
            Remote::bound(&address, token.clone(), wrong)
                .unwrap()
                .cookie_set(&url, "bad=2", true)
                .is_err()
        );
        let remote = Remote::bound(&address, token, first.identity()).unwrap();
        remote.cookie_set(&url, "allowed=1; Path=/", true).unwrap();
        assert_eq!(remote.cookie_get(&url, true).unwrap(), "allowed=1");
        let inflight = first.session.clone(); // Jobs can retain Arc<Session> after the page closes.
        first.revoke();
        assert!(remote.cookie_get(&url, true).is_err());
        assert!(first.download_cookies(&url).is_err());
        assert!(inflight.revoked.load(Ordering::Acquire));
        let (address, token) = second.endpoint();
        assert_eq!(
            Remote::bound(&address, token, second.identity())
                .unwrap()
                .cookie_get(&url, true)
                .unwrap(),
            "allowed=1"
        );
    }

    #[test]
    fn only_browser_approved_observed_navigation_can_supply_a_native_principal() {
        // Exercise real transport; page-supplied origin/top_level do not select native authority.
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        server.set_nonblocking(true).unwrap();
        let url = Url::parse(&format!("http://{}/page", server.local_addr().unwrap())).unwrap();
        let address = server.local_addr().unwrap();
        let serving = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            for _ in 0..3 {
                let mut stream = loop {
                    match server.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "Navigation fixture deadline"
                            );
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("Navigation fixture: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    header.push(byte[0]);
                    assert!(header.len() < 4096);
                }
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi").unwrap();
            }
        });
        let broker = Broker::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let session = broker
            .open_bound_session(
                dir.path().into(),
                Policy {
                    allow_loopback: true,
                    ..Policy::default()
                },
            )
            .unwrap();
        let (endpoint, token) = session.endpoint();
        let remote = Remote::bound(&endpoint, token, session.identity()).unwrap();
        let mut request = Request::get(&url, 4096);
        request.origin = "https://forged.test".into();
        remote.fetch(&request).unwrap();
        assert_eq!(
            session.take_committed_principal(Some(url.as_str())).origin,
            Origin::Opaque
        );
        session.begin_navigation(&url);
        remote.fetch(&request).unwrap();
        assert_eq!(
            session
                .take_committed_principal(Some("https://forged.test/"))
                .origin,
            Origin::Opaque
        );
        session.begin_navigation(&url);
        remote.fetch(&request).unwrap();
        assert_eq!(
            session.take_committed_principal(Some(url.as_str())).origin,
            Origin::Tuple(format!("http://{address}"))
        );
        assert_eq!(
            session.take_committed_principal(Some(url.as_str())).origin,
            Origin::Opaque,
            "Receipt is consumed once"
        );
        let mut fragment = url.clone();
        fragment.set_fragment(Some("section"));
        session.begin_navigation(&fragment);
        assert_eq!(
            session
                .take_committed_principal(Some(fragment.as_str()))
                .origin,
            principal(&url).origin
        );
        serving.join().unwrap();
    }
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
