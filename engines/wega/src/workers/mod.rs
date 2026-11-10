//! Web Workers architecture: DedicatedWorker, SharedWorker, ServiceWorker, and transferable ports.

pub mod dedicated;
pub mod service;
pub mod shared;
pub mod transfer;

pub use dedicated::{DedicatedWorker, WorkerEvent, WorkerId};
pub use service::{ServiceWorkerInfo, ServiceWorkerManager, ServiceWorkerState};
pub use shared::{SharedWorkerHub, SharedWorkerInstance};
pub use transfer::{ArrayBufferTransfer, MessagePortHub, PortId, PortMessage, SharedPortHub};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use url::Url;
use wega_network::CacheEntry;

pub struct WorkerManager {
    document_url: Url,
    storage_dir: Option<PathBuf>,
    dedicated: HashMap<WorkerId, DedicatedWorker>,
    shared: Arc<Mutex<SharedWorkerHub>>,
    service: ServiceWorkerManager,
    port_hub: SharedPortHub,
    next_worker_id: u64,
}

impl WorkerManager {
    pub fn with_shared(
        document_url: Url,
        storage_dir: Option<PathBuf>,
        shared: Arc<Mutex<SharedWorkerHub>>,
        port_hub: SharedPortHub,
    ) -> Self {
        Self {
            document_url,
            storage_dir,
            dedicated: HashMap::new(),
            shared,
            service: ServiceWorkerManager::new(),
            port_hub,
            next_worker_id: 0,
        }
    }

    pub fn new(document_url: Url, storage_dir: Option<PathBuf>) -> Self {
        Self::with_shared(
            document_url,
            storage_dir,
            Arc::new(Mutex::new(SharedWorkerHub::new())),
            Arc::new(Mutex::new(MessagePortHub::new())),
        )
    }

    pub fn port_hub(&self) -> SharedPortHub {
        self.port_hub.clone()
    }

    pub fn spawn_dedicated(
        &mut self,
        script_url: &str,
        script_content: String,
    ) -> Result<WorkerId, String> {
        let content = if !script_content.is_empty() {
            script_content
        } else if script_url.starts_with("data:") {
            if let Ok(data_url) = data_url::DataUrl::process(script_url) {
                if let Ok((bytes, _)) = data_url.decode_to_vec() {
                    String::from_utf8(bytes).unwrap_or_default()
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        if !script_url.starts_with("data:") {
            let script = self
                .document_url
                .join(script_url)
                .map_err(|e| format!("Invalid script URL: {e}"))?;

            if script.origin() != self.document_url.origin() {
                return Err(
                    "SecurityError: Worker script must be same-origin with the document".into(),
                );
            }
        }

        self.next_worker_id += 1;
        let id = WorkerId(self.next_worker_id);
        let worker = DedicatedWorker::spawn(id, content)?;
        self.dedicated.insert(id, worker);
        Ok(id)
    }

    pub fn post_dedicated(&self, id: WorkerId, data: String) -> Result<(), String> {
        let worker = self.dedicated.get(&id).ok_or("Worker not found")?;
        worker.post_message(data)
    }

    pub fn terminate_dedicated(&mut self, id: WorkerId) -> Result<(), String> {
        let mut worker = self.dedicated.remove(&id).ok_or("Worker not found")?;
        worker.terminate();
        Ok(())
    }

    pub fn poll_dedicated_events(&mut self, id: WorkerId) -> Vec<WorkerEvent> {
        if let Some(worker) = self.dedicated.get(&id) {
            worker.poll_events()
        } else {
            Vec::new()
        }
    }

    pub fn connect_shared(
        &mut self,
        script_url: &str,
        name: &str,
        script_content: String,
    ) -> Result<(PortId, PortId), String> {
        let content = if !script_content.is_empty() {
            script_content
        } else if script_url.starts_with("data:") {
            if let Ok(data_url) = data_url::DataUrl::process(script_url) {
                if let Ok((bytes, _)) = data_url.decode_to_vec() {
                    String::from_utf8(bytes).unwrap_or_default()
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let script_full = if !script_url.starts_with("data:") {
            let script = self
                .document_url
                .join(script_url)
                .map_err(|e| format!("Invalid script URL: {e}"))?;

            if script.origin() != self.document_url.origin() {
                return Err(
                    "SecurityError: SharedWorker script must be same-origin with the document"
                        .into(),
                );
            }
            script.to_string()
        } else {
            script_url.to_string()
        };

        let origin = self.document_url.origin().ascii_serialization();
        let (port_doc, port_worker) = self
            .port_hub
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .create_channel();

        let instance = self
            .shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_or_spawn(&origin, name, &script_full, content, self.port_hub.clone())?;

        instance
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .connect(port_worker)?;

        Ok((port_doc, port_worker))
    }

    pub fn register_service_worker(
        &mut self,
        script_url: &str,
        scope: Option<&str>,
    ) -> Result<ServiceWorkerInfo, String> {
        self.service.register(
            &self.document_url,
            script_url,
            scope,
            self.storage_dir.clone(),
        )
    }

    pub fn get_service_worker(&self, url: &Url) -> Option<ServiceWorkerInfo> {
        self.service.get_registration(url)
    }

    pub fn unregister_service_worker(&mut self, scope: &str) -> bool {
        self.service.unregister(scope)
    }

    pub fn intercept_fetch(&self, url: &Url) -> Option<CacheEntry> {
        self.service.intercept_fetch(url)
    }

    pub fn create_channel(&mut self) -> (PortId, PortId) {
        self.port_hub
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .create_channel()
    }

    pub fn post_port(
        &mut self,
        source: PortId,
        data: String,
        transferred_ports: Vec<PortId>,
    ) -> Result<(), String> {
        self.port_hub
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .post_message(source, data, transferred_ports)
    }

    pub fn close_port(&mut self, port: PortId) {
        self.port_hub
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .close(port);
    }

    pub fn poll_port(&mut self, port: PortId) -> Vec<PortMessage> {
        self.port_hub
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .poll_messages(port)
    }

    pub fn terminate_all(&mut self) {
        for (_, mut worker) in self.dedicated.drain() {
            worker.terminate();
        }
    }
}

impl Drop for WorkerManager {
    fn drop(&mut self) {
        self.terminate_all();
    }
}
