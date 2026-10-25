use std::{
    io::Read,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use url::Url;

#[derive(Clone, Default, Debug)]
pub struct NetworkPolicy {
    pub offline: bool,
    pub blocked_hosts: String,
    pub allowed_hosts: String,
}
impl NetworkPolicy {
    pub fn permits(&self, url: &Url) -> bool {
        if self.offline {
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
        matches!(url.scheme(), "http" | "https")
            && !matches(&self.blocked_hosts)
            && (self.allowed_hosts.trim().is_empty() || matches(&self.allowed_hosts))
    }
}
type Task = Box<dyn FnOnce(&reqwest::blocking::Client) + Send>;
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
        let policy = Arc::new(std::sync::RwLock::new(policy));
        let redirect_policy = policy.clone();
        let cookies = Arc::new(crate::cookies::Cookies::default());
        let client = reqwest::blocking::Client::builder()
            .cookie_provider(cookies.clone())
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .pool_max_idle_per_host(2)
            .user_agent("Wega/0.1 (experimental Blitz engine)")
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 8
                    || !redirect_policy
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .permits(attempt.url())
                {
                    attempt.error("Redirect blocked")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|e| e.to_string())?;
        let (jobs, receiver) = mpsc::sync_channel::<Task>(32);
        let receiver = Arc::new(Mutex::new(receiver));
        for index in 0..4 {
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
                .map_err(|e| e.to_string())?;
        }
        Ok(Self {
            jobs,
            policy,
            wake,
            storage_dir: None,
            cookies,
        })
    }
    pub fn spawn(
        &self,
        task: impl FnOnce(&reqwest::blocking::Client) + Send + 'static,
    ) -> Result<(), String> {
        self.jobs
            .try_send(Box::new(task))
            .map_err(|_| "Network queue is full".into())
    }
    pub fn fetch(
        client: &reqwest::blocking::Client,
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
                let response = client
                    .get(url.clone())
                    .send()
                    .and_then(|r| r.error_for_status())
                    .map_err(|e| e.to_string())?;
                (response.url().clone(), Box::new(response))
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
