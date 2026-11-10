//! SharedWorker execution and cross-document port connection hub.

use super::transfer::{PortId, SharedPortHub};
use deno_core::{JsRuntime, RuntimeOptions};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, SyncSender},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct SharedWorkerInstance {
    pub name: String,
    pub script_url: String,
    to_worker: SyncSender<PortId>,
    terminated: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SharedWorkerInstance {
    pub fn spawn(
        name: String,
        script_url: String,
        script_content: String,
        port_hub: SharedPortHub,
    ) -> Result<Self, String> {
        let (to_worker_tx, to_worker_rx) = mpsc::sync_channel::<PortId>(32);
        let terminated = Arc::new(AtomicBool::new(false));
        let term_clone = terminated.clone();
        let port_hub_clone = port_hub;

        let thread = thread::Builder::new()
            .name(format!("wega-shared-worker-{}", name))
            .spawn(move || {
                let mut runtime = match JsRuntime::try_new(RuntimeOptions::default()) {
                    Ok(rt) => rt,
                    Err(_) => return,
                };

                let mut connected_ports = Vec::new();
                let bootstrap_js = r#"
                    globalThis.self = globalThis;
                    globalThis.__port_map = {};
                    globalThis.__port_outbox = [];
                    globalThis.onconnect = null;
                "#;
                let _ = runtime.execute_script("bootstrap", bootstrap_js);
                if let Err(e) = runtime.execute_script("shared_script", script_content) {
        eprintln!("SHARED_SCRIPT ERROR: {e}");
    }

                while !term_clone.load(Ordering::Acquire) {
                    match to_worker_rx.recv_timeout(Duration::from_millis(5)) {
                        Ok(connected_port) => {
                            connected_ports.push(connected_port);
                            let dispatch_js = format!(
                                "(() => {{
                                     const p = {{
                                         id: {},
                                         onmessage: null,
                                         postMessage: function(d) {{
                                             globalThis.__port_outbox.push({{ id: this.id, data: JSON.stringify(d) }});
                                         }}
                                     }};
                                     globalThis.__port_map[p.id] = p;
                                     if (typeof onconnect === 'function') {{
                                         onconnect({{ ports: [p] }});
                                     }}
                                 }})()",
                                connected_port.0
                            );
                            if let Err(e) = runtime.execute_script("dispatch_connect", dispatch_js) {
        eprintln!("DISPATCH_CONNECT ERROR: {e}");
    }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }

                    // 1. Drain port outbox
                    let drain_js = "JSON.stringify(globalThis.__port_outbox.splice(0, globalThis.__port_outbox.length))";
                    if let Ok(val) = runtime.execute_script("drain_outbox", drain_js) {
                        deno_core::scope!(scope, runtime);
                                                let local = deno_core::v8::Local::new(scope, val);
                        let json_str = local.to_rust_string_lossy(scope); {
                            #[derive(serde::Deserialize)]
                            struct OutMsg {
                                id: u64,
                                data: String,
                            }
                            if let Ok(messages) = serde_json::from_str::<Vec<OutMsg>>(&json_str) {
                                let mut hub = port_hub_clone
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                for msg in messages {
                                    let _ = hub.post_message(PortId(msg.id), msg.data, vec![]);
                                }
                            }
                        }
                    }

                    // 2. Poll incoming messages for all connected ports
                    for &port_id in &connected_ports {
                        let messages = {
                            let mut hub = port_hub_clone
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            hub.poll_messages(port_id)
                        };
                        for msg in messages {
                            let dispatch_js = format!(
                                "(() => {{
                                     const p = globalThis.__port_map[{}];
                                     if (p && typeof p.onmessage === 'function') {{
                                         p.onmessage({{ data: {} }});
                                     }}
                                 }})()",
                                port_id.0, msg.data
                            );
                            if let Err(e) = runtime.execute_script("dispatch_port_message", dispatch_js) {
        eprintln!("DISPATCH_PORT_MESSAGE ERROR: {e}");
    }
                        }
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn shared worker thread: {e}"))?;

        Ok(Self {
            name,
            script_url,
            to_worker: to_worker_tx,
            terminated,
            thread: Some(thread),
        })
    }

    pub fn connect(&self, port: PortId) -> Result<(), String> {
        if self.terminated.load(Ordering::Acquire) {
            return Err("SharedWorker is terminated".into());
        }
        self.to_worker.send(port).map_err(|e| e.to_string())
    }

    pub fn terminate(&mut self) {
        if !self.terminated.swap(true, Ordering::Release) {
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

impl Drop for SharedWorkerInstance {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[derive(Default)]
pub struct SharedWorkerHub {
    workers: HashMap<(String, String), Arc<Mutex<SharedWorkerInstance>>>,
}

impl SharedWorkerHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_spawn(
        &mut self,
        origin: &str,
        name: &str,
        script_url: &str,
        script_content: String,
        port_hub: SharedPortHub,
    ) -> Result<Arc<Mutex<SharedWorkerInstance>>, String> {
        let key = (origin.to_string(), name.to_string());
        if let Some(existing) = self.workers.get(&key) {
            return Ok(existing.clone());
        }

        let instance = Arc::new(Mutex::new(SharedWorkerInstance::spawn(
            name.to_string(),
            script_url.to_string(),
            script_content,
            port_hub,
        )?));
        self.workers.insert(key, instance.clone());
        Ok(instance)
    }

    pub fn clear(&mut self) {
        for (_, worker) in self.workers.drain() {
            if let Ok(mut w) = worker.lock() {
                w.terminate();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::transfer::MessagePortHub;

    #[test]
    fn shared_worker_multi_connection_and_state_sharing() {
        let port_hub = Arc::new(Mutex::new(MessagePortHub::new()));
        let mut hub = SharedWorkerHub::new();

        let shared_script = r#"
            let count = 0;
            onconnect = function(e) {
                const port = e.ports[0];
                port.onmessage = function(msg) {
                    count++;
                    port.postMessage({ count: count, echo: msg.data });
                };
            };
        "#;

        // Document 1 connects
        let (doc1_port, worker1_port) = port_hub.lock().unwrap().create_channel();
        let instance1 = hub
            .get_or_spawn(
                "https://app.test",
                "counter",
                "https://app.test/shared.js",
                shared_script.into(),
                port_hub.clone(),
            )
            .unwrap();
        instance1.lock().unwrap().connect(worker1_port).unwrap();

        // Document 1 posts message
        port_hub
            .lock()
            .unwrap()
            .post_message(doc1_port, "\"doc1 first\"".into(), vec![])
            .unwrap();

        // Wait for doc1 reply
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut doc1_res = None;
        while std::time::Instant::now() < deadline {
            let messages = port_hub.lock().unwrap().poll_messages(doc1_port);
            if let Some(msg) = messages.into_iter().next() {
                doc1_res = Some(msg.data);
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(doc1_res.is_some(), "Did not receive doc1 reply");
        assert!(doc1_res.unwrap().contains("\"count\":1"));

        // Document 2 connects to the SAME shared worker
        let (doc2_port, worker2_port) = port_hub.lock().unwrap().create_channel();
        let instance2 = hub
            .get_or_spawn(
                "https://app.test",
                "counter",
                "https://app.test/shared.js",
                shared_script.into(),
                port_hub.clone(),
            )
            .unwrap();
        instance2.lock().unwrap().connect(worker2_port).unwrap();

        // Document 2 posts message -> count should be 2!
        port_hub
            .lock()
            .unwrap()
            .post_message(doc2_port, "\"doc2 second\"".into(), vec![])
            .unwrap();

        let mut doc2_res = None;
        while std::time::Instant::now() < deadline {
            let messages = port_hub.lock().unwrap().poll_messages(doc2_port);
            if let Some(msg) = messages.into_iter().next() {
                doc2_res = Some(msg.data);
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(doc2_res.is_some(), "Did not receive doc2 reply");
        assert!(
            doc2_res.unwrap().contains("\"count\":2"),
            "Shared worker state must be shared across connections"
        );
    }
}
