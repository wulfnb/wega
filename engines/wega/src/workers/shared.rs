//! Shared workers: one realm per (origin, script URL, name), reachable from
//! every same-origin document through its own message port.

use super::runtime::{Handle, Spec};
use super::transfer::PortId;
use std::collections::HashMap;

#[derive(Default)]
pub struct SharedWorkerHub {
    workers: HashMap<(String, String, String), Handle>,
}

impl SharedWorkerHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Connect `port` to the worker for this key, starting it when needed.
    pub(crate) fn connect(
        &mut self,
        origin: &str,
        script_url: &str,
        name: &str,
        port: PortId,
        spec: impl FnOnce() -> Spec,
    ) -> Result<(), String> {
        let key = (origin.to_owned(), script_url.to_owned(), name.to_owned());
        if self.workers.get(&key).is_none_or(Handle::stopped) {
            if self.workers.len() >= 16 {
                self.workers.retain(|_, worker| !worker.stopped());
            }
            if self.workers.len() >= 16 {
                return Err("QuotaExceededError: Too many shared workers".into());
            }
            self.workers.insert(key.clone(), Handle::spawn(spec())?);
        }
        self.workers[&key].dispatch("connect", format!("{{\"port\":{}}}", port.0))
    }

    pub fn clear(&mut self) {
        self.workers.clear();
    }
}
