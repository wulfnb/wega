//! Web workers: dedicated, shared and service workers plus the message ports
//! that connect them to documents.

pub(crate) mod runtime;
pub mod service;
pub mod shared;
pub mod transfer;

pub use runtime::{CanvasFrame, CanvasFrames, Kind};
pub use service::{ServiceWorkerHub, ServiceWorkerInfo, ServiceWorkerState};
pub use shared::SharedWorkerHub;
pub use transfer::{ArrayBufferTransfer, MessagePortHub, PortId, PortMessage, SharedPortHub};

use runtime::{Handle, Spec};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkerId(pub u64);

/// `new Worker()` / `new SharedWorker()` arguments after URL resolution.
#[derive(Deserialize)]
pub(crate) struct WorkerConfig {
    pub url: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub module: bool,
    pub port: u64,
    /// Script text the document resolved itself (blob: URLs).
    #[serde(default)]
    pub source: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkerEvent {
    pub kind: String,
    pub payload: String,
}

struct Dedicated {
    handle: Handle,
    events: Arc<Mutex<VecDeque<WorkerEvent>>>,
}

/// The workers one document owns.
pub struct WorkerManager {
    document_url: Url,
    dedicated: HashMap<WorkerId, Dedicated>,
    shared: Arc<Mutex<SharedWorkerHub>>,
    port_hub: SharedPortHub,
    frames: CanvasFrames,
    next_worker_id: u64,
}

/// What a worker inherits from the document that creates it.
pub(crate) struct Environment<'a> {
    pub network: &'a crate::network::Network,
    pub csp: &'a Arc<RwLock<crate::csp::Csp>>,
    pub limits: &'a crate::Limits,
}

impl WorkerManager {
    pub fn with_shared(
        document_url: Url,
        shared: Arc<Mutex<SharedWorkerHub>>,
        port_hub: SharedPortHub,
    ) -> Self {
        Self {
            document_url,
            dedicated: HashMap::new(),
            shared,
            port_hub,
            frames: Default::default(),
            next_worker_id: 0,
        }
    }

    pub fn port_hub(&self) -> SharedPortHub {
        self.port_hub.clone()
    }

    /// Frames committed by workers for this document's placeholder canvases.
    pub fn take_frames(&self) -> Vec<(u64, CanvasFrame)> {
        self.frames
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain()
            .collect()
    }

    /// Worker scripts are same-origin, `data:` or this origin's `blob:` URLs,
    /// and are subject to the document's `worker-src` policy.
    pub(crate) fn check_script(
        &self,
        address: &str,
        csp: &Arc<RwLock<crate::csp::Csp>>,
    ) -> Result<Url, String> {
        let url = Url::parse(address)
            .map_err(|_| format!("SyntaxError: Invalid worker script URL {address}"))?;
        let document = &self.document_url;
        let same_origin = match url.scheme() {
            "data" => true,
            "file" => document.scheme() == "file",
            "http" | "https" | "blob" => url.origin() == document.origin(),
            _ => false,
        };
        if !same_origin {
            return Err(
                "SecurityError: Worker script must be same-origin with the document".into(),
            );
        }
        if !csp
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .allows_url("worker-src", &url, document, None)
        {
            return Err(format!(
                "SecurityError: Refused to create a worker from {url}: Content Security Policy worker-src"
            ));
        }
        Ok(url)
    }

    fn spec(
        &self,
        kind: Kind,
        url: Url,
        config: &WorkerConfig,
        environment: &Environment,
        sink: runtime::Sink,
    ) -> Spec {
        Spec {
            kind,
            name: config.name.clone(),
            script_url: url,
            source: (!config.source.is_empty()).then(|| config.source.clone()),
            module: config.module,
            creator: self.document_url.clone(),
            network: environment.network.clone(),
            csp: environment.csp.clone(),
            limits: environment.limits.clone(),
            port_hub: self.port_hub.clone(),
            port: (kind == Kind::Dedicated).then_some(PortId(config.port)),
            frames: if kind == Kind::Dedicated {
                self.frames.clone()
            } else {
                Default::default()
            },
            scope: String::new(),
            sink,
        }
    }

    pub(crate) fn spawn_dedicated(
        &mut self,
        config: &WorkerConfig,
        environment: &Environment,
    ) -> Result<WorkerId, String> {
        let url = self.check_script(&config.url, environment.csp)?;
        if url.scheme() == "blob" && config.source.is_empty() {
            return Err("NetworkError: The worker script blob is not available".into());
        }
        if self.dedicated.len() >= 32 {
            return Err("QuotaExceededError: Too many workers".into());
        }
        let events: Arc<Mutex<VecDeque<WorkerEvent>>> = Default::default();
        let (queue, wake) = (events.clone(), environment.network.wake.clone());
        let sink: runtime::Sink = Arc::new(move |kind, payload| {
            if kind == "log" {
                return;
            }
            let mut queue = queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if queue.len() < 256 {
                queue.push_back(WorkerEvent {
                    kind: kind.into(),
                    payload: payload.into(),
                });
            }
            drop(queue);
            wake();
        });
        let handle = Handle::spawn(self.spec(Kind::Dedicated, url, config, environment, sink))?;
        self.next_worker_id += 1;
        let id = WorkerId(self.next_worker_id);
        self.dedicated.insert(id, Dedicated { handle, events });
        Ok(id)
    }

    pub fn terminate_dedicated(&mut self, id: WorkerId) -> Result<(), String> {
        let mut worker = self.dedicated.remove(&id).ok_or("Worker not found")?;
        worker.handle.stop();
        Ok(())
    }

    pub fn poll_dedicated_events(&mut self, id: WorkerId) -> Vec<WorkerEvent> {
        self.dedicated
            .get(&id)
            .map(|worker| {
                worker
                    .events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .drain(..)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn dedicated_count(&self) -> usize {
        self.dedicated.len()
    }

    pub(crate) fn connect_shared(
        &mut self,
        config: &WorkerConfig,
        environment: &Environment,
    ) -> Result<(), String> {
        let url = self.check_script(&config.url, environment.csp)?;
        if url.scheme() == "blob" && config.source.is_empty() {
            return Err("NetworkError: The worker script blob is not available".into());
        }
        let origin = self.document_url.origin().ascii_serialization();
        let sink: runtime::Sink = Arc::new(|_, _| {});
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .connect(
                &origin,
                url.as_str(),
                &config.name,
                PortId(config.port),
                || self.spec(Kind::Shared, url.clone(), config, environment, sink),
            )
    }

    pub fn terminate_all(&mut self) {
        for (_, mut worker) in self.dedicated.drain() {
            worker.handle.stop();
        }
    }
}

impl Drop for WorkerManager {
    fn drop(&mut self) {
        self.terminate_all();
    }
}
