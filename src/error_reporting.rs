//! Durable error outbox and asynchronous delivery. Receivers deduplicate by id.
use crate::diagnostic_event::Diagnostic;
pub use crate::error_report_model::Report;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub endpoint: String,
}
impl Config {
    pub fn load(profile: &Path) -> Result<Option<Self>, String> {
        let path = profile.join("error-reporting.json");
        let mut config = match fs::read(&path) {
            Ok(bytes) => Some(
                serde_json::from_slice::<Self>(&bytes)
                    .map_err(|_| "Invalid error-reporting.json configuration".to_owned())?,
            ),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(_) => return Err("Cannot read error-reporting.json".into()),
        };
        if let Ok(endpoint) = std::env::var("WEGA_ERROR_REPORT_URL") {
            config = Some(Self { endpoint });
        }
        if let Some(config) = &config {
            config.validate()?;
        }
        Ok(config)
    }
    pub fn validate(&self) -> Result<(), String> {
        let url = url::Url::parse(&self.endpoint)
            .map_err(|_| "Invalid error-report endpoint".to_owned())?;
        let loopback = match url.host() {
            Some(url::Host::Domain("localhost")) => true,
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err("Error-report endpoint must use HTTPS (HTTP is allowed for loopback tests), without URL credentials or a fragment".into());
        }
        if url.path() != "/v1/errors" || url.query().is_some() {
            return Err("Error-report endpoint must end in /v1/errors, without a query".into());
        }
        Ok(())
    }
}
struct Outbox {
    directory: PathBuf,
}
fn private_file(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
impl Outbox {
    fn new(profile: &Path) -> io::Result<Self> {
        let directory = profile.join("logs/error-outbox");
        fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self { directory })
    }
    fn enqueue(&self, source: &str, diagnostic: Diagnostic) -> io::Result<()> {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
        let id = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let report = Report {
            schema_version: 1,
            id: id.clone(),
            occurred_at: chrono::Utc::now().to_rfc3339(),
            application: "Wega".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            source: source.chars().take(64).collect(),
            diagnostic: diagnostic.bounded(),
        };
        let temporary = self.directory.join(format!("{id}.tmp"));
        let result = (|| {
            let mut file = private_file(&temporary)?;
            file.write_all(&serde_json::to_vec(&report)?)?;
            file.sync_all()?;
            fs::rename(&temporary, self.directory.join(format!("{id}.json")))
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
    fn deliver_one(&self, transport: &mut impl Transport) -> io::Result<bool> {
        for entry in fs::read_dir(&self.directory)? {
            let path = entry?.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let bytes = fs::read(&path)?;
            let report: Report = match serde_json::from_slice(&bytes) {
                Ok(report) => report,
                Err(_) => {
                    // Preserve damaged files for inspection without blocking every
                    // subsequent report indefinitely.
                    fs::rename(&path, path.with_extension("corrupt"))?;
                    crate::diagnostics::record("error-reporter", "Corrupt outbox file quarantined");
                    continue;
                }
            };
            transport
                .post(&report)
                .map_err(|error| io::Error::other(error.to_string()))?;
            fs::remove_file(path)?;
            return Ok(true);
        }
        Ok(false)
    }
}
pub trait Transport: Send + 'static {
    fn post(&mut self, report: &Report) -> Result<(), String>;
}
pub struct Reporter {
    outbox: Arc<Mutex<Outbox>>,
    wake: mpsc::SyncSender<()>,
}
impl Reporter {
    pub fn buffer(profile: &Path) -> io::Result<Self> {
        let (wake, _receiver) = mpsc::sync_channel(1);
        Ok(Self {
            outbox: Arc::new(Mutex::new(Outbox::new(profile)?)),
            wake,
        })
    }

    pub fn start(profile: &Path, mut transport: impl Transport) -> io::Result<Self> {
        let outbox = Arc::new(Mutex::new(Outbox::new(profile)?));
        let shared = outbox.clone();
        let (wake, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("error-reporter".into())
            .spawn(move || {
                let mut delay = Duration::from_secs(1);
                loop {
                    // Network requests never hold the producer lock. In-flight files
                    // remain on disk, so a crash retries the same report id.
                    let directory = shared
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .directory
                        .clone();
                    let result = Outbox { directory }.deliver_one(&mut transport);
                    match result {
                        Ok(true) => {
                            delay = Duration::from_secs(1);
                            continue;
                        }
                        Ok(false) => {
                            if receiver.recv().is_err() {
                                break;
                            }
                        }
                        Err(_) => {
                            crate::diagnostics::record(
                                "error-reporter",
                                "Delivery failed; report retained for retry",
                            );
                            // New errors wake an idle reporter, but cannot bypass backoff.
                            std::thread::sleep(delay);
                            loop {
                                match receiver.try_recv() {
                                    Ok(()) => {}
                                    Err(mpsc::TryRecvError::Empty) => break,
                                    Err(mpsc::TryRecvError::Disconnected) => return,
                                }
                            }
                            delay = (delay * 2).min(Duration::from_secs(300));
                        }
                    }
                }
            })?;
        let reporter = Self { outbox, wake };
        let _ = reporter.wake.try_send(());
        Ok(reporter)
    }
    pub fn enqueue(&self, source: &str, diagnostic: Diagnostic) -> io::Result<()> {
        self.outbox
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .enqueue(source, diagnostic)?;
        let _ = self.wake.try_send(());
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct SavedIdentity {
    client_id: String,
    seed: String,
}
pub struct Identity {
    pub client_id: String,
    seed: [u8; 32],
}
impl Identity {
    pub fn sign(&self, timestamp: &str, nonce: &str, body: &[u8]) -> String {
        crate::error_auth::sign(&self.seed, &self.client_id, timestamp, nonce, body)
    }
    pub fn load(profile: &Path) -> io::Result<Self> {
        let directory = profile.join("error-reporting");
        fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        let path = directory.join("identity.json");
        if !path.exists() {
            let mut seed = [0; 32];
            let mut id = [0; 16];
            getrandom::fill(&mut seed).map_err(|error| io::Error::other(error.to_string()))?;
            getrandom::fill(&mut id).map_err(|error| io::Error::other(error.to_string()))?;
            let saved = SavedIdentity {
                client_id: crate::error_auth::hex(&id),
                seed: crate::error_auth::hex(&seed),
            };
            let temporary = directory.join(format!("identity-{}.tmp", saved.client_id));
            let mut file = private_file(&temporary)?;
            file.write_all(&serde_json::to_vec(&saved)?)?;
            file.sync_all()?;
            // Link atomically without replacing another process's established key.
            let result = fs::hard_link(&temporary, &path);
            let _ = fs::remove_file(&temporary);
            if let Err(e) = result
                && e.kind() != io::ErrorKind::AlreadyExists
            {
                return Err(e);
            }
        }
        let saved: SavedIdentity = serde_json::from_slice(&fs::read(path)?)?;
        crate::error_auth::unhex::<16>(&saved.client_id)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let seed = crate::error_auth::unhex::<32>(&saved.seed)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let public = serde_json::json!({"client_id":saved.client_id,"public_key":crate::error_auth::hex(&crate::error_auth::public_key(&seed))});
        fs::write(
            directory.join("enrollment.json"),
            serde_json::to_vec_pretty(&public)?,
        )?;
        Ok(Self {
            client_id: saved.client_id,
            seed,
        })
    }
}

#[cfg(feature = "desktop")]
pub struct HttpTransport {
    client: reqwest::blocking::Client,
    config: Config,
    identity: Identity,
}
#[cfg(feature = "desktop")]
impl HttpTransport {
    pub fn new(config: Config, profile: &Path) -> Result<Self, String> {
        config.validate()?;
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("WegaErrorReporter/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| "Cannot initialize error-report HTTP client".to_owned())?;
        Ok(Self {
            client,
            config,
            identity: Identity::load(profile)
                .map_err(|_| "Cannot load error-report signing identity".to_owned())?,
        })
    }
}
#[cfg(feature = "desktop")]
impl Transport for HttpTransport {
    fn post(&mut self, report: &Report) -> Result<(), String> {
        let body = serde_json::to_vec(report).map_err(|_| "Cannot encode report")?;
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).map_err(|_| "Cannot generate request nonce")?;
        let nonce = crate::error_auth::hex(&nonce);
        let signature = self.identity.sign(&timestamp, &nonce, &body);
        let request = self
            .client
            .post(&self.config.endpoint)
            .header("Content-Type", "application/json")
            .header("Idempotency-Key", &report.id)
            .header("X-Wega-Client", &self.identity.client_id)
            .header("X-Wega-Timestamp", &timestamp)
            .header("X-Wega-Nonce", &nonce)
            .header("X-Wega-Signature", signature)
            .body(body);
        let response = request.send().map_err(|_| "Error-report request failed")?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!("Error-report HTTP {}", response.status().as_u16()))
        }
    }
}
#[cfg(test)]
#[path = "error_reporting_tests.rs"]
mod tests;
