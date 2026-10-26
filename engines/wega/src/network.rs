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
type Task = Box<dyn FnOnce(&Client) + Send>;
#[derive(Clone)]
pub(crate) struct Network {
    jobs: mpsc::SyncSender<Task>,
    pub policy: Arc<std::sync::RwLock<NetworkPolicy>>,
    pub wake: Arc<dyn Fn() + Send + Sync>,
    pub cookies: Arc<crate::cookies::Cookies>,
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
        // Tab workers have one lightweight broker dispatcher, no TLS/client pool.
        // The standalone library retains a bounded local pool for in-process users.
        for index in 0..if brokered { 1 } else { 4 } {
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
    pub fn fetch(
        client: &Client,
        policy: &NetworkPolicy,
        url: &Url,
        limit: usize,
        local_base: Option<&std::path::Path>,
    ) -> Result<(Url, Vec<u8>), String> {
        let (resolved, mut reader): (Url, Box<dyn Read>) = match url.scheme() {
            "http" | "https" => {
                if !policy.permits(url) {
                    return Err("Network request blocked by browsing policy".into());
                }
                let response = client.response(&wega_network::Request::get(url, limit))?;
                if !(200..300).contains(&response.status) {
                    return Err(format!("HTTP {}", response.status));
                }
                return Ok((
                    Url::parse(&response.url).map_err(|e| e.to_string())?,
                    response.body,
                ));
            }
            "file" => {
                let path = url
                    .to_file_path()
                    .map_err(|_| "Invalid file address")?
                    .canonicalize()
                    .map_err(|e| e.to_string())?;
                if local_base.is_none_or(|base| !path.starts_with(base)) {
                    return Err("Local file request is outside this document's directory".into());
                }
                (
                    url.clone(),
                    Box::new(std::fs::File::open(path).map_err(|e| e.to_string())?),
                )
            }
            "data" => {
                if url.as_str().len() > limit.saturating_mul(2) {
                    return Err("Data resource exceeds the size limit".into());
                }
                let data = data_url::DataUrl::process(url.as_str())
                    .map_err(|_| "Invalid data resource")?
                    .decode_to_vec()
                    .map_err(|_| "Invalid data encoding")?
                    .0;
                if data.len() > limit {
                    return Err("Resource exceeds the size limit".into());
                }
                return Ok((url.clone(), data));
            }
            _ => return Err("Unsupported address scheme; use Servo for this page".into()),
        };
        let mut bytes = Vec::new();
        reader
            .by_ref()
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > limit {
            return Err("Resource exceeds the size limit".into());
        }
        Ok((resolved, bytes))
    }
}
