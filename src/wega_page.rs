//! A real page in the independently built Blitz/V8 worker. IPC never blocks the UI.
use crate::engine::Wake;
use std::{
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
};
use wega::{
    engine_protocol::{self as protocol, Command as Request, Event},
    model::Policy,
    navigation::NavigationPort,
};
use winit::event_loop::EventLoopProxy;

pub(crate) fn executable() -> Option<PathBuf> {
    let path = std::env::var_os("WEGA_ENGINE_BINARY")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe().ok()?.parent().map(|p| {
                p.join(if cfg!(windows) {
                    "wega-engine.exe"
                } else {
                    "wega-engine"
                })
            })
        })?;
    path.is_file().then_some(path)
}
#[derive(Default)]
pub(crate) struct Snapshot {
    pub serial: u64,
    pub rss_bytes: Option<u64>,
    pub interacted: bool,
    pub compatibility: Option<String>,
    pub fallback_reported: bool,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub failed: bool,
    pub nodes: usize,
    pub js_heap: usize,
    pub notices: std::collections::VecDeque<String>,
    pub navigations: std::collections::VecDeque<String>,
    pub frame: Option<(u32, u32, Vec<u8>)>,
    pub capture: Option<PathBuf>,
    pub capture_result: Option<Result<(), String>>,
    pub visited: Option<(String, String)>,
    pub evaluated: Option<Result<String, String>>,
    pub frozen: bool,
}
pub(crate) struct Page {
    child: Child,
    frames: Arc<Mutex<wega_platform::FrameStore>>,
    broker: wega_network::SessionHandle,
    requests: Option<mpsc::SyncSender<Request>>,
    pub state: Arc<Mutex<Snapshot>>,
    pub texture: Option<egui::TextureHandle>,
    pub viewport: Option<(u32, u32, f32, f32, bool)>,
    pub zoom: f32,
    history: Vec<String>,
    index: usize,
}
fn failed(state: &Arc<Mutex<Snapshot>>, message: String, proxy: &EventLoopProxy<Wake>) {
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.compatibility.get_or_insert_with(|| message.clone());
    state.failed = true;
    state.loading = false;
    if state.notices.len() < 16 {
        state.notices.push_back(message);
    }
    let _ = proxy.send_event(Wake::Ui);
}
fn accept_frame(shared: &Mutex<Snapshot>, serial: u64, width: u32, height: u32, bytes: Vec<u8>) {
    let capture = {
        let mut state = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if serial != state.serial || state.frozen {
            return;
        }
        state.capture.take()
    };
    // PNG encoding must not hold the UI's state lock.
    let result = capture.map(|path| {
        image::save_buffer(path, &bytes, width, height, image::ColorType::Rgba8)
            .map_err(|e| e.to_string())
    });
    let mut state = shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if serial == state.serial && !state.frozen {
        if let Some(result) = result {
            state.capture_result = Some(result);
        }
        state.frame = Some((width, height, bytes));
    }
}
impl Page {
    pub fn new(
        url: &url::Url,
        policy: &Policy,
        proxy: EventLoopProxy<Wake>,
        profile_dir: &std::path::Path,
    ) -> Result<Self, String> {
        let executable=executable().ok_or("Wega engine worker is not installed. Build it with python3 scripts/build-wega-engine.py --debug first.")?;
        let broker = wega_network::Broker::shared()?.open_session(
            profile_dir.join("wega-storage"),
            wega_network::Policy {
                offline: policy.offline,
                blocked_hosts: policy.blocked_hosts.clone(),
                allowed_hosts: policy.allowed_hosts.clone(),
            },
        )?;
        let frames = Arc::new(Mutex::new(
            wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4)
                .map_err(|e| e.to_string())?,
        ));
        let frame_path = frames
            .lock()
            .unwrap()
            .path()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let (broker_address, broker_token) = broker.endpoint();
        let mut command = Command::new(executable);
        command.env("MALLOC_ARENA_MAX", "2");
        // Workers are private to a view, so process-wide V8 flags stay per-view.
        if std::env::var_os("WEGA_ENGINE_LITE").is_some_and(|value| value == "1")
            || wega::engine_choice::rollout_site(url)
        {
            command.arg("--lite");
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // Private worker, no console window.
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot start Wega engine: {e}"))?;
        let mut stdin = child.stdin.take().ok_or("Missing worker input")?;
        let mut stdout = child.stdout.take().ok_or("Missing worker output")?;
        let state = Arc::new(Mutex::new(Snapshot {
            url: url.to_string(),
            loading: true,
            ..Default::default()
        }));
        let (sender, receiver) = mpsc::sync_channel(32);
        let shared = state.clone();
        let writer_proxy = proxy.clone();
        std::thread::spawn(move || {
            while let Ok(request) = receiver.recv() {
                if let Err(error) = protocol::write(&mut stdin, &request) {
                    failed(
                        &shared,
                        format!("Wega engine connection failed: {error}"),
                        &writer_proxy,
                    );
                    break;
                }
            }
        });
        let shared = state.clone();
        let reader_frames = frames.clone();
        let acknowledgements = sender.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                let mut ready = false;
                while let Some(event) =
                    protocol::read::<Event>(&mut stdout).map_err(|e| e.to_string())?
                {
                    if !ready {
                        if !matches!(
                            event,
                            Event::Ready {
                                version: protocol::VERSION
                            }
                        ) {
                            return Err("Incompatible Wega engine worker".into());
                        }
                        ready = true;
                        continue;
                    }
                    match event {
                        Event::SharedFrame {
                            serial,
                            width,
                            height,
                            slot,
                            sequence,
                        } => {
                            let size =
                                protocol::frame_bytes(width, height).map_err(|e| e.to_string())?;
                            // Acknowledge stale/frozen frames without faulting their pages back in.
                            let bytes = {
                                // Freeze takes these locks in the same order. Keep the
                                // state lock through the copy so freeze cannot unmap and
                                // then race with a reader remapping a hidden tab.
                                let state = shared
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                if serial != state.serial || state.frozen {
                                    None
                                } else {
                                    std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
                                    Some(
                                        reader_frames
                                            .lock()
                                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                                            .slot(slot as usize, size)
                                            .map_err(|e| e.to_string())?
                                            .to_vec(),
                                    )
                                }
                            };
                            acknowledgements
                                .send(Request::FrameConsumed { slot, sequence })
                                .map_err(|_| "Worker closed before frame acknowledgement")?;
                            if let Some(bytes) = bytes {
                                accept_frame(&shared, serial, width, height, bytes);
                            }
                        }
                        Event::Compatibility { serial, reason } => {
                            let mut state = shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if state.serial == serial && !state.fallback_reported {
                                state.compatibility.get_or_insert(reason);
                            }
                        }
                        Event::Frame {
                            serial,
                            width,
                            height,
                        } => {
                            let mut bytes = vec![
                                0;
                                protocol::frame_bytes(width, height)
                                    .map_err(|e| e.to_string())?
                            ];
                            stdout.read_exact(&mut bytes).map_err(|e| e.to_string())?;
                            accept_frame(&shared, serial, width, height, bytes);
                        }
                        Event::State {
                            serial,
                            rss_bytes,
                            url,
                            title,
                            loading,
                            nodes,
                            js_heap,
                        } => {
                            let mut state = shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if serial != state.serial {
                                continue;
                            }
                            state.rss_bytes = rss_bytes;
                            if state.loading && !loading {
                                state.visited = Some((url.clone(), title.clone()));
                            }
                            state.url = url;
                            state.title = title;
                            state.loading = loading;
                            state.nodes = nodes;
                            state.js_heap = js_heap;
                        }
                        Event::Notice(message) => {
                            let mut state = shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if state.notices.len() < 16 {
                                state.notices.push_back(message);
                            }
                        }
                        Event::Navigate(url) => {
                            let mut state = shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if state.navigations.len() < 8 {
                                state.navigations.push_back(url);
                            }
                        }
                        Event::Evaluated { value, error } => {
                            shared
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .evaluated = Some(error.map_or(Ok(value), Err));
                        }
                        Event::Ready { .. } => return Err("Unexpected worker handshake".into()),
                    }
                    let _ = proxy.send_event(Wake::Ui);
                }
                Err("Wega engine stopped. Reload this page or switch to Servo.".into())
            })();
            if let Err(error) = result {
                failed(&shared, error, &proxy);
            }
        });
        let page = Self {
            child,
            frames,
            broker,
            requests: Some(sender),
            state,
            texture: None,
            viewport: None,
            zoom: 1.0,
            history: vec![url.to_string()],
            index: 0,
        };
        page.send(Request::Init {
            version: protocol::VERSION,
            url: url.to_string(),
            offline: policy.offline,
            blocked_hosts: policy.blocked_hosts.clone(),
            allowed_hosts: policy.allowed_hosts.clone(),
            broker_address: Some(broker_address),
            broker_token: Some(broker_token),
            frame_path: Some(frame_path),
            storage_dir: Some(
                profile_dir
                    .join("wega-storage")
                    .to_string_lossy()
                    .into_owned(),
            ),
        })?;
        Ok(page)
    }
    pub fn send(&self, request: Request) -> Result<(), String> {
        if matches!(
            &request,
            Request::Click { .. }
                | Request::Key { pressed: true, .. }
                | Request::Ime { .. }
                | Request::Evaluate(_)
        ) {
            self.state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .interacted = true;
        }
        if let Request::Policy {
            offline,
            blocked_hosts,
            allowed_hosts,
        } = &request
        {
            self.broker.set_policy(wega_network::Policy {
                offline: *offline,
                blocked_hosts: blocked_hosts.clone(),
                allowed_hosts: allowed_hosts.clone(),
            });
        }
        self.requests
            .as_ref()
            .ok_or("Worker is closed")?
            .try_send(request)
            .map_err(|_| "Wega engine is busy or stopped. Reload or switch to Servo.".into())
    }
    pub fn keyboard(
        &self,
        event: &winit::event::KeyEvent,
        modifiers: winit::keyboard::ModifiersState,
    ) -> Result<(), String> {
        let key = match &event.logical_key {
            winit::keyboard::Key::Character(value) => value.to_string(),
            winit::keyboard::Key::Named(value) => format!("{value:?}"),
            _ => "Unidentified".into(),
        };
        let code = match event.physical_key {
            winit::keyboard::PhysicalKey::Code(code) => format!("{code:?}"),
            _ => "Unidentified".into(),
        };
        let flags = u8::from(modifiers.shift_key())
            | u8::from(modifiers.control_key()) << 1
            | u8::from(modifiers.alt_key()) << 2
            | u8::from(modifiers.super_key()) << 3;
        self.send(Request::Key {
            key,
            code,
            text: event.text.as_ref().map(ToString::to_string),
            pressed: event.state.is_pressed(),
            modifiers: flags,
            repeat: event.repeat,
        })
    }
    pub fn ime(&self, event: &winit::event::Ime) -> Result<(), String> {
        match event {
            winit::event::Ime::Preedit(text, cursor) => self.send(Request::Ime {
                text: text.clone(),
                preedit: true,
                cursor: *cursor,
            }),
            winit::event::Ime::Commit(text) => self.send(Request::Ime {
                text: text.clone(),
                preedit: false,
                cursor: None,
            }),
            _ => Ok(()),
        }
    }
    pub fn command_sender(&self) -> Option<mpsc::SyncSender<Request>> {
        self.requests.clone()
    }
    pub fn url(&self) -> String {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .url
            .clone()
    }
    pub fn title(&self) -> String {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .title
            .clone()
    }
    pub fn loading(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .loading
    }
    pub fn can_back(&self) -> bool {
        self.index > 0
    }
    pub fn can_forward(&self) -> bool {
        self.index + 1 < self.history.len()
    }
    pub fn freeze(&mut self, frozen: bool) {
        let _ = self.send(Request::Freeze(frozen));
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.frozen = frozen;
        if frozen {
            self.texture = None;
            state.frame = None;
            self.frames
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .release_pages();
        }
        self.viewport = None;
    }
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    fn request_load(&self, url: &url::Url) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let serial = state.serial.wrapping_add(1);
        self.send(Request::Load {
            url: url.to_string(),
            serial,
        })?;
        state.serial = serial;
        state.interacted = false;
        state.compatibility = None;
        state.fallback_reported = false;
        drop(state);
        Ok(())
    }
    pub fn load_url(&mut self, url: url::Url) -> Result<(), String> {
        self.request_load(&url)?;
        self.history.truncate(self.index + 1);
        self.history.push(url.to_string());
        self.index = self.history.len() - 1;
        if self.history.len() > 256 {
            self.history.remove(0);
            self.index -= 1;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.url = url.to_string();
        state.loading = true;
        state.failed = false;
        Ok(())
    }
    pub fn resize(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        dark: bool,
    ) -> Result<(), String> {
        let viewport = (width, height, scale, self.zoom, dark);
        if self.viewport != Some(viewport) {
            self.send(Request::Resize {
                width,
                height,
                scale,
                zoom: self.zoom,
                dark,
            })?;
            self.viewport = Some(viewport);
        }
        Ok(())
    }
}
impl NavigationPort for Page {
    fn load(&mut self, url: url::Url) {
        let _ = self.load_url(url);
    }
    fn reload(&mut self) {
        if let Ok(url) = url::Url::parse(&self.url()) {
            let _ = self.request_load(&url);
        }
    }
    fn traverse(&mut self, forward: bool) -> bool {
        if (forward && !self.can_forward()) || (!forward && !self.can_back()) {
            return false;
        }
        let next = if forward {
            self.index + 1
        } else {
            self.index - 1
        };
        if url::Url::parse(&self.history[next])
            .map_err(|e| e.to_string())
            .and_then(|url| self.request_load(&url))
            .is_err()
        {
            return false;
        }
        self.index = next;
        true
    }
    fn zoom(&self) -> f32 {
        self.zoom
    }
    fn set_zoom(&mut self, value: f32) {
        self.zoom = value;
        self.viewport = None;
    }
}
impl Drop for Page {
    fn drop(&mut self) {
        self.requests.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
