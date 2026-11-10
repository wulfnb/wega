//! DedicatedWorker execution on independent threads and V8 isolates.

use deno_core::{JsRuntime, RuntimeOptions};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkerId(pub u64);

#[derive(Debug)]
pub enum WorkerCommand {
    PostMessage(String),
    Terminate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerEvent {
    Message(String),
    Error(String),
    Terminated,
}

pub struct DedicatedWorker {
    id: WorkerId,
    to_worker: SyncSender<WorkerCommand>,
    from_worker: Receiver<WorkerEvent>,
    terminated: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl DedicatedWorker {
    pub fn spawn(id: WorkerId, script_content: String) -> Result<Self, String> {
        let (to_worker_tx, to_worker_rx) = mpsc::sync_channel::<WorkerCommand>(64);
        let (from_worker_tx, from_worker_rx) = mpsc::sync_channel::<WorkerEvent>(64);
        let terminated = Arc::new(AtomicBool::new(false));
        let term_clone = terminated.clone();

        let thread = thread::Builder::new()
            .name(format!("wega-dedicated-worker-{}", id.0))
            .spawn(move || {
                let mut runtime = match JsRuntime::try_new(RuntimeOptions::default()) {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = from_worker_tx.send(WorkerEvent::Error(format!("Failed to create worker isolate: {e}")));
                        return;
                    }
                };

                // Bootstrap DedicatedWorkerGlobalScope
                let bootstrap_js = r##"
                    globalThis.self = globalThis;
                    globalThis.__outbox = [];
                    globalThis.postMessage = function(data) {
                        globalThis.__outbox.push(JSON.stringify(data));
                    };
                    globalThis.close = function() {
                        globalThis.__closed = true;
                    };
                    globalThis.ImageData = class ImageData {
                        constructor(w, h) {
                            this.width = w >>> 0;
                            this.height = h >>> 0;
                            this.data = new Uint8ClampedArray(this.width * this.height * 4);
                        }
                    };
                    globalThis.OffscreenCanvas = class OffscreenCanvas {
                        constructor(width, height) {
                            this.width = Math.max(1, width >>> 0);
                            this.height = Math.max(1, height >>> 0);
                            this._detached = false;
                        }
                        getContext(type) {
                            if (this._detached) throw new Error("InvalidStateError: Canvas is detached");
                            if (type === "2d") {
                                if (!this._ctx) {
                                    this._ctx = {
                                        canvas: this,
                                        fillStyle: "#000000",
                                        fillRect(x, y, w, h) {},
                                        clearRect(x, y, w, h) {},
                                        getImageData(x, y, w, h) { return new ImageData(w, h); }
                                    };
                                }
                                return this._ctx;
                            }
                            if (type === "webgl" || type === "experimental-webgl") {
                                if (!this._gl) {
                                    this._gl = {
                                        canvas: this,
                                        clearColor(r, g, b, a) {},
                                        clear(mask) {},
                                        createBuffer() { return {}; },
                                        bindBuffer(t, b) {},
                                        bufferData(t, d, u) {},
                                        createShader(t) { return {}; },
                                        shaderSource(s, src) {},
                                        compileShader(s) {},
                                        getShaderParameter(s, p) { return true; },
                                        createProgram() { return {}; },
                                        attachShader(p, s) {},
                                        linkProgram(p) {},
                                        useProgram(p) {},
                                        getProgramParameter(p, param) { return true; },
                                        drawArrays(m, f, c) {},
                                        readPixels(x, y, w, h, f, t, p) { return p; }
                                    };
                                }
                                return this._gl;
                            }
                            return null;
                        }
                    };
                "##;
                if let Err(e) = runtime.execute_script("bootstrap", bootstrap_js) {
                    let _ = from_worker_tx.send(WorkerEvent::Error(format!("Bootstrap error: {e}")));
                    return;
                }

                // Execute worker script
                if let Err(e) = runtime.execute_script("worker_script", script_content) {
                    let _ = from_worker_tx.send(WorkerEvent::Error(format!("Worker script execution error: {e}")));
                    return;
                }

                // Flush initial messages if any
                Self::flush_outbox(&mut runtime, &from_worker_tx);

                // Worker event loop
                while !term_clone.load(Ordering::Acquire) {
                    match to_worker_rx.recv_timeout(Duration::from_millis(10)) {
                        Ok(WorkerCommand::PostMessage(data)) => {
                            let dispatch_js = format!(
                                "if (typeof onmessage === 'function') {{ onmessage({{ data: {data} }}); }}
                                 if (typeof globalThis.dispatchEvent === 'function') {{
                                     globalThis.dispatchEvent(new MessageEvent('message', {{ data: {data} }}));
                                 }}"
                            );
                            if let Err(e) = runtime.execute_script("dispatch_message", dispatch_js) {
                                let _ = from_worker_tx.send(WorkerEvent::Error(e.to_string()));
                            }
                            Self::flush_outbox(&mut runtime, &from_worker_tx);

                            // Check self.close()
                            if let Ok(closed) = runtime.execute_script("check_close", "Boolean(globalThis.__closed)") {
                                deno_core::scope!(scope, runtime);
                                let local = deno_core::v8::Local::new(scope, closed);
                                if local.is_true() {
                                    break;
                                }
                            }
                        }
                        Ok(WorkerCommand::Terminate) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            // Check self.close()
                            if let Ok(closed) = runtime.execute_script("check_close", "Boolean(globalThis.__closed)") {
                                deno_core::scope!(scope, runtime);
                                let local = deno_core::v8::Local::new(scope, closed);
                                if local.is_true() {
                                    break;
                                }
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }

                let _ = from_worker_tx.send(WorkerEvent::Terminated);
            })
            .map_err(|e| format!("Failed to spawn worker thread: {e}"))?;

        Ok(Self {
            id,
            to_worker: to_worker_tx,
            from_worker: from_worker_rx,
            terminated,
            thread: Some(thread),
        })
    }

    fn flush_outbox(runtime: &mut JsRuntime, from_worker: &SyncSender<WorkerEvent>) {
        let drain_js = "JSON.stringify(globalThis.__outbox.splice(0, globalThis.__outbox.length))";
        if let Ok(val) = runtime.execute_script("drain_outbox", drain_js) {
            deno_core::scope!(scope, runtime);
            let local = deno_core::v8::Local::new(scope, val);
            let json_str = local.to_rust_string_lossy(scope);
            if let Ok(messages) = serde_json::from_str::<Vec<String>>(&json_str) {
                for msg in messages {
                    let _ = from_worker.send(WorkerEvent::Message(msg));
                }
            }
        }
    }

    pub fn id(&self) -> WorkerId {
        self.id
    }

    pub fn post_message(&self, data: String) -> Result<(), String> {
        if self.terminated.load(Ordering::Acquire) {
            return Err("Worker is terminated".into());
        }
        self.to_worker
            .send(WorkerCommand::PostMessage(data))
            .map_err(|e| e.to_string())
    }

    pub fn terminate(&mut self) {
        if !self.terminated.swap(true, Ordering::Release) {
            let _ = self.to_worker.send(WorkerCommand::Terminate);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    pub fn poll_events(&self) -> Vec<WorkerEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.from_worker.try_recv() {
            events.push(event);
        }
        events
    }
}

impl Drop for DedicatedWorker {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedicated_worker_lifecycle_messaging_and_termination() {
        let script = r#"
            onmessage = function(e) {
                postMessage({ echo: e.data.text, double: e.data.num * 2 });
            };
        "#;
        let mut worker = DedicatedWorker::spawn(WorkerId(1), script.into()).unwrap();

        worker
            .post_message(r#"{"text":"hello","num":21}"#.into())
            .unwrap();

        // Wait for reply
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut received = None;
        while std::time::Instant::now() < deadline {
            for event in worker.poll_events() {
                if let WorkerEvent::Message(data) = event {
                    received = Some(data);
                    break;
                }
            }
            if received.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        assert!(received.is_some(), "Did not receive reply from worker");
        let reply = received.unwrap();
        assert!(reply.contains("\"echo\":\"hello\""));
        assert!(reply.contains("\"double\":42"));

        // Terminate worker
        worker.terminate();
        assert!(worker.post_message("dropped".into()).is_err());
    }
}
