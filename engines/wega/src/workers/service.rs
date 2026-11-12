//! Service worker registrations, lifecycle and fetch interception.
//!
//! One hub serves every document that shares a network stack. Each worker
//! script runs in its own realm (`runtime`); the hub drives install and
//! activate, routes fetch events to the active worker and stores activated
//! registrations so they survive a restart.

use super::runtime::{Handle, Kind, Spec};
use crate::network::Network;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, Weak, atomic::AtomicBool, mpsc},
    time::{Duration, Instant},
};
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceWorkerState {
    Parsed,
    Installing,
    Installed,
    Activating,
    Activated,
    Redundant,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceWorkerInfo {
    pub id: u64,
    pub script_url: String,
    pub state: ServiceWorkerState,
}

struct Worker {
    id: u64,
    script_url: String,
    source: String,
    module: bool,
    state: ServiceWorkerState,
    handle: Option<Handle>,
    evaluated: bool,
    skip_waiting: bool,
    has_fetch: bool,
    /// `None` for a worker restored from storage: it predates every document.
    activated: Option<Instant>,
}
impl Worker {
    fn info(&self) -> ServiceWorkerInfo {
        ServiceWorkerInfo {
            id: self.id,
            script_url: self.script_url.clone(),
            state: self.state,
        }
    }
}

struct Registration {
    scope: String,
    origin: String,
    installing: Option<Worker>,
    waiting: Option<Worker>,
    active: Option<Worker>,
    claimed: bool,
}
impl Registration {
    fn worker_mut(&mut self, id: u64) -> Option<&mut Worker> {
        [&mut self.installing, &mut self.waiting, &mut self.active]
            .into_iter()
            .flatten()
            .find(|worker| worker.id == id)
    }
    fn matches(&self, url: &Url) -> bool {
        url.origin().ascii_serialization() == self.origin && url.as_str().starts_with(&self.scope)
    }
}

enum Job {
    Pending {
        origin: String,
        worker: Option<u64>,
    },
    Done {
        origin: String,
        scope: String,
        at: Instant,
    },
    Failed {
        origin: String,
        error: String,
        at: Instant,
    },
}

struct Client {
    url: Url,
    seen: Instant,
    messages: Vec<(u64, String)>,
}

enum Outcome {
    Response(wega_network::Response),
    Fallback,
    Error(String),
}

/// What is written to disk for one activated registration.
#[derive(Serialize, Deserialize)]
struct Stored {
    scope: String,
    script_url: String,
    source: String,
    module: bool,
    claimed: bool,
    has_fetch: bool,
}

#[derive(Default)]
struct Inner {
    registrations: HashMap<String, Registration>,
    jobs: HashMap<u64, Job>,
    fetches: HashMap<u64, mpsc::Sender<Outcome>>,
    clients: HashMap<String, Client>,
    retired: Vec<(String, u64, Instant)>,
    restored: HashSet<String>,
    limits: Option<crate::Limits>,
    next_id: u64,
}
impl Inner {
    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
    /// The registration with the longest scope that contains `url`.
    fn matching(&self, url: &Url) -> Option<&Registration> {
        self.registrations
            .values()
            .filter(|registration| registration.matches(url))
            .max_by_key(|registration| registration.scope.len())
    }
    fn retire(&mut self, origin: &str, mut worker: Worker) {
        worker.state = ServiceWorkerState::Redundant;
        self.retired
            .push((origin.to_owned(), worker.id, Instant::now()));
        reap(worker.handle.take());
    }
    fn live_clients(&self, scope: &str) -> bool {
        self.clients.values().any(|client| {
            client.seen.elapsed() < Duration::from_secs(2) && client.url.as_str().starts_with(scope)
        })
    }
}

/// Worker threads report through the hub's lock; never join one while
/// holding it, or from the worker's own thread.
fn reap(handle: Option<Handle>) {
    if let Some(handle) = handle {
        let _ = std::thread::Builder::new()
            .name("wega-worker-reaper".into())
            .spawn(move || drop(handle));
    }
}

fn origin_of(url: &Url) -> String {
    url.origin().ascii_serialization()
}

pub struct ServiceWorkerHub {
    inner: Mutex<Inner>,
    ports: super::SharedPortHub,
}

impl ServiceWorkerHub {
    pub fn new(ports: super::SharedPortHub) -> Self {
        Self {
            inner: Default::default(),
            ports,
        }
    }
    /// Resource limits for worker realms the hub starts on its own.
    pub(crate) fn configure(&self, limits: &crate::Limits) {
        self.lock().limits.get_or_insert_with(|| limits.clone());
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    // ---- Persistence -------------------------------------------------------
    /// Read an origin's stored registrations. `None` means storage could not
    /// be consulted yet (for example before the broker grants the origin).
    fn load(network: &Network, origin: &str) -> Option<Vec<u8>> {
        if let Some(remote) = &network.remote {
            return remote
                .resource(
                    wega_network::ResourceRequest::ServiceWorkers {
                        origin: origin.to_owned(),
                        write: false,
                    },
                    &[],
                )
                .ok();
        }
        let Some(root) = &network.storage_dir else {
            return Some(Vec::new());
        };
        Some(std::fs::read(wega_network::service_worker_store(root, origin)).unwrap_or_default())
    }
    fn save(network: &Network, origin: &str, bytes: &[u8]) {
        if let Some(remote) = &network.remote {
            let _ = remote.resource(
                wega_network::ResourceRequest::ServiceWorkers {
                    origin: origin.to_owned(),
                    write: true,
                },
                bytes,
            );
            return;
        }
        let Some(root) = &network.storage_dir else {
            return;
        };
        let path = wega_network::service_worker_store(root, origin);
        if bytes.is_empty() {
            let _ = std::fs::remove_file(path);
            return;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Replace atomically so a crash never leaves a partial registry.
        let temporary = path.with_extension("json.tmp");
        if std::fs::write(&temporary, bytes).is_ok() {
            let _ = std::fs::rename(temporary, path);
        }
    }
    fn persist(&self, network: &Network, origin: &str) {
        let stored: Vec<Stored> = self
            .lock()
            .registrations
            .values()
            .filter(|registration| registration.origin == origin)
            .filter_map(|registration| {
                let worker = registration.active.as_ref()?;
                (worker.state == ServiceWorkerState::Activated).then(|| Stored {
                    scope: registration.scope.clone(),
                    script_url: worker.script_url.clone(),
                    source: worker.source.clone(),
                    module: worker.module,
                    claimed: registration.claimed,
                    has_fetch: worker.has_fetch,
                })
            })
            .collect();
        let bytes = if stored.is_empty() {
            Vec::new()
        } else {
            match serde_json::to_vec(&stored) {
                Ok(bytes) if bytes.len() <= wega_network::MAX_BODY => bytes,
                _ => return,
            }
        };
        Self::save(network, origin, &bytes);
    }
    /// Load an origin's stored registrations the first time it is seen.
    fn restore(&self, network: &Network, origin: &str) {
        if self.lock().restored.contains(origin) {
            return;
        }
        // Read without the lock: a brokered read is a blocking round trip.
        let Some(bytes) = Self::load(network, origin) else {
            return;
        };
        let mut inner = self.lock();
        if !inner.restored.insert(origin.to_owned()) {
            return;
        }
        let Ok(stored) = serde_json::from_slice::<Vec<Stored>>(&bytes) else {
            return;
        };
        for entry in stored {
            let valid = Url::parse(&entry.scope).is_ok_and(|scope| origin_of(&scope) == origin)
                && Url::parse(&entry.script_url).is_ok_and(|script| origin_of(&script) == origin);
            if !valid || inner.registrations.contains_key(&entry.scope) {
                continue;
            }
            let id = inner.id();
            inner.registrations.insert(
                entry.scope.clone(),
                Registration {
                    scope: entry.scope,
                    origin: origin.to_owned(),
                    installing: None,
                    waiting: None,
                    active: Some(Worker {
                        id,
                        script_url: entry.script_url,
                        source: entry.source,
                        module: entry.module,
                        state: ServiceWorkerState::Activated,
                        handle: None,
                        evaluated: false,
                        skip_waiting: false,
                        has_fetch: entry.has_fetch,
                        activated: None,
                    }),
                    claimed: entry.claimed,
                },
            );
        }
    }

    // ---- Worker realms -----------------------------------------------------
    fn start(
        self: &Arc<Self>,
        network: &Network,
        limits: &crate::Limits,
        scope: &str,
        worker: &Worker,
    ) -> Result<Handle, String> {
        let script_url = Url::parse(&worker.script_url).map_err(|error| error.to_string())?;
        let creator = Url::parse(scope).map_err(|error| error.to_string())?;
        let hub: Weak<Self> = Arc::downgrade(self);
        let (net, scope_key, id) = (network.clone(), scope.to_owned(), worker.id);
        Handle::spawn(Spec {
            kind: Kind::Service,
            name: String::new(),
            script_url,
            source: Some(worker.source.clone()),
            module: worker.module,
            creator,
            network: network.clone(),
            csp: Default::default(),
            limits: limits.clone(),
            port_hub: self.ports.clone(),
            service: self.clone(),
            port: None,
            frames: Default::default(),
            scope: scope.to_owned(),
            sink: Arc::new(move |kind, payload| {
                if let Some(hub) = hub.upgrade() {
                    hub.on_event(&net, &scope_key, id, kind, payload);
                }
            }),
        })
    }
    /// Start the active worker again after a restart or an idle stop.
    fn ensure_running(self: &Arc<Self>, network: &Network, scope: &str) {
        let mut inner = self.lock();
        let limits = inner.limits.clone().unwrap_or_default();
        let Some(worker) = inner
            .registrations
            .get_mut(scope)
            .and_then(|registration| registration.active.as_mut())
        else {
            return;
        };
        if worker
            .handle
            .as_ref()
            .is_some_and(|handle| !handle.stopped())
        {
            return;
        }
        reap(worker.handle.take());
        worker.evaluated = false;
        if let Ok(handle) = self.start(network, &limits, scope, worker) {
            worker.handle = Some(handle);
        }
    }

    fn finish_jobs(inner: &mut Inner, worker: u64, result: Result<String, String>) {
        for job in inner.jobs.values_mut() {
            if let Job::Pending {
                origin,
                worker: Some(id),
            } = job
                && *id == worker
            {
                let origin = std::mem::take(origin);
                *job = match &result {
                    Ok(scope) => Job::Done {
                        origin,
                        scope: scope.clone(),
                        at: Instant::now(),
                    },
                    Err(error) => Job::Failed {
                        origin,
                        error: error.clone(),
                        at: Instant::now(),
                    },
                };
            }
        }
    }

    /// Promote the waiting worker when nothing still depends on the old one.
    fn try_activate(inner: &mut Inner, scope: &str) {
        let busy = inner.live_clients(scope);
        let Some(registration) = inner.registrations.get_mut(scope) else {
            return;
        };
        let ready = registration.waiting.as_ref().is_some_and(|waiting| {
            waiting.state == ServiceWorkerState::Installed
                && (registration.active.is_none() || waiting.skip_waiting || !busy)
        });
        if !ready {
            return;
        }
        let origin = registration.origin.clone();
        let previous = registration.active.take();
        let mut next = registration.waiting.take().expect("checked waiting worker");
        next.state = ServiceWorkerState::Activating;
        if let Some(handle) = &next.handle {
            let _ = handle.dispatch("activate", "null".into());
        }
        registration.active = Some(next);
        if let Some(previous) = previous {
            inner.retire(&origin, previous);
        }
    }

    fn on_event(
        self: &Arc<Self>,
        network: &Network,
        scope: &str,
        id: u64,
        kind: &str,
        payload: &str,
    ) {
        let detail: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
        let mut persist = None;
        let mut follow_up = None;
        {
            let mut inner = self.lock();
            match kind {
                "swFetch" => {
                    let Some(reply) = detail["id"]
                        .as_u64()
                        .and_then(|id| inner.fetches.remove(&id))
                    else {
                        return;
                    };
                    let outcome = match detail["kind"].as_str() {
                        Some("response") => {
                            use base64::Engine;
                            match base64::engine::general_purpose::STANDARD
                                .decode(detail["body"].as_str().unwrap_or(""))
                            {
                                Ok(body) => Outcome::Response(wega_network::Response {
                                    url: detail["url"].as_str().unwrap_or("").to_owned(),
                                    status: detail["status"].as_u64().unwrap_or(200) as u16,
                                    headers: serde_json::from_value(detail["headers"].clone())
                                        .unwrap_or_default(),
                                    redirected: detail["redirected"].as_bool().unwrap_or(false),
                                    body,
                                }),
                                Err(_) => Outcome::Error("Invalid service worker response".into()),
                            }
                        }
                        Some("error") => Outcome::Error(format!(
                            "Service worker fetch handler failed: {}",
                            detail["message"].as_str().unwrap_or("")
                        )),
                        _ => Outcome::Fallback,
                    };
                    let _ = reply.send(outcome);
                    return;
                }
                "swClientMessage" => {
                    if let (Some(client), Some(data)) =
                        (detail["client"].as_str(), detail["data"].as_str())
                        && let Some(client) = inner.clients.get_mut(client)
                        && client.messages.len() < 256
                    {
                        client.messages.push((id, data.to_owned()));
                    }
                    return;
                }
                "swUpdate" => follow_up = Some("update"),
                "swUnregister" => follow_up = Some("unregister"),
                _ => {}
            }
            let Some(registration) = inner.registrations.get_mut(scope) else {
                return;
            };
            let origin = registration.origin.clone();
            match kind {
                "swEvaluated" => {
                    let Some(worker) = registration.worker_mut(id) else {
                        return;
                    };
                    worker.evaluated = true;
                    worker.has_fetch = detail["fetch"].as_bool().unwrap_or(false);
                    if worker.state == ServiceWorkerState::Installing
                        && let Some(handle) = &worker.handle
                    {
                        let _ = handle.dispatch("install", "null".into());
                    }
                    Self::finish_jobs(&mut inner, id, Ok(scope.to_owned()));
                }
                "swInstalled" => {
                    let installed = registration
                        .installing
                        .as_ref()
                        .is_some_and(|worker| worker.id == id);
                    if !installed {
                        return;
                    }
                    let mut worker = registration.installing.take().expect("checked worker");
                    if detail["ok"].as_bool().unwrap_or(false) {
                        worker.state = ServiceWorkerState::Installed;
                        let replaced = registration.waiting.replace(worker);
                        if let Some(replaced) = replaced {
                            inner.retire(&origin, replaced);
                        }
                        Self::try_activate(&mut inner, scope);
                    } else {
                        let empty = registration.active.is_none() && registration.waiting.is_none();
                        inner.retire(&origin, worker);
                        if empty {
                            inner.registrations.remove(scope);
                        }
                    }
                }
                "swActivated" => {
                    if let Some(worker) = registration
                        .active
                        .as_mut()
                        .filter(|worker| worker.id == id)
                    {
                        worker.state = ServiceWorkerState::Activated;
                        worker.activated = Some(Instant::now());
                        persist = Some(origin);
                    }
                }
                "swSkipWaiting" => {
                    if let Some(worker) = registration.worker_mut(id) {
                        worker.skip_waiting = true;
                    }
                    Self::try_activate(&mut inner, scope);
                }
                "swClaim" => {
                    if registration
                        .active
                        .as_ref()
                        .is_some_and(|worker| worker.id == id)
                    {
                        registration.claimed = true;
                        persist = Some(origin);
                    }
                }
                "error" => {
                    // A script that fails to load or evaluate never installs.
                    let failed = registration
                        .installing
                        .as_ref()
                        .is_some_and(|worker| worker.id == id && !worker.evaluated);
                    if failed {
                        let worker = registration.installing.take().expect("checked worker");
                        let empty = registration.active.is_none() && registration.waiting.is_none();
                        inner.retire(&origin, worker);
                        if empty {
                            inner.registrations.remove(scope);
                        }
                        let message = detail["message"].as_str().unwrap_or("script failed");
                        let error = if message.contains("Error:") {
                            message.to_owned()
                        } else {
                            format!("TypeError: Service worker script failed: {message}")
                        };
                        Self::finish_jobs(&mut inner, id, Err(error));
                    }
                }
                "closed" => {
                    if let Some(worker) = registration.worker_mut(id) {
                        reap(worker.handle.take());
                    }
                }
                _ => {}
            }
        }
        if let Some(origin) = persist {
            self.persist(network, &origin);
        }
        match follow_up {
            Some("update") => {
                let _ = self.update(network, scope);
            }
            Some("unregister") => {
                if let Ok(url) = Url::parse(scope) {
                    self.unregister(network, &url, scope);
                }
            }
            _ => {}
        }
    }

    // ---- Document API --------------------------------------------------------
    /// Begin registering `script` for `scope`; the returned job settles the
    /// page's promise once the script has been fetched and evaluated.
    pub(crate) fn register(
        self: &Arc<Self>,
        network: &Network,
        limits: &crate::Limits,
        document: &Url,
        script: &str,
        scope: &str,
        module: bool,
    ) -> Result<u64, String> {
        if !crate::security_context::trustworthy(document)
            || !matches!(document.scheme(), "http" | "https")
        {
            return Err("SecurityError: Service workers require a secure context".into());
        }
        let script_url = Url::parse(script)
            .map_err(|_| "TypeError: Invalid service worker script URL".to_string())?;
        if !matches!(script_url.scheme(), "http" | "https") {
            return Err("TypeError: Service worker scripts must be fetched over HTTP(S)".into());
        }
        if script_url.origin() != document.origin() {
            return Err("SecurityError: Service worker script must be same-origin".into());
        }
        let lower = script_url.path().to_ascii_lowercase();
        if lower.contains("%2f") || lower.contains("%5c") {
            return Err(
                "TypeError: Service worker script URL contains an encoded separator".into(),
            );
        }
        let scope_url = if scope.is_empty() {
            script_url.join("./").map_err(|error| error.to_string())?
        } else {
            Url::parse(scope).map_err(|_| "TypeError: Invalid service worker scope".to_string())?
        };
        if scope_url.origin() != document.origin() {
            return Err("SecurityError: Service worker scope must be same-origin".into());
        }
        let origin = origin_of(document);
        self.restore(network, &origin);
        let job = {
            let mut inner = self.lock();
            inner.limits.get_or_insert_with(|| limits.clone());
            if inner
                .registrations
                .values()
                .filter(|registration| registration.origin == origin)
                .count()
                >= 32
                && !inner.registrations.contains_key(scope_url.as_str())
            {
                return Err("QuotaExceededError: Too many service worker registrations".into());
            }
            let job = inner.id();
            inner.jobs.insert(
                job,
                Job::Pending {
                    origin: origin.clone(),
                    worker: None,
                },
            );
            job
        };
        let (hub, net, document) = (self.clone(), network.clone(), document.clone());
        std::thread::Builder::new()
            .name("wega-service-worker-job".into())
            .spawn(move || {
                if let Err(error) = hub.install(&net, &document, script_url, scope_url, module, job)
                {
                    let mut inner = hub.lock();
                    let origin = origin_of(&document);
                    inner.jobs.insert(
                        job,
                        Job::Failed {
                            origin,
                            error,
                            at: Instant::now(),
                        },
                    );
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(job)
    }

    /// Fetch the script, then install it unless it is byte-identical to the
    /// registration's current worker.
    fn install(
        self: &Arc<Self>,
        network: &Network,
        document: &Url,
        script_url: Url,
        scope_url: Url,
        module: bool,
        job: u64,
    ) -> Result<(), String> {
        let limit = self
            .lock()
            .limits
            .as_ref()
            .map_or(8 * 1024 * 1024, |limits| limits.resource_bytes);
        let script = super::runtime::fetch_script(
            network,
            &script_url,
            document,
            limit,
            &AtomicBool::new(false),
            true,
        )
        .map_err(|error| {
            if error.starts_with("SecurityError") {
                error
            } else {
                format!(
                    "TypeError: Failed to fetch the service worker script: {}",
                    error.trim_start_matches("NetworkError: ")
                )
            }
        })?;
        if script.url != script_url {
            return Err("SecurityError: Service worker script must not redirect".into());
        }
        // The widest scope a script may claim is its own directory, unless the
        // server widens it with Service-Worker-Allowed.
        let allowed = script
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("service-worker-allowed"))
            .and_then(|(_, value)| script_url.join(value).ok())
            .filter(|allowed| allowed.origin() == script_url.origin())
            .or_else(|| script_url.join("./").ok())
            .ok_or("SecurityError: Invalid service worker scope")?;
        if !scope_url.path().starts_with(allowed.path()) {
            return Err(format!(
                "SecurityError: Scope {scope_url} is outside the script's allowed scope {allowed}"
            ));
        }
        let scope = scope_url.to_string();
        let origin = origin_of(document);
        let mut inner = self.lock();
        let limits = inner.limits.clone().unwrap_or_default();
        let unchanged = inner.registrations.get(&scope).is_some_and(|registration| {
            [
                &registration.active,
                &registration.waiting,
                &registration.installing,
            ]
            .into_iter()
            .flatten()
            .any(|worker| {
                worker.script_url == script_url.as_str() && worker.source == script.source
            })
        });
        if unchanged {
            inner.jobs.insert(
                job,
                Job::Done {
                    origin,
                    scope,
                    at: Instant::now(),
                },
            );
            return Ok(());
        }
        let id = inner.id();
        let mut worker = Worker {
            id,
            script_url: script_url.to_string(),
            source: script.source,
            module,
            state: ServiceWorkerState::Installing,
            handle: None,
            evaluated: false,
            skip_waiting: false,
            has_fetch: false,
            activated: None,
        };
        worker.handle = Some(self.start(network, &limits, &scope, &worker)?);
        inner.jobs.insert(
            job,
            Job::Pending {
                origin: origin.clone(),
                worker: Some(id),
            },
        );
        let registration =
            inner
                .registrations
                .entry(scope.clone())
                .or_insert_with(|| Registration {
                    scope: scope.clone(),
                    origin: origin.clone(),
                    installing: None,
                    waiting: None,
                    active: None,
                    claimed: false,
                });
        let replaced = registration.installing.replace(worker);
        if let Some(replaced) = replaced {
            inner.retire(&origin, replaced);
        }
        Ok(())
    }

    /// Check the registered script for a new version.
    pub(crate) fn update(self: &Arc<Self>, network: &Network, scope: &str) -> Result<u64, String> {
        let (script, module, limits) = {
            let inner = self.lock();
            let registration = inner
                .registrations
                .get(scope)
                .ok_or("InvalidStateError: The registration no longer exists")?;
            let worker = [
                &registration.active,
                &registration.waiting,
                &registration.installing,
            ]
            .into_iter()
            .flatten()
            .next()
            .ok_or("InvalidStateError: The registration has no worker")?;
            (
                worker.script_url.clone(),
                worker.module,
                inner.limits.clone().unwrap_or_default(),
            )
        };
        let document = Url::parse(scope).map_err(|error| error.to_string())?;
        self.register(network, &limits, &document, &script, scope, module)
    }

    pub(crate) fn unregister(&self, network: &Network, document: &Url, scope: &str) -> bool {
        let origin = origin_of(document);
        let removed = {
            let mut inner = self.lock();
            let owned = inner
                .registrations
                .get(scope)
                .is_some_and(|registration| registration.origin == origin);
            if !owned {
                return false;
            }
            let registration = inner.registrations.remove(scope).expect("checked scope");
            for worker in [
                registration.installing,
                registration.waiting,
                registration.active,
            ]
            .into_iter()
            .flatten()
            {
                inner.retire(&origin, worker);
            }
            true
        };
        self.persist(network, &origin);
        removed
    }

    fn controller<'a>(inner: &'a Inner, url: &Url, since: Instant) -> Option<&'a Registration> {
        inner.matching(url).filter(|registration| {
            registration.active.as_ref().is_some_and(|worker| {
                worker.state == ServiceWorkerState::Activated
                    && (registration.claimed
                        || worker.activated.is_none_or(|activated| activated <= since))
            })
        })
    }

    /// Whether a document at `url` is controlled by an active worker.
    pub(crate) fn controls(&self, network: &Network, url: &Url) -> bool {
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        self.restore(network, &origin_of(url));
        Self::controller(&self.lock(), url, network.navigated()).is_some()
    }

    /// One document's view of its origin: registrations, finished jobs,
    /// its controller and messages addressed to it.
    pub(crate) fn poll(
        &self,
        network: &Network,
        client: &str,
        document: &Url,
        since: Instant,
    ) -> Value {
        let origin = origin_of(document);
        self.restore(network, &origin);
        let mut inner = self.lock();
        let now = Instant::now();
        inner
            .clients
            .retain(|_, entry| now.duration_since(entry.seen) < Duration::from_secs(10));
        inner
            .retired
            .retain(|(_, _, at)| now.duration_since(*at) < Duration::from_secs(10));
        inner.jobs.retain(|_, job| match job {
            Job::Done { at, .. } | Job::Failed { at, .. } => {
                now.duration_since(*at) < Duration::from_secs(30)
            }
            Job::Pending { .. } => true,
        });
        let entry = inner
            .clients
            .entry(client.to_owned())
            .or_insert_with(|| Client {
                url: document.clone(),
                seen: now,
                messages: Vec::new(),
            });
        entry.seen = now;
        entry.url = document.clone();
        let messages: Vec<Value> = std::mem::take(&mut entry.messages)
            .into_iter()
            .map(|(worker, data)| json!({ "worker": worker, "data": data }))
            .collect();
        let registrations: Vec<Value> = inner
            .registrations
            .values()
            .filter(|registration| registration.origin == origin)
            .map(|registration| {
                json!({
                    "scope": registration.scope,
                    "installing": registration.installing.as_ref().map(Worker::info),
                    "waiting": registration.waiting.as_ref().map(Worker::info),
                    "active": registration.active.as_ref().map(Worker::info),
                })
            })
            .collect();
        let jobs: serde_json::Map<String, Value> = inner
            .jobs
            .iter()
            .filter_map(|(id, job)| match job {
                Job::Done {
                    origin: owner,
                    scope,
                    ..
                } if *owner == origin => Some((id.to_string(), json!({ "scope": scope }))),
                Job::Failed {
                    origin: owner,
                    error,
                    ..
                } if *owner == origin => Some((id.to_string(), json!({ "error": error }))),
                _ => None,
            })
            .collect();
        json!({
            "registrations": registrations,
            "retired": inner.retired.iter()
                .filter(|(owner, _, _)| *owner == origin)
                .map(|(_, id, _)| json!({ "id": id }))
                .collect::<Vec<_>>(),
            "controller": Self::controller(&inner, document, since)
                .and_then(|registration| registration.active.as_ref())
                .map(|worker| worker.id),
            "ready": inner.matching(document)
                .filter(|registration| registration.active.is_some())
                .map(|registration| registration.scope.clone()),
            "jobs": jobs,
            "messages": messages,
        })
    }

    /// `ServiceWorker.postMessage` from a document.
    pub(crate) fn post(
        self: &Arc<Self>,
        network: &Network,
        worker: u64,
        client: &str,
        document: &Url,
        data: &str,
    ) -> Result<(), String> {
        let origin = origin_of(document);
        let scope = self
            .lock()
            .registrations
            .values()
            .find(|registration| {
                registration.origin == origin
                    && [
                        &registration.active,
                        &registration.waiting,
                        &registration.installing,
                    ]
                    .into_iter()
                    .flatten()
                    .any(|candidate| candidate.id == worker)
            })
            .map(|registration| registration.scope.clone())
            .ok_or("InvalidStateError: The service worker is no longer available")?;
        self.ensure_running(network, &scope);
        let mut inner = self.lock();
        let target = inner
            .registrations
            .get_mut(&scope)
            .and_then(|registration| registration.worker_mut(worker))
            .and_then(|worker| worker.handle.as_ref())
            .ok_or("InvalidStateError: The service worker is not running")?;
        target.dispatch(
            "message",
            json!({ "data": data, "client": client, "url": document.as_str() }).to_string(),
        )
    }

    /// Window clients of an origin, for `clients.matchAll()`.
    pub(crate) fn clients(&self, origin: &str) -> Value {
        let inner = self.lock();
        Value::Array(
            inner
                .clients
                .iter()
                .filter(|(_, client)| {
                    origin_of(&client.url) == origin
                        && client.seen.elapsed() < Duration::from_secs(2)
                })
                .map(|(id, client)| json!({ "id": id, "url": client.url.as_str() }))
                .collect(),
        )
    }

    /// Offer a request to the controlling worker's fetch handlers. `None`
    /// means the request continues to the network.
    pub(crate) fn intercept(
        self: &Arc<Self>,
        network: &Network,
        client: Option<&Url>,
        request: &wega_network::Request,
    ) -> Option<Result<wega_network::Response, String>> {
        let url = Url::parse(&request.url).ok()?;
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }
        let navigation = request.destination == wega_network::ResourceDestination::Document;
        let subject = if navigation { &url } else { client? };
        if !matches!(subject.scheme(), "http" | "https") {
            return None;
        }
        self.restore(network, &origin_of(subject));
        let since = if navigation {
            Instant::now()
        } else {
            network.navigated()
        };
        let scope = {
            let inner = self.lock();
            let registration = Self::controller(&inner, subject, since)?;
            if !registration.active.as_ref()?.has_fetch {
                return None;
            }
            registration.scope.clone()
        };
        self.ensure_running(network, &scope);
        let (sender, receiver) = mpsc::channel();
        let id = {
            let mut inner = self.lock();
            let id = inner.id();
            let handle = inner
                .registrations
                .get(&scope)?
                .active
                .as_ref()?
                .handle
                .as_ref()?;
            use base64::Engine;
            handle
                .dispatch(
                    "fetch",
                    json!({
                        "id": id,
                        "url": request.url,
                        "method": request.method,
                        "headers": request.headers,
                        "body": base64::engine::general_purpose::STANDARD.encode(&request.body),
                        "client": "",
                    })
                    .to_string(),
                )
                .ok()?;
            inner.fetches.insert(id, sender);
            id
        };
        match receiver.recv_timeout(Duration::from_secs(30)) {
            Ok(Outcome::Response(response)) => Some(Ok(response)),
            Ok(Outcome::Fallback) => None,
            Ok(Outcome::Error(error)) => Some(Err(error)),
            Err(_) => {
                self.lock().fetches.remove(&id);
                Some(Err("Service worker did not answer the fetch event".into()))
            }
        }
    }
}

impl crate::network::FetchInterceptor for Arc<ServiceWorkerHub> {
    fn intercept(
        &self,
        network: &Network,
        client: Option<&Url>,
        request: &wega_network::Request,
    ) -> Option<Result<wega_network::Response, String>> {
        ServiceWorkerHub::intercept(self, network, client, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_matching_prefers_the_longest_scope_within_an_origin() {
        let mut inner = Inner::default();
        for scope in ["https://app.test/", "https://app.test/app/"] {
            inner.registrations.insert(
                scope.into(),
                Registration {
                    scope: scope.into(),
                    origin: "https://app.test".into(),
                    installing: None,
                    waiting: None,
                    active: None,
                    claimed: false,
                },
            );
        }
        let find = |address: &str| {
            inner
                .matching(&Url::parse(address).unwrap())
                .map(|registration| registration.scope.clone())
        };
        assert_eq!(
            find("https://app.test/app/page").as_deref(),
            Some("https://app.test/app/")
        );
        assert_eq!(
            find("https://app.test/other").as_deref(),
            Some("https://app.test/")
        );
        assert_eq!(find("https://evil.test/app/page"), None);
    }
}
