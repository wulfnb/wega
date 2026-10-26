use crate::cookies::Cookies;
use std::{
    io::Read,
    sync::{Arc, Mutex, mpsc},
};
use url::Url;

pub use wega_network::Policy as NetworkPolicy;
#[derive(Clone)]
pub(crate) enum Client {
    Local {
        http: wega_network::LocalClient,
        policy: Arc<std::sync::RwLock<NetworkPolicy>>,
        cookies: Arc<wega_network::Cookies>,
    },
    Remote(wega_network::Remote),
}
impl Client {
    pub fn response(
        &self,
        request: &wega_network::Request,
    ) -> Result<wega_network::Response, String> {
        match self {
            Self::Local {
                http,
                policy,
                cookies,
            } => http.fetch(request, policy, cookies),
            Self::Remote(remote) => remote.fetch(request),
        }
    }
}

/// What a request is for; documents are top-level navigations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Destination {
    Document,
    Script,
    Style,
    Image,
    Font,
    #[default]
    Other,
}

/// Everything about a request besides its URL.
#[derive(Clone, Debug)]
pub(crate) struct FetchContext {
    pub method: reqwest::Method,
    pub body: Option<Vec<u8>>,
    pub content_type: Option<String>,
    /// The document that made the request; None for navigations the user started.
    pub initiator: Option<Url>,
    pub referrer_policy: Option<String>,
    pub destination: Destination,
}
impl Default for FetchContext {
    fn default() -> Self {
        Self {
            method: reqwest::Method::GET,
            body: None,
            content_type: None,
            initiator: None,
            referrer_policy: None,
            destination: Destination::Other,
        }
    }
}
impl FetchContext {
    pub fn subresource(initiator: &Url, destination: Destination) -> Self {
        Self {
            initiator: Some(initiator.clone()),
            destination,
            ..Default::default()
        }
    }
}

pub(crate) struct Response {
    pub url: Url,
    pub status: u16,
    pub headers: reqwest::header::HeaderMap,
    pub bytes: Vec<u8>,
}
impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
    fn local(url: Url, bytes: Vec<u8>, content_type: Option<&str>) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(value) = content_type.and_then(|t| t.parse().ok()) {
            headers.insert(reqwest::header::CONTENT_TYPE, value);
        }
        Self {
            url,
            status: 200,
            headers,
            bytes,
        }
    }
}

/// Mixed content: a secure document never loads insecure subresources. Top-level
/// navigations may still move from https to http.
pub(crate) fn mixed_content(initiator: Option<&Url>, url: &Url, top_level: bool) -> bool {
    !top_level
        && initiator.is_some_and(|i| matches!(i.scheme(), "https" | "wss"))
        && matches!(url.scheme(), "http" | "ws")
        && !is_loopback(url)
}
fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(host)) => host == "localhost" || host.ends_with(".localhost"),
        None => false,
    }
}

/// The Referer value for a request, per Referrer-Policy (default
/// strict-origin-when-cross-origin).
pub(crate) fn referrer(initiator: &Url, target: &Url, policy: Option<&str>) -> Option<String> {
    if !matches!(initiator.scheme(), "http" | "https") {
        return None;
    }
    let mut full = initiator.clone();
    full.set_fragment(None);
    let _ = full.set_username("");
    let _ = full.set_password(None);
    let origin = format!("{}/", initiator.origin().ascii_serialization());
    let same_origin = initiator.origin() == target.origin();
    let downgrade = initiator.scheme() == "https" && target.scheme() == "http";
    let policy = policy
        .and_then(|p| p.split(',').map(str::trim).rfind(|p| !p.is_empty()))
        .unwrap_or("strict-origin-when-cross-origin")
        .to_ascii_lowercase();
    match policy.as_str() {
        "no-referrer" => None,
        "no-referrer-when-downgrade" => (!downgrade).then(|| full.to_string()),
        "origin" => Some(origin),
        "origin-when-cross-origin" => Some(if same_origin {
            full.to_string()
        } else {
            origin
        }),
        "same-origin" => same_origin.then(|| full.to_string()),
        "strict-origin" => (!downgrade).then_some(origin),
        "unsafe-url" => Some(full.to_string()),
        _ if same_origin => Some(full.to_string()),
        _ => (!downgrade).then_some(origin),
    }
}

type Task = Box<dyn FnOnce(&Client) + Send>;
#[derive(Clone)]
pub(crate) struct Network {
    jobs: mpsc::SyncSender<Task>,
    pub policy: Arc<std::sync::RwLock<NetworkPolicy>>,
    pub wake: Arc<dyn Fn() + Send + Sync>,
    pub cookies: Arc<Cookies>,
    pub storage_dir: Option<std::path::PathBuf>,
}
impl Network {
    pub fn new(policy: NetworkPolicy, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        Self::create(policy, wake, None)
    }
    pub fn brokered(
        policy: NetworkPolicy,
        wake: Arc<dyn Fn() + Send + Sync>,
        remote: wega_network::Remote,
    ) -> Result<Self, String> {
        Self::create(policy, wake, Some(remote))
    }
    fn create(
        policy: NetworkPolicy,
        wake: Arc<dyn Fn() + Send + Sync>,
        remote: Option<wega_network::Remote>,
    ) -> Result<Self, String> {
        let policy = Arc::new(std::sync::RwLock::new(policy));
        let brokered = remote.is_some();
        let (client, cookies) = if let Some(remote) = remote {
            (
                Client::Remote(remote.clone()),
                Arc::new(crate::cookies::Cookies::Remote(remote)),
            )
        } else {
            let cookies = Arc::new(wega_network::Cookies::default());
            (
                Client::Local {
                    http: wega_network::LocalClient::new()?,
                    policy: policy.clone(),
                    cookies: cookies.clone(),
                },
                Arc::new(crate::cookies::Cookies::Local(cookies)),
            )
        };
        let (jobs, receiver) = mpsc::sync_channel::<Task>(32);
        let receiver = Arc::new(Mutex::new(receiver));
        // Tab workers keep two bounded broker dispatchers so independent resources
        // can complete out of order. They own no HTTP/TLS clients.
        // The standalone library retains a bounded local pool for in-process users.
        for index in 0..if brokered { 2 } else { 4 } {
            let receiver = receiver.clone();
            let client = client.clone();
            std::thread::Builder::new()
                .name(format!("wega-net-{index}"))
                .spawn(move || {
                    loop {
                        let Ok(job) = receiver
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .recv()
                        else {
                            break;
                        };
                        job(&client);
                    }
                })
                .map_err(|error| error.to_string())?;
        }
        Ok(Self {
            jobs,
            policy,
            wake,
            storage_dir: None,
            cookies,
        })
    }
    pub fn spawn(&self, task: impl FnOnce(&Client) + Send + 'static) -> Result<(), String> {
        self.jobs
            .try_send(Box::new(task))
            .map_err(|_| "Network queue is full".into())
    }
    fn policy(&self) -> NetworkPolicy {
        self.policy
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub fn fetch(
        &self,
        client: &Client,
        url: &Url,
        context: &FetchContext,
        limit: usize,
        local_base: Option<&std::path::Path>,
    ) -> Result<Response, String> {
        match url.scheme() {
            "http" | "https" => self.fetch_http(client, url, context, limit),
            "file" => {
                let path = url
                    .to_file_path()
                    .map_err(|_| "Invalid file address")?
                    .canonicalize()
                    .map_err(|e| e.to_string())?;
                if local_base.is_none_or(|base| !path.starts_with(base)) {
                    return Err("Local file request is outside this document's directory".into());
                }
                let mut bytes = Vec::new();
                std::fs::File::open(&path)
                    .map_err(|e| e.to_string())?
                    .take(limit as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > limit {
                    return Err("Resource exceeds the size limit".into());
                }
                let html = path.extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm")
                });
                Ok(Response::local(
                    url.clone(),
                    bytes,
                    html.then_some("text/html"),
                ))
            }
            "data" => {
                if url.as_str().len() > limit.saturating_mul(2) {
                    return Err("Data resource exceeds the size limit".into());
                }
                let parsed = data_url::DataUrl::process(url.as_str())
                    .map_err(|_| "Invalid data resource")?;
                let mime = parsed.mime_type().to_string();
                let data = parsed
                    .decode_to_vec()
                    .map_err(|_| "Invalid data encoding")?
                    .0;
                if data.len() > limit {
                    return Err("Resource exceeds the size limit".into());
                }
                Ok(Response::local(url.clone(), data, Some(&mime)))
            }
            _ => Err("Unsupported address scheme; use Servo for this page".into()),
        }
    }
    fn fetch_http(
        &self,
        client: &Client,
        url: &Url,
        context: &FetchContext,
        limit: usize,
    ) -> Result<Response, String> {
        let top_level = context.destination == Destination::Document;
        let mut url = url.clone();
        let mut method = context.method.clone();
        let mut body = context.body.clone();
        let mut content_type = context.content_type.clone();
        for _hop in 0..=8 {
            if !self.policy().permits(&url) {
                return Err("Network request blocked by browsing policy".into());
            }
            if mixed_content(context.initiator.as_ref(), &url, top_level) {
                return Err(format!("Mixed content blocked: {url}"));
            }
            let mut request = wega_network::Request::get(&url, limit);
            request.method = method.to_string();
            request.body = body.clone().unwrap_or_default();
            request.initiator = context.initiator.clone();
            request.top_level = top_level;
            request.redirect = "manual".into();
            if let Some(initiator) = &context.initiator
                && let Some(value) = referrer(initiator, &url, context.referrer_policy.as_deref())
            {
                request.headers.push(("referer".into(), value));
            }
            if let Some(kind) = &content_type {
                request.headers.push(("content-type".into(), kind.clone()));
            }
            let response = client.response(&request)?;
            let status =
                reqwest::StatusCode::from_u16(response.status).map_err(|e| e.to_string())?;
            let mut headers = reqwest::header::HeaderMap::new();
            for (name, value) in response.headers {
                if let (Ok(name), Ok(value)) =
                    (name.parse::<reqwest::header::HeaderName>(), value.parse())
                {
                    headers.append(name, value);
                }
            }
            if status.is_redirection()
                && let Some(location) = headers
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
            {
                let next = url.join(location).map_err(|e| e.to_string())?;
                if status == reqwest::StatusCode::SEE_OTHER
                    || (matches!(status.as_u16(), 301 | 302) && method == reqwest::Method::POST)
                {
                    if method != reqwest::Method::HEAD {
                        method = reqwest::Method::GET;
                    }
                    body = None;
                    content_type = None;
                }
                url = next;
                continue;
            }
            if !status.is_success() && !top_level {
                return Err(format!("HTTP status {status}"));
            }
            let bytes = response.body;
            return Ok(Response {
                url,
                status: status.as_u16(),
                headers,
                bytes,
            });
        }
        Err("Too many redirects".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }
    #[test]
    fn referrer_defaults_to_strict_origin_when_cross_origin() {
        let page = url("https://a.example/path/page?q=1#frag");
        assert_eq!(
            referrer(&page, &url("https://a.example/x"), None).as_deref(),
            Some("https://a.example/path/page?q=1")
        );
        assert_eq!(
            referrer(&page, &url("https://b.example/x"), None).as_deref(),
            Some("https://a.example/")
        );
        assert_eq!(referrer(&page, &url("http://b.example/x"), None), None);
        assert_eq!(
            referrer(&page, &url("https://a.example/x"), Some("no-referrer")),
            None
        );
    }
    #[test]
    fn secure_documents_block_insecure_subresources_only() {
        let secure = url("https://a.example/");
        assert!(mixed_content(
            Some(&secure),
            &url("http://b.example/x.js"),
            false
        ));
        assert!(!mixed_content(
            Some(&secure),
            &url("http://b.example/"),
            true
        ));
        assert!(!mixed_content(
            Some(&secure),
            &url("http://127.0.0.1/x.js"),
            false
        ));
        assert!(!mixed_content(
            Some(&url("http://a.example/")),
            &url("http://b.example/"),
            false
        ));
    }
}
