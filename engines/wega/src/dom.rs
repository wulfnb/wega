//! Checked native operations over Blitz's single DOM, used by the V8 host.
use crate::Limits;
use blitz_dom::NodeId;
use blitz_dom::node::NodeData;
use html5ever::{LocalName, QualName, ns};
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
use wega_adapter_blitz::BlitzDocument;
use wega_adapter_blitz::{ToKey, ToNodeId};
use wega_dom::{DocumentView, Mutator, NodeKind, NodeView};

/// A `<script>` element that script inserted into the document.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScriptRequest {
    pub node: NodeId,
    /// Dynamically inserted scripts are async unless script set `async = false`.
    pub force_async: bool,
}
/// Synchronous dialogs (`alert`, `confirm`, `prompt`) answered by the browser.
pub type DialogHandler = Box<dyn FnMut(&str, &str, &str) -> Option<String>>;
pub type DocumentDialogHandler = Box<dyn FnMut(u64, u64, &str, &str, &str, &str) -> Option<String>>;

/// Requests from page script that the view or the browser must act on.
#[derive(Default)]
pub(crate) struct Requests {
    pub scripts: VecDeque<ScriptRequest>,
    /// `element.click()` activation behaviour, run natively after script.
    pub activate: Vec<NodeId>,
    pub writes: String,
    pub opened: Vec<url::Url>,
    pub traverse: Vec<i32>,
    pub compatibility: Option<String>,
    pub permission_prompts: Vec<(u64, String, wega_protocol::permissions::PermissionFeature)>,
    pub notifications: Vec<(u64, String, wega_protocol::permissions::NotificationOptions)>,
    pub notification_closes: Vec<u64>,
    pub fullscreen: Vec<bool>,
    pub protocol_registrations: Vec<wega_protocol::permissions::ProtocolHandlerRegistration>,
    pub external_apps: Vec<String>,
    /// Text for the OS clipboard, written by the browser process.
    pub clipboard_writes: Vec<String>,
}

pub(crate) struct Dom {
    pub network: crate::network::Network,
    pub hubs: crate::workers::Hubs,
    pub document: Rc<RefCell<BlitzDocument>>,
    pub limits: Limits,
    pub navigate: Rc<RefCell<Vec<url::Url>>>,
    pub errors: Rc<RefCell<Vec<String>>>,
    pub requests: RefCell<Requests>,
    pub csp: std::sync::Arc<std::sync::RwLock<crate::csp::Csp>>,
    pub dialog: Rc<RefCell<Option<DocumentDialogHandler>>>,
    pub document_generation: Rc<Cell<u64>>,
    pub document_revision: u64,
    /// Transient user activation, for popups.
    pub activation: Rc<Cell<Option<Instant>>>,
    pub started_scripts: Rc<RefCell<HashSet<NodeId>>>,
    /// This document's session history: (index, length), reported by script.
    pub history: Cell<(usize, usize)>,
    allocated: Cell<usize>,
    used: Cell<usize>,
    start: Instant,
    retained: RefCell<HashSet<NodeId>>,
    components: RefCell<crate::components::Registry>,
    shadows: RefCell<crate::components::ShadowTree>,
    clipboard: RefCell<String>,
    /// This document as a service worker client.
    client_id: String,
    created: Instant,
    pub workers: RefCell<crate::workers::WorkerManager>,
    pub canvas_engine: RefCell<crate::canvas::CanvasEngine>,
    pub webgl_engine: RefCell<crate::webgl::WebGLEngine>,
    pub media_engine: RefCell<crate::media::MediaEngine>,
    pub permissions: Rc<RefCell<crate::permissions::PermissionRegistry>>,
    // The DOM thread cannot receive parser/input mutations during script. Start
    // a fresh epoch at each host step, then invalidate synchronously on writes.
    revision: Cell<u64>,
    layout_dirty: Cell<bool>,
}
pub fn origin_for_url(url: &url::Url) -> wega_protocol::authority::Origin {
    match url.scheme() {
        "http" | "https" | "file" | "ws" | "wss" => {
            let ser = url.origin().ascii_serialization();
            if ser == "null" || ser.is_empty() {
                wega_protocol::authority::Origin::Opaque
            } else {
                wega_protocol::authority::Origin::Tuple(ser)
            }
        }
        _ => wega_protocol::authority::Origin::Opaque,
    }
}

impl Dom {
    pub fn new(context: crate::script::HostContext, limits: &Limits, initial_bytes: usize) -> Self {
        let document = context.document;
        let allocated =
            Cell::new(blitz_dom::traversal::TreeTraverser::new(&document.borrow()).count());
        let doc_url = document.borrow().url().clone();
        context.network.mark_navigated();
        context.hubs.service.configure(limits);
        let workers = RefCell::new(crate::workers::WorkerManager::with_hubs(
            doc_url,
            context.hubs.clone(),
        ));
        Self {
            network: context.network,
            hubs: context.hubs,
            document,
            limits: limits.clone(),
            navigate: context.navigate,
            errors: Default::default(),
            requests: Default::default(),
            csp: context.csp,
            dialog: context.dialog,
            document_generation: context.document_generation,
            document_revision: context.document_revision,
            activation: context.activation,
            started_scripts: context.started_scripts,
            history: Cell::new((0, 1)),
            allocated,
            used: Cell::new(initial_bytes),
            start: Instant::now(),
            retained: Default::default(),
            components: Default::default(),
            shadows: Default::default(),
            clipboard: Default::default(),
            client_id: {
                use std::hash::{BuildHasher, Hasher};
                let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
                hasher.write_u64(1);
                format!("{:016x}", hasher.finish())
            },
            created: Instant::now(),
            workers,
            canvas_engine: RefCell::new(crate::canvas::CanvasEngine::new(limits.max_pixels * 4)),
            webgl_engine: RefCell::new(crate::webgl::WebGLEngine::new(limits.max_pixels * 8)),
            media_engine: RefCell::new(crate::media::MediaEngine::new(limits.resource_bytes * 8)),
            permissions: context.permissions,
            revision: Cell::new(0),
            layout_dirty: Cell::new(true),
        }
    }
    fn port_hub(&self) -> std::sync::MutexGuard<'_, crate::workers::MessagePortHub> {
        self.hubs
            .ports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    /// Copy frames committed by workers into their placeholder canvases.
    fn apply_worker_frames(&self) {
        let frames = self.workers.borrow().take_frames();
        if frames.is_empty() {
            return;
        }
        let mut engine = self.canvas_engine.borrow_mut();
        for (id, frame) in frames {
            let id = crate::canvas::CanvasId(id);
            let Some(context) = engine.get_mut(id) else {
                continue;
            };
            if (context.width, context.height) != (frame.width, frame.height)
                && engine.resize_canvas(id, frame.width, frame.height).is_err()
            {
                continue;
            }
            if let Some(context) = engine.get_mut(id) {
                let _ = context.put_image_data(
                    &frame.pixels,
                    0,
                    0,
                    frame.width as i32,
                    frame.height as i32,
                );
            }
        }
    }
    pub fn begin_task(&self) {
        self.invalidate();
    }
    fn invalidate(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
        self.layout_dirty.set(true);
    }
    fn resolve(&self, doc: &mut BlitzDocument) {
        if self.layout_dirty.replace(false) {
            doc.resolve(self.start.elapsed().as_secs_f64());
        }
    }
    pub fn call(&self, args: &[Value]) -> Result<Value, String> {
        let text = |index: usize| args.get(index).and_then(Value::as_str).unwrap_or_default();
        // Each committed document owns its realm and Dom, so only node
        // liveness needs checking here.
        if text(0) == "bindingCheck" {
            let identities = args
                .get(2)
                .and_then(Value::as_array)
                .ok_or("TypeError: invalid binding identities")?;
            let document = self
                .document
                .try_borrow()
                .map_err(|_| "InvalidStateError: reentrant native DOM borrow")?;
            for identity in identities {
                let key = wega_dom::NodeKey::from_u64(
                    identity
                        .as_str()
                        .and_then(|id| id.parse::<u64>().ok())
                        .ok_or("TypeError: invalid binding node identity")?,
                );
                if wega_dom::DocumentView::node(&*document, key).is_none() {
                    return Err("InvalidStateError: binding node no longer exists".into());
                }
            }
            return Ok(Value::Null);
        }
        if text(0).starts_with("canvas") {
            self.apply_worker_frames();
        }
        if let Some(result) =
            crate::canvas::host::host_call(&mut self.canvas_engine.borrow_mut(), args)
        {
            return result;
        }
        if let Some(result) =
            crate::webgl::host::host_call(&mut self.webgl_engine.borrow_mut(), args)
        {
            return result;
        }
        match text(0) {
            "cacheAllowed" => {
                return Ok(json!(wega_network::cache_api::cache_allowed(
                    self.document.borrow().url()
                )));
            }
            "cacheCall" => {
                let url = self.document.borrow().url().clone();
                return self.network.cache_call(&url, text(1));
            }
            "mediaCanPlayType" => {
                let mime = text(1);
                let answer = crate::media::MediaEngine::can_play_type(mime);
                return Ok(json!(answer));
            }
            "mediaCreate" => {
                let kind = if text(1) == "audio" {
                    crate::media::MediaKind::Audio
                } else {
                    crate::media::MediaKind::Video
                };
                let id = self.media_engine.borrow_mut().create_instance(kind);
                return Ok(json!(id.0));
            }
            "mediaLoad" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid Media ID")?;
                let src = text(2);
                let find_bytes = || -> Vec<u8> {
                    if let Ok(data_url) = data_url::DataUrl::process(src) {
                        return data_url.decode_to_vec().map(|d| d.0).unwrap_or_default();
                    }
                    let filename = if let Ok(parsed) = url::Url::parse(src) {
                        if parsed.scheme() == "file" {
                            if let Ok(p) = parsed.to_file_path() {
                                if let Ok(b) = std::fs::read(p) {
                                    return b;
                                }
                            }
                        }
                        parsed.path().split('/').last().unwrap_or("").to_string()
                    } else {
                        src.split('/').last().unwrap_or("").to_string()
                    };
                    let candidates = [
                        src.to_string(),
                        filename.clone(),
                        format!("tests/fixtures/{filename}"),
                        format!("../tests/fixtures/{filename}"),
                        format!("../../tests/fixtures/{filename}"),
                    ];
                    for c in &candidates {
                        if !c.is_empty() {
                            if let Ok(b) = std::fs::read(c) {
                                return b;
                            }
                        }
                    }
                    vec![]
                };
                let bytes = find_bytes();

                let mut engine = self.media_engine.borrow_mut();
                let inst = engine
                    .get_instance_mut(crate::media::MediaId(id))
                    .ok_or("Media instance not found")?;

                if bytes.is_empty() {
                    // A source that cannot be read is a load failure; never
                    // report synthetic metadata for it.
                    inst.clock.error = Some(crate::media::MediaError {
                        code: crate::media::MediaErrorCode::SrcNotSupported,
                        message: "Failed to load media: unsupported format or missing source"
                            .into(),
                    });
                    inst.clock.ready_state = crate::media::ReadyState::HaveNothing;
                    inst.clock.network_state = crate::media::NetworkState::NoSource;
                } else {
                    let _ = inst.load_bytes(src, bytes);
                }
                return Ok(Value::Null);
            }
            "mediaPlay" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid Media ID")?;
                let mut engine = self.media_engine.borrow_mut();
                let inst = engine
                    .get_instance_mut(crate::media::MediaId(id))
                    .ok_or("Media instance not found")?;
                inst.clock.play().map_err(|e| e.to_string())?;
                return Ok(Value::Null);
            }
            "mediaPause" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid Media ID")?;
                let mut engine = self.media_engine.borrow_mut();
                let inst = engine
                    .get_instance_mut(crate::media::MediaId(id))
                    .ok_or("Media instance not found")?;
                inst.clock.pause();
                return Ok(Value::Null);
            }
            "mediaSeek" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid Media ID")?;
                let pos = text(2).parse::<f64>().unwrap_or(0.0);
                let mut engine = self.media_engine.borrow_mut();
                let inst = engine
                    .get_instance_mut(crate::media::MediaId(id))
                    .ok_or("Media instance not found")?;
                inst.clock.seek(pos);
                return Ok(Value::Null);
            }
            "mediaSetVolume" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid Media ID")?;
                let vol = text(2).parse::<f64>().map_err(|_| "Invalid volume")?;
                let mut engine = self.media_engine.borrow_mut();
                let inst = engine
                    .get_instance_mut(crate::media::MediaId(id))
                    .ok_or("Media instance not found")?;
                inst.clock.set_volume(vol).map_err(|e| e.to_string())?;
                return Ok(Value::Null);
            }
            "mediaSetMuted" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid Media ID")?;
                let muted = text(2) == "true";
                let mut engine = self.media_engine.borrow_mut();
                let inst = engine
                    .get_instance_mut(crate::media::MediaId(id))
                    .ok_or("Media instance not found")?;
                inst.clock.set_muted(muted);
                return Ok(Value::Null);
            }
            "mediaSetRate" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid Media ID")?;
                let rate = text(2).parse::<f64>().unwrap_or(1.0);
                let mut engine = self.media_engine.borrow_mut();
                let inst = engine
                    .get_instance_mut(crate::media::MediaId(id))
                    .ok_or("Media instance not found")?;
                inst.clock.set_playback_rate(rate);
                return Ok(Value::Null);
            }
            "mediaSetLoop" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid Media ID")?;
                let looping = text(2) == "true";
                let mut engine = self.media_engine.borrow_mut();
                let inst = engine
                    .get_instance_mut(crate::media::MediaId(id))
                    .ok_or("Media instance not found")?;
                inst.clock.set_loop(looping);
                return Ok(Value::Null);
            }
            "mediaGetState" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid Media ID")?;
                let mut engine = self.media_engine.borrow_mut();
                let inst = engine
                    .get_instance_mut(crate::media::MediaId(id))
                    .ok_or("Media instance not found")?;
                let cur = inst.clock.current_position();
                let state = json!({
                    "currentTime": cur,
                    "duration": inst.clock.duration,
                    "paused": inst.clock.paused,
                    "ended": inst.clock.ended,
                    "seeking": inst.clock.seeking,
                    "playbackRate": inst.clock.playback_rate,
                    "volume": inst.clock.volume,
                    "muted": inst.clock.muted,
                    "loop": inst.clock.looping,
                    "readyState": inst.clock.ready_state as u32,
                    "networkState": inst.clock.network_state as u32,
                    "videoWidth": inst.video_width,
                    "videoHeight": inst.video_height,
                    "error": inst.clock.error.as_ref().map(|e| json!({"code": e.code as u32, "message": &e.message})),
                });
                return Ok(state);
            }
            "mediaDestroy" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid Media ID")?;
                self.media_engine
                    .borrow_mut()
                    .destroy_instance(crate::media::MediaId(id));
                return Ok(Value::Null);
            }
            "mediaSessionEvent" => {
                let kind = text(1);
                let mut engine = self.media_engine.borrow_mut();
                let ev = match kind {
                    "metadata" => {
                        let payload: Value = serde_json::from_str(text(2)).unwrap_or_default();
                        let title = payload
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let artist = payload
                            .get("artist")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let album = payload
                            .get("album")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        engine
                            .session
                            .set_metadata(wega_protocol::media::MediaMetadata {
                                title,
                                artist,
                                album,
                                artwork: vec![],
                            })
                    }
                    "playbackState" => {
                        let state = match text(2).trim_matches('"') {
                            "playing" => wega_protocol::media::MediaSessionPlaybackState::Playing,
                            "paused" => wega_protocol::media::MediaSessionPlaybackState::Paused,
                            _ => wega_protocol::media::MediaSessionPlaybackState::None,
                        };
                        engine.session.set_playback_state(state)
                    }
                    "positionState" => {
                        let payload: Value = serde_json::from_str(text(2)).unwrap_or_default();
                        let duration = payload
                            .get("duration")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        let playback_rate = payload
                            .get("playbackRate")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(1.0);
                        let position = payload
                            .get("position")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        engine.session.set_position_state(
                            wega_protocol::media::MediaPositionState {
                                duration,
                                playback_rate,
                                position,
                            },
                        )
                    }
                    _ => return Ok(Value::Null),
                };
                let _ = ev;
                return Ok(Value::Null);
            }
            "permissionQuery" => {
                let feature_name = text(1);
                let feature = wega_protocol::permissions::PermissionFeature::parse(feature_name)
                    .ok_or_else(|| format!("Invalid permission feature: {}", feature_name))?;
                let url = self.document.borrow().url().clone();
                let origin = origin_for_url(&url);
                let state = self.permissions.borrow().query(
                    &origin,
                    feature,
                    &self.network.policy.read().unwrap(),
                );
                return Ok(json!(state.as_str()));
            }
            "permissionRequest" => {
                let feature_name = text(1);
                let feature = wega_protocol::permissions::PermissionFeature::parse(feature_name)
                    .ok_or_else(|| format!("Invalid permission feature: {}", feature_name))?;
                let url = self.document.borrow().url().clone();
                let origin = origin_for_url(&url);
                let doc_ctx = wega_protocol::DocumentContext {
                    document: wega_protocol::DocumentId(self.document_generation.get()),
                    revision: wega_protocol::DocumentRevision(self.document_revision),
                };
                let (state, prompt_id) = self.permissions.borrow_mut().request(
                    &origin,
                    doc_ctx,
                    feature,
                    &self.network.policy.read().unwrap(),
                );
                if let Some(id) = prompt_id {
                    let origin_str = match &origin {
                        wega_protocol::authority::Origin::Tuple(s) => s.clone(),
                        wega_protocol::authority::Origin::Opaque => "null".into(),
                    };
                    self.requests
                        .borrow_mut()
                        .permission_prompts
                        .push((id, origin_str, feature));
                }
                return Ok(json!({
                    "state": state.as_str(),
                    "promptId": prompt_id,
                }));
            }
            "notificationCreate" => {
                let id = text(1)
                    .parse::<u64>()
                    .map_err(|_| "Invalid notification id")?;
                let title = text(2);
                let opts_raw = text(3);
                let opts: wega_protocol::permissions::NotificationOptions =
                    serde_json::from_str(opts_raw).unwrap_or_default();
                let url = self.document.borrow().url().clone();
                let origin = origin_for_url(&url);
                let state = self.permissions.borrow().query(
                    &origin,
                    wega_protocol::permissions::PermissionFeature::Notifications,
                    &self.network.policy.read().unwrap(),
                );
                if state != wega_protocol::permissions::PermissionState::Granted {
                    return Err("Notification permission not granted".into());
                }
                self.requests
                    .borrow_mut()
                    .notifications
                    .push((id, title.to_string(), opts));
                return Ok(Value::Null);
            }
            "notificationClose" => {
                let id = text(1)
                    .parse::<u64>()
                    .map_err(|_| "Invalid notification id")?;
                self.requests.borrow_mut().notification_closes.push(id);
                return Ok(Value::Null);
            }
            "wakeLockRequest" => {
                let lock_type = text(1);
                if lock_type != "screen" {
                    return Err("NotSupportedError".into());
                }
                let url = self.document.borrow().url().clone();
                let origin = origin_for_url(&url);
                let origin_str = match origin {
                    wega_protocol::authority::Origin::Tuple(s) => s,
                    wega_protocol::authority::Origin::Opaque => {
                        return Err(
                            "NotAllowedError: opaque origin cannot request wake lock".into()
                        );
                    }
                };
                let state = self.permissions.borrow().query(
                    &wega_protocol::authority::Origin::Tuple(origin_str.clone()),
                    wega_protocol::permissions::PermissionFeature::ScreenWakeLock,
                    &self.network.policy.read().unwrap(),
                );
                if state == wega_protocol::permissions::PermissionState::Denied {
                    return Err("NotAllowedError: screen wake lock permission denied".into());
                }
                let doc_ctx = wega_protocol::DocumentContext {
                    document: wega_protocol::DocumentId(self.document_generation.get()),
                    revision: wega_protocol::DocumentRevision(self.document_revision),
                };
                let lock_id = self
                    .permissions
                    .borrow_mut()
                    .devices
                    .create_wake_lock(origin_str, doc_ctx);
                return Ok(json!(lock_id));
            }
            "wakeLockRelease" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid wake lock id")?;
                self.permissions.borrow_mut().devices.release_wake_lock(id);
                return Ok(Value::Null);
            }
            "deviceGetUserMedia" => {
                let need_audio = text(1) == "true";
                let need_video = text(2) == "true";
                let url = self.document.borrow().url().clone();
                let origin = origin_for_url(&url);
                let origin_str = match origin {
                    wega_protocol::authority::Origin::Tuple(s) => s,
                    wega_protocol::authority::Origin::Opaque => {
                        return Err(
                            "NotAllowedError: opaque origin cannot access media devices".into()
                        );
                    }
                };
                let policy = self.network.policy.read().unwrap();
                if need_video {
                    let state = self.permissions.borrow().query(
                        &wega_protocol::authority::Origin::Tuple(origin_str.clone()),
                        wega_protocol::permissions::PermissionFeature::Camera,
                        &policy,
                    );
                    if state != wega_protocol::permissions::PermissionState::Granted {
                        return Err("NotAllowedError: Camera permission not granted".into());
                    }
                }
                if need_audio {
                    let state = self.permissions.borrow().query(
                        &wega_protocol::authority::Origin::Tuple(origin_str.clone()),
                        wega_protocol::permissions::PermissionFeature::Microphone,
                        &policy,
                    );
                    if state != wega_protocol::permissions::PermissionState::Granted {
                        return Err("NotAllowedError: Microphone permission not granted".into());
                    }
                }
                let doc_ctx = wega_protocol::DocumentContext {
                    document: wega_protocol::DocumentId(self.document_generation.get()),
                    revision: wega_protocol::DocumentRevision(self.document_revision),
                };
                let mut perms = self.permissions.borrow_mut();
                let mut track_descriptors = Vec::new();
                if need_video {
                    let vid = perms.devices.create_track(
                        origin_str.clone(),
                        doc_ctx,
                        crate::permissions::MediaTrackKind::Video,
                        None,
                    );
                    track_descriptors.push(
                        json!({ "id": vid, "kind": "video", "label": "Default Video Camera" }),
                    );
                }
                if need_audio {
                    let aid = perms.devices.create_track(
                        origin_str.clone(),
                        doc_ctx,
                        crate::permissions::MediaTrackKind::Audio,
                        None,
                    );
                    track_descriptors.push(
                        json!({ "id": aid, "kind": "audio", "label": "Default Audio Device" }),
                    );
                }
                return Ok(json!(track_descriptors));
            }
            "deviceTrackStop" => {
                let id = text(1);
                self.permissions.borrow_mut().devices.stop_track(id);
                return Ok(Value::Null);
            }
            "deviceTrackClone" => {
                let id = text(1);
                let cloned_id = self.permissions.borrow_mut().devices.clone_track(id)?;
                return Ok(json!(cloned_id));
            }
            "requestFullscreen" => {
                let now = Instant::now();
                let has_activation = self
                    .activation
                    .get()
                    .map(|until| until > now)
                    .unwrap_or(false);
                if !has_activation {
                    return Err(
                        "TypeError: requestFullscreen requires transient user activation".into(),
                    );
                }
                self.activation.set(None);
                self.requests.borrow_mut().fullscreen.push(true);
                return Ok(Value::Null);
            }
            "exitFullscreen" => {
                self.requests.borrow_mut().fullscreen.push(false);
                return Ok(Value::Null);
            }
            "registerProtocolHandler" => {
                let scheme = text(1);
                let target_url = text(2);
                self.requests.borrow_mut().protocol_registrations.push(
                    wega_protocol::permissions::ProtocolHandlerRegistration {
                        scheme: scheme.to_string(),
                        url: target_url.to_string(),
                    },
                );
                return Ok(Value::Null);
            }
            "openExternalApp" => {
                let target_url = text(1);
                self.requests
                    .borrow_mut()
                    .external_apps
                    .push(target_url.to_string());
                return Ok(Value::Null);
            }
            "workerCheck" => {
                self.workers.borrow().check_script(text(1), &self.csp)?;
                return Ok(Value::Null);
            }
            "workerCreate" => {
                let config: crate::workers::WorkerConfig =
                    serde_json::from_str(text(1)).map_err(|error| error.to_string())?;
                let id = self.workers.borrow_mut().spawn_dedicated(
                    &config,
                    &crate::workers::Environment {
                        network: &self.network,
                        csp: &self.csp,
                        limits: &self.limits,
                    },
                )?;
                return Ok(json!(id.0));
            }
            "workerTerminate" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid worker ID")?;
                self.workers
                    .borrow_mut()
                    .terminate_dedicated(crate::workers::WorkerId(id))?;
                return Ok(Value::Null);
            }
            "workerPoll" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid worker ID")?;
                let events = self
                    .workers
                    .borrow_mut()
                    .poll_dedicated_events(crate::workers::WorkerId(id));
                return Ok(json!(events));
            }
            "sharedWorkerConnect" => {
                let config: crate::workers::WorkerConfig =
                    serde_json::from_str(text(1)).map_err(|error| error.to_string())?;
                self.workers.borrow_mut().connect_shared(
                    &config,
                    &crate::workers::Environment {
                        network: &self.network,
                        csp: &self.csp,
                        limits: &self.limits,
                    },
                )?;
                return Ok(json!(""));
            }
            "serviceWorkerRegister" => {
                let options: Value = serde_json::from_str(text(3)).unwrap_or(Value::Null);
                let document = self.document.borrow().url().clone();
                let script = url::Url::parse(text(1))
                    .map_err(|_| "TypeError: Invalid service worker script URL")?;
                if !self
                    .csp
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .allows_url("worker-src", &script, &document, None)
                {
                    return Err(
                        "SecurityError: Service worker blocked by Content Security Policy".into(),
                    );
                }
                let job = self.hubs.service.register(
                    &self.network,
                    &self.limits,
                    &document,
                    text(1),
                    text(2),
                    options["module"].as_bool().unwrap_or(false),
                )?;
                return Ok(json!(job));
            }
            "serviceWorkerUpdate" => {
                let document = self.document.borrow().url().clone();
                if !text(1).starts_with(&document.origin().ascii_serialization()) {
                    return Err("SecurityError: Registration belongs to another origin".into());
                }
                return Ok(json!(self.hubs.service.update(&self.network, text(1))?));
            }
            "serviceWorkerUnregister" => {
                let document = self.document.borrow().url().clone();
                return Ok(json!(self.hubs.service.unregister(
                    &self.network,
                    &document,
                    text(1)
                )));
            }
            "serviceWorkerPoll" => {
                let document = self.document.borrow().url().clone();
                return Ok(self.hubs.service.poll(
                    &self.network,
                    &self.client_id,
                    &document,
                    self.created,
                ));
            }
            "serviceWorkerControlled" => {
                let document = self.document.borrow().url().clone();
                return Ok(json!(self.hubs.service.controls(&self.network, &document)));
            }
            "serviceWorkerPost" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid worker ID")?;
                let document = self.document.borrow().url().clone();
                self.hubs
                    .service
                    .post(&self.network, id, &self.client_id, &document, text(2))?;
                return Ok(Value::Null);
            }
            "channelCreate" => {
                let (first, second) = self.port_hub().create_channel();
                return Ok(json!({ "port1": first.0, "port2": second.0 }));
            }
            "portPost" => {
                let port = text(1).parse::<u64>().map_err(|_| "Invalid port ID")?;
                self.port_hub().post_message(
                    crate::workers::PortId(port),
                    text(2).to_string(),
                    Vec::new(),
                )?;
                return Ok(Value::Null);
            }
            "portClose" => {
                let port = text(1).parse::<u64>().map_err(|_| "Invalid port ID")?;
                self.port_hub().close(crate::workers::PortId(port));
                return Ok(Value::Null);
            }
            "portPoll" => {
                let port = text(1).parse::<u64>().map_err(|_| "Invalid port ID")?;
                return Ok(json!(
                    self.port_hub().poll_messages(crate::workers::PortId(port))
                ));
            }
            "clipboardWrite" => {
                self.clipboard.borrow_mut().clear();
                self.clipboard.borrow_mut().push_str(text(1));
                return Ok(Value::Null);
            }
            "clipboardWriteText" => {
                // navigator.clipboard.writeText: gated on transient activation
                // and forwarded to the browser, which owns the OS clipboard.
                const MAX_CLIPBOARD_BYTES: usize = 1024 * 1024;
                let now = Instant::now();
                if !self.activation.get().is_some_and(|until| until > now) {
                    return Err(
                        "NotAllowedError: clipboard write requires transient user activation"
                            .into(),
                    );
                }
                if text(1).len() > MAX_CLIPBOARD_BYTES {
                    return Err("NotAllowedError: clipboard text is too large".into());
                }
                self.clipboard.borrow_mut().clear();
                self.clipboard.borrow_mut().push_str(text(1));
                self.requests
                    .borrow_mut()
                    .clipboard_writes
                    .push(text(1).to_string());
                return Ok(Value::Null);
            }
            "clipboardRead" => {
                return Ok(json!(self.clipboard.borrow().clone()));
            }
            "customNameValid" => return Ok(json!(crate::components::valid_name(text(1)))),
            "customDefinition" => return Ok(json!(self.components.borrow().get(text(1)))),
            "customDefinitionId" => {
                return Ok(json!(
                    self.components
                        .borrow()
                        .by_id(text(1).parse().map_err(|_| "Invalid definition ID")?)
                ));
            }
            "customPreflight" => {
                self.components.borrow().preflight(
                    text(1),
                    text(2)
                        .parse()
                        .map_err(|_| "Invalid constructor identity")?,
                    (!text(3).is_empty()).then_some(text(3)),
                )?;
                return Ok(Value::Null);
            }
            "messageTarget" => {
                let doc = self.document.borrow();
                let requested = text(1);
                if requested == "*" || requested == "/" {
                    return Ok(json!(true));
                }
                let target =
                    url::Url::parse(requested).map_err(|_| "SyntaxError: invalid target origin")?;
                let origin = target.origin();
                return Ok(json!(
                    matches!(origin, url::Origin::Tuple(..)) && origin == doc.url().origin()
                ));
            }
            "customDefine" => {
                let options: Value =
                    serde_json::from_str(text(3)).map_err(|_| "Invalid custom element options")?;
                let observed = options
                    .get("observed")
                    .and_then(Value::as_array)
                    .ok_or("Invalid observed attributes")?
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .ok_or("Invalid observed attribute")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(json!(
                    self.components.borrow_mut().define(
                        text(1),
                        text(2)
                            .parse()
                            .map_err(|_| "Invalid constructor identity")?,
                        observed,
                        options.get("extends").and_then(Value::as_str)
                    )?
                ));
            }
            "customReactions" => return Ok(json!(self.components.borrow_mut().drain()?)),
            _ => {}
        }
        if text(0) == "revision" {
            return Ok(json!(self.revision.get()));
        }
        if matches!(
            text(0),
            "templateContent"
                | "scrollViewport"
                | "scrollNode"
                | "scrollIntoView"
                | "history"
                | "innerHTML"
                | "contextualFragment"
                | "clone"
                | "insert"
                | "createNS"
                | "createComment"
                | "createFragment"
                | "createDocument"
                | "setAttrNS"
                | "styleSet"
                | "focus"
                | "setChecked"
                | "setText"
                | "setAttr"
                | "removeAttr"
                | "removeAttrNS"
                | "create"
                | "createText"
                | "append"
                | "remove"
                | "activate"
                | "write"
                | "fragment"
                | "submitForm"
                | "resetForm"
        ) {
            self.invalidate();
        }
        if text(0) == "error" {
            crate::diagnostics::Diagnostic::error(
                crate::diagnostics::Category::Javascript,
                text(1),
                Some(self.document.borrow().url().to_string()),
            )
            .emit();
            let mut reports = self.errors.borrow_mut();
            if reports.len() < 16 {
                reports.push(text(1).chars().take(4096).collect());
            }
            return Ok(Value::Null);
        }
        // Dialogs block this thread on the browser's answer; never hold the DOM.
        if text(0) == "dialog" {
            let document_url = self.document.borrow().url().to_string();
            let mut handler = self.dialog.borrow_mut();
            let answer = handler.as_mut().and_then(|handler| {
                handler(
                    self.document_generation.get(),
                    self.document_revision,
                    &document_url,
                    text(1),
                    &text(2).chars().take(4096).collect::<String>(),
                    text(3),
                )
            });
            return Ok(answer.map_or(Value::Null, Value::String));
        }
        if text(0) == "cspInline" {
            return Ok(json!(
                self.csp
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .allows_inline_script(Some(text(1)), text(2))
            ));
        }
        if text(0) == "cspConnect" {
            let document = self.document.borrow().url().clone();
            let Ok(url) = document.join(text(1)) else {
                return Ok(json!(false));
            };
            let allowed = self
                .csp
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .allows_url("connect-src", &url, &document, None)
                && !crate::network::mixed_content(Some(&document), &url, false);
            return Ok(json!(allowed));
        }
        if text(0) == "platform" {
            return Ok(json!(match std::env::consts::OS {
                "macos" => "MacIntel",
                "windows" => "Win32",
                "linux" => "Linux x86_64",
                other => other,
            }));
        }
        if text(0) == "historyPosition" {
            let index = text(1)
                .parse::<usize>()
                .map_err(|_| "Invalid history index")?;
            let length = text(2)
                .parse::<usize>()
                .map_err(|_| "Invalid history length")?;
            if index < length {
                self.history.set((index, length));
            }
            return Ok(Value::Null);
        }
        if text(0) == "traverseHost" {
            let delta = text(1)
                .parse::<i32>()
                .map_err(|_| "Invalid history delta")?;
            let mut requests = self.requests.borrow_mut();
            if requests.traverse.len() < 8 {
                requests.traverse.push(delta);
            }
            return Ok(Value::Null);
        }
        let charge = match text(0) {
            "setText" => text(2).len() + 32,
            "setAttr" => text(2).len() + text(3).len() + 32,
            "create" | "createText" | "createComment" | "createFragment" => text(1).len() + 32,
            "createNS" => text(1).len() + text(2).len() + 32,
            "innerHTML" | "contextualFragment" => text(2).len() + 32,
            "write" => text(1).len(),
            "styleSet" => text(2).len() + text(3).len() + 32,
            _ => 0,
        };
        let next = self.used.get().saturating_add(charge);
        if next > self.limits.dom_text_bytes {
            return Err("DOM text allocation budget exceeded".into());
        }
        self.used.set(next);
        let id = |index: usize| {
            text(index)
                .parse::<u64>()
                .map(NodeId::from_u64)
                .map_err(|_| "Invalid node identity".to_string())
        };
        let mut doc = self
            .document
            .try_borrow_mut()
            .map_err(|_| "DOM is currently busy".to_string())?;
        let node_id = || -> Result<NodeId, String> {
            let id = id(1)?;
            if doc.node(id.key()).is_none() {
                Err("Node no longer exists".into())
            } else {
                Ok(id)
            }
        };
        let value = match text(0) {
            "customUpgrade" => {
                let target = node_id()?;
                let name = doc.node(target.key()).and_then(NodeView::element_name).map_or("", |name| name.local);
                json!(self.components.borrow().needs_upgrade(target.as_u64(), name))
            },
            "customCandidates" => {
                let root = node_id()?;
                let definition: u64 = text(2).parse().map_err(|_| "Invalid definition ID")?;
                let registry = self.components.borrow();
                let candidates: Vec<_> = self.component_subtree(&doc, root).into_iter().filter(|id| {
                    let name = doc.node((*id).key()).and_then(NodeView::element_name).map_or("", |name| name.local);
                    registry.needs_upgrade(id.as_u64(), name) == Some(definition)
                }).map(|id| id.as_u64().to_string()).collect();
                json!(candidates)
            },
            "customBegin" => {
                let target = node_id()?;
                let definition: u64 = text(2).parse().map_err(|_| "Invalid definition ID")?;
                let mut registry = self.components.borrow_mut();
                let name = doc.node(target.key()).and_then(NodeView::element_name).map(|name| name.local).ok_or("Custom element must be an element")?;
                if registry.get(name).is_none_or(|entry| entry.id != definition) { return Err("Custom element definition does not match its node".into()); }
                let began = registry.begin(target.as_u64(), definition)?;
                if began {
                    for (name, value) in doc.node(target.key()).into_iter().flat_map(NodeView::attributes) {
                        registry.attribute(target.as_u64(), name.local.as_ref(), None, Some(value), (!name.ns.as_str().is_empty()).then_some(name.ns.as_str()));
                    }
                    if self.component_connected(&doc, target) { registry.lifecycle(target.as_u64(), true); }
                }
                json!(began)
            },
            "customComplete" => {
                let target = node_id()?; self.components.borrow_mut().complete(target.as_u64(), text(2) == "true")?; Value::Null
            },
            "customConstruct" => {
                let definition: u64 = text(1).parse().map_err(|_| "Invalid definition ID")?;
                let name = self.components.borrow().by_id(definition).ok_or("TypeError: Illegal constructor")?.name.clone();
                if self.allocated.get() >= self.limits.dom_nodes { return Err("DOM node limit exceeded".into()); }
                let created = (wega_dom::mutate(&mut *doc).create_element(QualName::new(None, ns!(html), LocalName::from(name)), [])).node_id();
                self.allocated.set(self.allocated.get()+1); self.invalidate();
                let mut registry = self.components.borrow_mut(); registry.begin(created.as_u64(), definition)?; registry.complete(created.as_u64(), true)?;
                json!(created.as_u64().to_string())
            },
            "customConnected" | "customDisconnected" => {
                let target = node_id()?; let nodes = self.component_subtree(&doc, target); let mut registry = self.components.borrow_mut();
                for id in nodes { registry.lifecycle(id.as_u64(), text(0) == "customConnected"); } Value::Null
            },
            "customAdopt" => {
                let target = node_id()?; let old = id(2)?; let new = id(3)?;
                if !doc.node(old.key()).is_some_and(|node|node.kind()==NodeKind::Document) || !doc.node(new.key()).is_some_and(|node|node.kind()==NodeKind::Document) { return Err("Invalid adoption documents".into()); }
                let mut registry = self.components.borrow_mut(); for child in self.component_subtree(&doc, target) { registry.adopted(child.as_u64(), old.as_u64(), new.as_u64()); } Value::Null
            },
            "shadowAttach" => {
                let target = node_id()?;
                if self.shadows.borrow().root(target.as_u64(), true).is_some() { return Err("NotSupportedError: shadow root already attached".into()); }
                let tag = doc.node(target.key()).and_then(NodeView::element_name).ok_or("NotSupportedError: invalid shadow host")?.local.to_string();
                if self.allocated.get() >= self.limits.dom_nodes { return Err("DOM node limit exceeded".into()); }
                let root = (wega_dom::mutate(&mut *doc).create_element(QualName::new(None, ns!(html), LocalName::from("#document-fragment")), [])).node_id();
                let entry = match self.shadows.borrow_mut().attach(target.as_u64(), root.as_u64(), text(2), text(3) == "true", &tag) {
                    Ok(entry) => entry, Err(error) => { let _ = wega_dom::mutate(&mut *doc).remove_and_drop(root.key()); return Err(error); }
                };
                self.allocated.set(self.allocated.get()+1); self.invalidate();
                self.requests.borrow_mut().compatibility = Some("Shadow DOM scoped style and flattened layout are not implemented by Wega".into());
                json!(entry)
            },
            "shadowRoot" => json!(self.shadows.borrow().root(node_id()?.as_u64(), false)),
            "shadowInternal" => json!(self.shadows.borrow().root(node_id()?.as_u64(), true)),
            "shadowHost" => json!(self.shadows.borrow().host(node_id()?.as_u64()).map(|id|id.to_string())),
            "eventParent" => {
                let target = node_id()?;
                if let Some(slot) = self.assigned_slot(&doc, target) { json!(slot.as_u64().to_string()) }
                else if let Some(parent) = wega_dom::DocumentView::node(&*doc, target.key()).and_then(wega_dom::NodeView::parent) { json!(parent.to_string()) }
                else if text(2) != "true" && text(3) == text(1) { Value::Null }
                else { json!(self.shadows.borrow().host(target.as_u64()).map(|id|id.to_string())) }
            },
            "assignedSlot" | "assignedSlotInternal" => {
                let mut assigned = self.assigned_slot(&doc, node_id()?);
                if text(0) == "assignedSlot" && let Some(slot) = assigned {
                        let root = wega_dom::tree_root(&*doc, slot.key());
                        if let Some(host) = self.shadows.borrow().host(root.as_u64()) && self.shadows.borrow().root(host, false).is_none() { assigned = None; }
                }
                json!(assigned.map(|id|id.as_u64().to_string()))
            },
            "componentRoot" => {
                let mut root = node_id()?;
                loop {
                    if let Some(parent) = doc.node(root.key()).and_then(NodeView::parent) { root = parent.node_id(); }
                    else if text(2) == "true" && let Some(host) = self.shadows.borrow().host(root.as_u64()) { root = NodeId::from_u64(host); }
                    else { break; }
                }
                json!(root.as_u64().to_string())
            },
            "assignedNodes" => {
                let nodes = self.assigned_nodes(&doc, node_id()?, text(2) == "true", &mut HashSet::new());
                json!(nodes.into_iter().map(|id|id.as_u64().to_string()).collect::<Vec<_>>())
            },
            "cookie"=>json!(self.network.cookies.get(doc.url(),true,crate::cookies::SiteContext::SameSite)),
            "setCookie"=>{self.network.cookies.set(doc.url(),text(1),true,crate::cookies::SiteContext::SameSite);Value::Null},
            "templateContent"=>{let target=node_id()?;let content=wega_dom::mutate(&mut *doc).template_contents(target.key()).map_err(Self::mutation_error)?;json!(content.node_id().as_u64().to_string())},
            "viewport"=>{let viewport=doc.viewport();let scale=(viewport.hidpi_scale*viewport.zoom) as f64;let scroll=doc.viewport_scroll();json!({"width":viewport.window_size.0 as f64/scale,"height":viewport.window_size.1 as f64/scale,"dpr":scale,"scrollX":scroll.x,"scrollY":scroll.y})},
            "scroll"=>{let node=doc.get_node(node_id()?).unwrap();let scroll=node.scroll_offset();json!({"x":scroll.x,"y":scroll.y})},
            "scrollViewport"=>{let x=text(1).parse::<f64>().map_err(|_|"Invalid scroll offset")?;let y=text(2).parse::<f64>().map_err(|_|"Invalid scroll offset")?;if x.is_finite()&&y.is_finite(){doc.set_viewport_scroll(blitz_dom::Point{x,y});}Value::Null},
            "scrollNode"=>{let target=node_id()?;let x=text(2).parse::<f64>().map_err(|_|"Invalid scroll offset")?;let y=text(3).parse::<f64>().map_err(|_|"Invalid scroll offset")?;if x.is_finite()&&y.is_finite(){doc.scroll_to(target,x,y,blitz_dom::ScrollBehavior::Instant);}Value::Null},
            "scrollIntoView"=>{let target=node_id()?;doc.scroll_into_view(target,blitz_dom::ScrollBehavior::Instant,blitz_dom::ScrollLogicalPosition::Start,blitz_dom::ScrollLogicalPosition::Nearest);Value::Null},
            "active"=>json!(wega_dom::input::FocusState::keyboard_target(&*doc).map(|key|key.to_string())),
            "history"=>{let url=doc.url().join(text(1)).map_err(|e|e.to_string())?;if url.origin()!=doc.url().origin()||!matches!(url.scheme(),"http"|"https"){return Err("SecurityError: history URL must have the same tuple origin".into());}doc.set_base_url(url.as_str());Value::Null},
            "media"=>{
                let url_data=style::stylesheets::UrlExtraData(style::servo_arc::Arc::new(doc.url().clone()));
                let mut context=style::parser::ParserContext::new(style::stylesheets::Origin::Author,&url_data,Some(style::stylesheets::CssRuleType::Media),style_traits::ParsingMode::DEFAULT,style::context::QuirksMode::NoQuirks,std::borrow::Cow::Owned(Default::default()),None,None,Default::default());
                let mut input=cssparser::Parser::new(text(1));let media=style::media_queries::MediaList::parse(&mut context,&mut input);
                json!(media.evaluate(doc.stylist_device(),style::context::QuirksMode::NoQuirks,&mut style::stylesheets::CustomMediaEvaluator::none()))
            },
            "subtree"=>json!(self.component_subtree(&doc,node_id()?).iter().map(|id|id.as_u64().to_string()).collect::<Vec<_>>()),
            "documentDomain"=>{
                if doc.url().origin().is_tuple() { Value::Null }
                else {return Err("SecurityError: opaque documents have no domain".into());}
            },
            "resolveURL"=>{let base=url::Url::parse(text(2)).map_err(|e|e.to_string())?;let url=base.join(text(1)).map_err(|e|e.to_string())?;json!({"href":url.as_str(),"origin":url.origin().ascii_serialization(),"protocol":format!("{}:",url.scheme()),"host":url.host_str().map_or(String::new(),|host|url.port().map_or_else(||host.to_owned(),|port|format!("{host}:{port}"))),"hostname":url.host_str().unwrap_or_default(),"port":url.port().map(|p|p.to_string()).unwrap_or_default(),"pathname":url.path(),"search":url.query().map(|q|format!("?{q}")).unwrap_or_default(),"hash":url.fragment().map(|f|format!("#{f}")).unwrap_or_default()})},
            "connected"=>json!(self.component_connected(&doc, node_id()?)),
            "retain" => {let id=node_id()?;self.retained.borrow_mut().insert(id);Value::Null},
            "release" => {let id=id(1)?;self.retained.borrow_mut().remove(&id);self.collect_detached(&mut doc,id);Value::Null},
            "info"=>{
                let info=wega_dom::node_info(&*doc,node_id()?.key()).ok_or("InvalidStateError: node no longer exists")?;
                json!({"nodeType":info.node_type,"nodeName":info.node_name,"localName":info.local_name,"namespaceURI":info.namespace,"prefix":info.prefix})
            },
            "attributes"=>json!(doc.node(node_id()?.key()).into_iter().flat_map(NodeView::attributes).map(|(name,value)|json!({"name":name.prefix.as_ref().map_or_else(||name.local.to_string(),|p|format!("{p}:{}",name.local)),"localName":name.local.to_string(),"namespaceURI":if name.ns==ns!(){None}else{Some(name.ns.to_string())},"value":value})).collect::<Vec<_>>()),
            "queryIn"=>json!(doc.query_selector_in(node_id()?,text(2)).map_err(|_|"Invalid selector")?.map(|id|id.as_u64().to_string())),
            "queryAllIn"=>json!(doc.query_selector_all_in(node_id()?,text(2)).map_err(|_|"Invalid selector")?.iter().map(|id|id.as_u64().to_string()).collect::<Vec<_>>()),
            "matches"=>json!(doc.matches_selector(node_id()?,text(2)).map_err(|_|"Invalid selector")?),
            "closest"=>json!(doc.closest(node_id()?,text(2)).map_err(|_|"Invalid selector")?.map(|id|id.as_u64().to_string())),
            "getById"=>json!(wega_dom::DocumentView::element_by_id(&*doc,text(1)).map(|key|key.to_string())),
            "outerHTML"=>json!(doc.get_node(node_id()?).unwrap().outer_html()),
            "innerHTML" | "contextualFragment"=>{
                let mut target=node_id()?;
                if doc.node(target.key()).and_then(NodeView::element_name).is_some_and(|name|name.local=="template") { target=wega_dom::mutate(&mut *doc).template_contents(target.key()).map(ToNodeId::node_id).map_err(Self::mutation_error)?; }
                if text(2).len()>self.limits.html_bytes{return Err("HTML fragment exceeds the size limit".into());}
                // Parse into a detached same-context element first; reject budgets before
                // replacing live children, and retain removed nodes held by JS wrappers.
                let element=doc.node(target.key()).and_then(NodeView::qual_name).ok_or("innerHTML needs an element")?;
                let name=if &*element.local=="#document-fragment" {QualName::new(None,ns!(html),LocalName::from("div"))} else {element.clone()};let staging:NodeId=wega_dom::mutate(&mut *doc).create_element(name,[]).node_id();
                let remaining=self.limits.dom_nodes.saturating_sub(self.allocated.get()).saturating_add(2);
                if wega_html::parse_fragment(&mut *doc,staging.key(),remaining,text(2),&crate::html_report::HtmlReport).is_err() {let _=wega_dom::mutate(&mut *doc).remove_and_drop(staging.key());return Err("DOM node limit exceeded".into());}
                let parsed=Self::subtree(&doc,staging);
                // innerHTML scripts stay inert; Range fragment scripts can run on insertion.
                if text(0)=="innerHTML" {for id in &parsed {if doc.node((*id).key()).and_then(NodeView::element_name).is_some_and(|name|name.local=="script"){self.started_scripts.borrow_mut().insert(*id);}}}
                if self.allocated.get().saturating_add(parsed.len())>self.limits.dom_nodes {wega_dom::mutate(&mut *doc).remove_and_drop(staging.key()).map_err(Self::mutation_error)?;return Err("DOM node limit exceeded".into());}
                let children:Vec<wega_dom::NodeKey>=doc.node(staging.key()).map(|node|node.children().collect()).unwrap_or_default();
                if text(0)=="contextualFragment" {
                    let mut mutation=wega_dom::mutate(&mut *doc);let fragment=mutation.create_element(QualName::new(None,ns!(html),LocalName::from("#document-fragment")),[]);
                    mutation.replace_children(fragment,&children).map_err(Self::mutation_error)?;mutation.remove_and_drop(staging.key()).map_err(Self::mutation_error)?;mutation.finish();let fragment:NodeId=fragment.node_id();
                    self.allocated.set(self.allocated.get()+parsed.len());
                    return Ok(json!(fragment.as_u64().to_string()));
                }
                let old:Vec<NodeId>=doc.node(target.key()).map(|node|node.children().map(ToNodeId::node_id).collect()).unwrap_or_default();
                {let mut mutation=wega_dom::mutate(&mut *doc);mutation.replace_children(target.key(),&children).map_err(Self::mutation_error)?;mutation.remove_and_drop(staging.key()).map_err(Self::mutation_error)?;}
                self.allocated.set(self.allocated.get()+parsed.len()-1);
                for id in old {self.collect_detached(&mut doc,id);}
                Value::Null
            },
            "clone"=>{
                let source=node_id()?;let deep=text(2)=="true";let count=if deep{Self::subtree(&doc,source).len()}else{1};
                if self.allocated.get().saturating_add(count)>self.limits.dom_nodes{return Err("DOM node limit exceeded".into());}
                let bytes=doc.get_node(source).unwrap().outer_html().len();
                if self.used.get().saturating_add(bytes)>self.limits.dom_text_bytes{return Err("DOM text allocation budget exceeded".into());}
                let cloned=(wega_dom::mutate(&mut *doc).clone_node(source.key(),deep).map_err(|error|match error{wega_dom::MutationError::NotCloneable(_)=>"This node cannot be cloned".to_string(),other=>Self::mutation_error(other)})?).node_id();
                self.allocated.set(self.allocated.get()+count);self.used.set(self.used.get()+bytes);json!(cloned.as_u64().to_string())
            },
            "insert"=>{
                let parent=node_id()?;let child=id(2)?;
                if doc.node(child.key()).is_none(){return Err("Node no longer exists".into());}
                let anchor=if text(3).is_empty(){None}else{Some(id(3)?)};
                if anchor.is_some_and(|id|doc.get_node(id).is_none_or(|n|n.parent!=Some(parent))){return Err("NotFoundError: reference is not a child".into());}
                let mut ancestor=Some(parent);while let Some(id)=ancestor {if id==child{return Err("HierarchyRequestError: DOM cycle".into());}ancestor=doc.node(id.key()).and_then(NodeView::parent).map(ToNodeId::node_id).or_else(||self.shadows.borrow().host(id.as_u64()).map(NodeId::from_u64));}
                if anchor==Some(child){return Ok(json!(child.as_u64().to_string()));}
                {let mut mutation=wega_dom::mutate(&mut *doc);if let Some(anchor)=anchor{mutation.insert_before(anchor.key(),&[child.key()]).map_err(Self::mutation_error)?;}else{mutation.append_children(parent.key(),&[child.key()]).map_err(Self::mutation_error)?;}}
                json!(child.as_u64().to_string())
            },
            "createNS"|"createComment"|"createFragment"|"createDocument"=>{
                if self.allocated.get()>=self.limits.dom_nodes{return Err("DOM node limit exceeded".into());}
                let created=match text(0){
                    "createDocument"=>doc.create_node(NodeData::Document(Box::default())),
                    "createComment"=>(wega_dom::mutate(&mut *doc).create_comment(text(1))).node_id(),
                    "createFragment"=>(wega_dom::mutate(&mut *doc).create_element(QualName::new(None,ns!(html),LocalName::from("#document-fragment")),[])).node_id(),
                    _=>{let parts=text(2).split_once(':');let (prefix,local)=parts.map_or((None,text(2)),|(p,l)|(Some(html5ever::Prefix::from(p)),l));(wega_dom::mutate(&mut *doc).create_element(QualName::new(prefix,html5ever::Namespace::from(text(1)),LocalName::from(local)),[])).node_id()}
                };self.allocated.set(self.allocated.get()+1);json!(created.as_u64().to_string())
            },
            "attrNS"=>json!(wega_dom::attribute_ns(&*doc,node_id()?.key(),text(2),text(3))),
            "setAttrNS"=>{
                let target=node_id()?;let payload=[text(3).to_owned(),text(4).to_owned()];
                let (prefix,local)=payload[0].split_once(':').map_or((None,payload[0].as_str()),|(p,l)|(Some(html5ever::Prefix::from(p)),l));
                let charge=payload[0].len()+payload[1].len();if self.used.get().saturating_add(charge)>self.limits.dom_text_bytes{return Err("DOM text allocation budget exceeded".into());}self.used.set(self.used.get()+charge);
                let old = wega_dom::attribute_ns(&*doc,target.key(),text(2),local);
                wega_dom::mutate(&mut *doc).set_attribute(target.key(),QualName::new(prefix,html5ever::Namespace::from(text(2)),LocalName::from(local)),&payload[1]).map_err(Self::mutation_error)?;
                self.components.borrow_mut().attribute(target.as_u64(), local, old.as_deref(), Some(&payload[1]), (!text(2).is_empty()).then_some(text(2)));
                Value::Null
            },
            "supports"=>json!(doc.css_supports_condition(text(1))),
            "supportsProperty"=>json!(doc.style_attr_set_property("",text(1),text(2),false).is_some_and(|css|!css.is_empty())),
            "inlineCSS"=>{let target=node_id()?;let mut css=String::new();if let Some(block)=doc.get_node(target).unwrap().element_data().and_then(|e|e.style_attribute.as_ref()){let guard=doc.guard().read();let _=block.read_with(&guard).to_css(&mut css);}json!(css)},
            "inlineProperty"=>{let target=node_id()?;let mut css=String::new();if let Ok(property)=style::properties::PropertyId::parse_enabled_for_all_content(text(2)) && let Some(block)=doc.get_node(target).unwrap().element_data().and_then(|e|e.style_attribute.as_ref()){let guard=doc.guard().read();let _=block.read_with(&guard).property_value_to_css(&property,&mut css);}json!(css)},
            "styleSet"=>{
                let target=node_id()?;let old=Self::attribute(&doc,target,"style");wega_dom::mutate(&mut *doc).remove_style_property(target.key(),text(2)).map_err(Self::mutation_error)?;
                let mut css=String::new();if let Some(block)=doc.get_node(target).unwrap().element_data().and_then(|e|e.style_attribute.as_ref()){let guard=doc.guard().read();let _=block.read_with(&guard).to_css(&mut css);}
                if !text(3).is_empty(){css.push_str(&format!("{}: {};",text(2),text(3)));}
                wega_dom::mutate(&mut *doc).set_attribute(target.key(),QualName::new(None,ns!(),LocalName::from("style")),&css).map_err(Self::mutation_error)?;
                self.components.borrow_mut().attribute(target.as_u64(),"style",old.as_deref(),Some(&css),None);Value::Null
            },
            "inputValue"=>{let target=node_id()?;let element=doc.get_node(target).unwrap().element_data().ok_or("Input needs an element")?;json!(element.text_input_data().map(|data|data.editor.text().to_string()).or_else(||element.attr(LocalName::from("value")).map(str::to_owned)).unwrap_or_default())},
            "checked"=>json!(doc.get_node(node_id()?).unwrap().element_data().and_then(|e|e.checkbox_input_checked()).unwrap_or(false)),
            "setChecked"=>{
                let target=node_id()?;
                self.resolve(&mut doc);
                let checked=text(2)=="true";
                doc.snapshot_node_and(target,style_dom::ElementState::CHECKED,|node|{
                    if let Some(element)=node.element_data_mut(){element.set_checkbox_input_checked(checked);}
                    node.mark_ancestors_dirty();
                });
                Value::Null
            },
            // Blitz creates native text editors during layout. Initialize a
            // newly inserted input before focus so immediate key/IME commands
            // cannot be silently dropped while waiting for the next paint.
            "focus"=>{let target=node_id()?;self.resolve(&mut doc);let _=wega_dom::input::InputControl::set_focus(&mut *doc,target.key());self.invalidate();Value::Null},
            "serialize"=>{let mut html=String::new();Self::serialize(&doc,node_id()?,&mut html);json!(html)},
            "computed"=>{
                let target=node_id()?;self.resolve(&mut doc);
                json!(doc.resolved_style_value(target,text(2)))
            },
            "limits" => json!({"resourceBytes":self.limits.resource_bytes}),
            "windowNamed" => json!(blitz_dom::traversal::TreeTraverser::new(&doc).filter(|id| {
                doc.node((*id).key()).is_some_and(|node| node.element_name().is_some_and(|element| {
                    NodeView::attr(node,"id")==Some(text(1)) || (matches!(element.local,"form"|"img"|"embed"|"object") && NodeView::attr(node,"name")==Some(text(1)))
                }))
            }).map(|id|id.as_u64().to_string()).collect::<Vec<_>>()),
            "root" => json!(doc.root_node().id.as_u64().to_string()),
            "query" => json!(
                doc.query_selector(text(1))
                    .map_err(|_| "Invalid selector")?
                    .map(|id| id.as_u64().to_string())
            ),
            "queryAll" => json!(
                doc.query_selector_all(text(1))
                    .map_err(|_| "Invalid selector")?
                    .iter()
                    .map(|id| id.as_u64().to_string())
                    .collect::<Vec<_>>()
            ),
            "text" => {let node=doc.get_node(node_id()?).unwrap();match &node.data {NodeData::Document(_)=>Value::Null,NodeData::Comment{contents}=>json!(contents),_=>json!(node.text_content())}},
            "tag" => json!(
                doc.node(node_id()?.key()).and_then(NodeView::element_name).map(|name| name.local.to_uppercase())
            ),
            "firstChild" | "lastChild" | "nextSibling" | "previousSibling" => {
                let target=wega_dom::NodeKey::from_u64(node_id()?.as_u64());
                let related=match text(0) {
                    "firstChild"=>doc.node(target).and_then(|node|node.children().next()),
                    "lastChild"=>doc.node(target).and_then(|node|node.children().next_back()),
                    other=>wega_dom::sibling(&*doc,target,other=="nextSibling")
                };
                json!(related.map(|key| key.to_string()))
            },
            "childCount" => json!(doc.node(node_id()?.key()).map_or(0, |node| node.children().len())),
            "childAt" => {
                let index=text(2).parse::<usize>().unwrap_or(usize::MAX);
                json!(doc.node(node_id()?.key()).and_then(|node| node.child_at(index)).map(|key| key.to_string()))
            },
            "children" => json!(
                doc.node(node_id()?.key())
                    .map(|node| node.children().map(|key| key.to_string()).collect::<Vec<_>>())
                    .unwrap_or_default()
            ),
            "parent" => json!(doc.node(node_id()?.key()).and_then(NodeView::parent).map(|key| key.to_string())),
            "box"=>{
                let target=node_id()?;self.resolve(&mut doc);let node=doc.get_node(target).unwrap();let layout=node.final_layout();let offset=doc.offset_rect(target).unwrap_or(blitz_dom::BoundingRect{x:0.0,y:0.0,width:0.0,height:0.0});
                json!({"hasBoxes":node.has_boxes(),"clientWidth":node.client_width(),"clientHeight":node.client_height(),"clientLeft":layout.border.left,"clientTop":layout.border.top,"offsetWidth":offset.width,"offsetHeight":offset.height,"offsetLeft":offset.x,"offsetTop":offset.y,"offsetParent":node.offset_parent().map(|node|node.id.as_u64().to_string()),"scrollWidth":node.scroll_width(),"scrollHeight":node.scroll_height(),"contentWidth":layout.content_box_width(),"contentHeight":layout.content_box_height(),"paddingLeft":layout.padding.left,"paddingTop":layout.padding.top})
            },
            "clientRects"=>{
                let target=node_id()?;self.resolve(&mut doc);
                json!(doc.node_client_rects(target).into_iter().map(|r|json!({"x":r.x,"y":r.y,"width":r.width,"height":r.height})).collect::<Vec<_>>())
            },
            "rect" => {
                let id = node_id()?;
                self.resolve(&mut doc);
                let node = doc.get_node(id).unwrap();
                if node.element_data().is_none() {return Ok(json!({"x":0,"y":0,"width":0,"height":0,"left":0,"right":0,"top":0,"bottom":0}));}
                let r=doc.get_client_bounding_rect(id).unwrap_or(blitz_dom::BoundingRect{x:0.0,y:0.0,width:0.0,height:0.0});
                json!({"x":r.x,"y":r.y,"width":r.width,"height":r.height,"left":r.x,"top":r.y,"right":r.x+r.width,"bottom":r.y+r.height})
            }
            "attr" => json!(
                doc.get_node(node_id()?)
                    .unwrap()
                    .element_data()
                    .and_then(|e| e.attr(LocalName::from(text(2))))
                    .map(str::to_owned)
            ),
            "setText" => {
                let id = node_id()?;
                if text(2).len() > 1024 * 1024 {
                    return Err("DOM text budget exceeded".into());
                }
                let credit = |old: usize| self.used.set(self.used.get().saturating_sub(old));
                match &mut doc.get_node_mut(id).unwrap().data {
                    NodeData::Comment { contents } => {
                        credit(contents.len());
                        *contents = text(2).to_owned();
                        return Ok(Value::Null);
                    }
                    NodeData::Document(_) => return Ok(Value::Null),
                    NodeData::Text(data) => {
                        credit(data.content.len());
                        wega_dom::mutate(&mut *doc)
                            .set_text(id.key(), text(2))
                            .map_err(Self::mutation_error)?;
                        return Ok(Value::Null);
                    }
                    _ => {}
                }
                // Replacing an element's children: only a new text child is a new node,
                // and the old children are collected (crediting their budget) unless
                // script still holds them.
                let old: Vec<NodeId> = doc.node(id.key()).map(|node| node.children().map(ToNodeId::node_id).collect()).unwrap_or_default();
                if text(2).is_empty() {
                    wega_dom::mutate(&mut *doc)
                        .replace_children(id.key(), &[])
                        .map_err(Self::mutation_error)?;
                } else {
                    if self.allocated.get() >= self.limits.dom_nodes {
                        return Err("DOM text or node budget exceeded".into());
                    }
                    self.allocated.set(self.allocated.get() + 1);
                    let mut mutation = wega_dom::mutate(&mut *doc);
                    let child = mutation.create_text(text(2));
                    mutation
                        .replace_children(id.key(), &[child])
                        .map_err(Self::mutation_error)?;
                }
                for child in old {
                    self.collect_detached(&mut doc, child);
                }
                Value::Null
            }
            "setAttr" => {
                let id = node_id()?;
                let old = Self::attribute(&doc, id, text(2));
                if text(3).len() > 1024 * 1024 {
                    return Err("DOM attribute budget exceeded".into());
                }
                wega_dom::mutate(&mut *doc)
                    .set_attribute(
                        id.key(),
                        QualName::new(None, ns!(), LocalName::from(text(2))),
                        text(3),
                    )
                    .map_err(Self::mutation_error)?;
                self.components.borrow_mut().attribute(id.as_u64(), text(2), old.as_deref(), Some(text(3)), None);
                Value::Null
            }
            "removeAttr" => {
                let id = node_id()?;
                let old = Self::attribute(&doc, id, text(2));
                wega_dom::mutate(&mut *doc)
                    .clear_attribute(id.key(), QualName::new(None, ns!(), LocalName::from(text(2))))
                    .map_err(Self::mutation_error)?;
                if old.is_some() { self.components.borrow_mut().attribute(id.as_u64(), text(2), old.as_deref(), None, None); }
                Value::Null
            }
            "removeAttrNS" => {
                let target = node_id()?;
                let name = doc.node(target.key()).into_iter().flat_map(NodeView::attributes)
                    .find(|(name, _)| name.ns.as_str() == text(2) && name.local.as_str() == text(3))
                    .map(|(name, value)| (name.clone(), value.to_owned()));
                if let Some((name, old)) = name {
                    wega_dom::mutate(&mut *doc).clear_attribute(target.key(), name).map_err(Self::mutation_error)?;
                    self.components.borrow_mut().attribute(target.as_u64(), text(3), Some(&old), None, (!text(2).is_empty()).then_some(text(2)));
                }
                Value::Null
            }
            "create" | "createText" => {
                if self.allocated.get() >= self.limits.dom_nodes || text(1).len() > 1024 * 1024 {
                    return Err("DOM node budget exceeded".into());
                }
                self.allocated.set(self.allocated.get() + 1);
                let mut mutation = wega_dom::mutate(&mut *doc);
                let id = if text(0) == "create" {
                    mutation.create_element(
                        QualName::new(None, ns!(html), LocalName::from(text(1))),
                        [],
                    )
                } else {
                    mutation.create_text(text(1))
                };
                json!(id.to_string())
            }
            "append" => {
                let parent = node_id()?;
                let child = id(2)?;
                if doc.node(child.key()).is_none() {
                    return Err("Node no longer exists".into());
                }
                // Reject cycles, including putting a node below itself.
                let mut ancestor = Some(parent);
                while let Some(id) = ancestor {
                    if id == child {
                        return Err("Cannot create a DOM cycle".into());
                    }
                    ancestor = doc.node(id.key()).and_then(NodeView::parent).map(ToNodeId::node_id);
                }
                wega_dom::mutate(&mut *doc)
                    .append_children(parent.key(), &[child.key()])
                    .map_err(Self::mutation_error)?;
                json!(child.as_u64().to_string())
            }
            "remove" => {
                let id = node_id()?;
                wega_dom::mutate(&mut *doc)
                    .remove(id.key())
                    .map_err(Self::mutation_error)?;
                Value::Null
            }
            "scriptInserted" => {
                let node = node_id()?;
                let is_script = doc.node(node.key()).and_then(NodeView::element_name).is_some_and(|name| name.local == "script");
                let mut requests = self.requests.borrow_mut();
                if is_script && requests.scripts.len() < 1024 {
                    requests.scripts.push_back(ScriptRequest { node, force_async: text(2) != "false" });
                }
                Value::Null
            }
            // Marks a script element "already started"; false if it was.
            "scriptStart" => json!(self.started_scripts.borrow_mut().insert(node_id()?)),
            "activate" => {
                let node = node_id()?;
                let mut requests = self.requests.borrow_mut();
                if requests.activate.len() < 64 {
                    requests.activate.push(node);
                }
                Value::Null
            }
            "write" => {
                let mut requests = self.requests.borrow_mut();
                if requests.writes.len().saturating_add(text(1).len()) > self.limits.html_bytes {
                    return Err("document.write exceeds the HTML size limit".into());
                }
                requests.writes.push_str(text(1));
                Value::Null
            }
            "open" => {
                let url = doc.url().join(text(1)).map_err(|e| e.to_string())?;
                let active = self.activation.get().is_some_and(|at| at.elapsed() < Duration::from_secs(5));
                let mut requests = self.requests.borrow_mut();
                if !active || !matches!(url.scheme(), "http" | "https") || requests.opened.len() >= 4 {
                    return Ok(json!(false));
                }
                // One popup per user activation.
                self.activation.set(None);
                requests.opened.push(url);
                json!(true)
            }
            "fragment" => {
                let url = doc.url().join(text(1)).map_err(|e| e.to_string())?;
                let mut without = url.clone();
                without.set_fragment(None);
                let mut current = doc.url().clone();
                current.set_fragment(None);
                if without != current {
                    return Err("Fragment navigation must stay in the document".into());
                }
                doc.set_base_url(url.as_str());
                let fragment = url.fragment().unwrap_or_default().to_owned();
                self.resolve(&mut doc);
                if fragment.is_empty() {
                    doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 0.0 });
                } else {
                    doc.scroll_to_fragment(&fragment);
                }
                Value::Null
            }
            "formEntries" => {
                let form = node_id()?;
                let submitter = if text(2).is_empty() { None } else { Some(id(2)?) };
                json!(Self::form_entries(&doc, form, submitter))
            }
            "submitForm" => {
                let form = node_id()?;
                let submitter = if text(2).is_empty() { form } else { id(2)? };
                if doc.node(submitter.key()).is_none() {
                    return Err("Node no longer exists".into());
                }
                doc.submit_form(form, submitter);
                Value::Null
            }
            "resetForm" => {
                let form = node_id()?;
                self.resolve(&mut doc);
                Self::reset_form_controls(&mut doc, form);
                Value::Null
            }
            "url" => json!(doc.url().as_str()),
            "navigate" => {
                let target = doc.url().join(text(1)).map_err(|e| e.to_string())?;
                if !matches!(target.scheme(), "http" | "https")
                    && !(target.scheme() == "file" && doc.url().scheme() == "file")
                {
                    return Err("Navigation scheme is not allowed".into());
                }
                if self.navigate.borrow().len() >= 8 {
                    return Err("Too many pending navigations".into());
                }
                self.navigate.borrow_mut().push(target);
                Value::Null
            }
            "now" => json!(self.start.elapsed().as_secs_f64() * 1000.0),
            "log" => {
                if matches!(text(2), "2" | "3") {
                    let mut diagnostic = crate::diagnostics::Diagnostic::error(
                        crate::diagnostics::Category::Javascript,
                        text(1),
                        Some(doc.url().to_string()),
                    );
                    if text(2) == "2" {
                        diagnostic.severity = "warning".into();
                    }
                    diagnostic.emit();
                }
                eprintln!(
                    "Wega JavaScript: {}",
                    text(1).chars().take(4096).collect::<String>()
                );
                Value::Null
            }
            _ => return Err("This Web API is not available; use Servo for this page".into()),
        };
        Ok(value)
    }
    /// The JS-facing message for a rejected mutation (kept from the pre-contract checks).
    fn mutation_error(error: wega_dom::MutationError) -> String {
        match error {
            wega_dom::MutationError::Stale(_) => "Node no longer exists".into(),
            wega_dom::MutationError::HierarchyCycle { .. } => "Cannot create a DOM cycle".into(),
            other => other.to_string(),
        }
    }
    fn subtree(doc: &BlitzDocument, root: NodeId) -> Vec<NodeId> {
        wega_dom::subtree(doc, root.key(), ToNodeId::node_id)
    }
    fn attribute(doc: &BlitzDocument, id: NodeId, name: &str) -> Option<String> {
        wega_dom::attribute(doc, id.key(), name)
    }
    fn component_subtree(&self, doc: &BlitzDocument, root: NodeId) -> Vec<NodeId> {
        let mut nodes = Vec::new();
        let mut pending = vec![root];
        let mut seen = HashSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(node) = doc.node(id.key()) else {
                continue;
            };
            nodes.push(id);
            pending.extend(node.children().rev().map(ToNodeId::node_id));
            if let Some(root) = self.shadows.borrow().root(id.as_u64(), true)
                && let Ok(root) = root.root.parse()
            {
                pending.push(NodeId::from_u64(root));
            }
        }
        nodes
    }
    fn component_connected(&self, doc: &BlitzDocument, mut node: NodeId) -> bool {
        let mut seen = HashSet::new();
        while seen.insert(node) {
            if node == doc.root_node().id {
                return true;
            }
            let Some(next) = doc
                .node(node.key())
                .and_then(NodeView::parent)
                .map(ToNodeId::node_id)
                .or_else(|| {
                    self.shadows
                        .borrow()
                        .host(node.as_u64())
                        .map(NodeId::from_u64)
                })
            else {
                return false;
            };
            node = next;
        }
        false
    }
    fn assigned_slot(&self, doc: &BlitzDocument, target: NodeId) -> Option<NodeId> {
        let node = doc.node(target.key())?;
        if !matches!(node.kind(), NodeKind::Element | NodeKind::Text) {
            return None;
        }
        let parent = (node.parent()?).node_id();
        let root = self
            .shadows
            .borrow()
            .root(parent.as_u64(), true)?
            .root
            .parse()
            .ok()?;
        let name = Self::attribute(doc, target, "slot").unwrap_or_default();
        Self::subtree(doc, NodeId::from_u64(root))
            .into_iter()
            .find(|id| {
                doc.node((*id).key())
                    .and_then(NodeView::element_name)
                    .is_some_and(|name| name.local == "slot")
                    && Self::attribute(doc, *id, "name").unwrap_or_default() == name
            })
    }
    fn assigned_nodes(
        &self,
        doc: &BlitzDocument,
        slot: NodeId,
        flatten: bool,
        seen: &mut HashSet<NodeId>,
    ) -> Vec<NodeId> {
        if !seen.insert(slot) {
            return vec![];
        }
        let root = wega_dom::tree_root(doc, slot.key());
        let Some(host) = self
            .shadows
            .borrow()
            .host(root.as_u64())
            .map(NodeId::from_u64)
        else {
            return vec![];
        };
        let mut assigned: Vec<_> = doc
            .node(host.key())
            .into_iter()
            .flat_map(|node| node.children().map(ToNodeId::node_id))
            .filter(|id| self.assigned_slot(doc, *id) == Some(slot))
            .collect();
        if flatten {
            if assigned.is_empty() {
                assigned = doc
                    .node(slot.key())
                    .map(|node| node.children().map(ToNodeId::node_id).collect())
                    .unwrap_or_default();
            }
            assigned
                .into_iter()
                .flat_map(|id| {
                    if doc
                        .get_node(id)
                        .and_then(|node| node.element_data())
                        .is_some_and(|element| element.name.local.as_str() == "slot")
                    {
                        self.assigned_nodes(doc, id, true, seen)
                    } else {
                        vec![id]
                    }
                })
                .collect()
        } else {
            assigned
        }
    }
    fn collect_detached(&self, doc: &mut BlitzDocument, id: NodeId) {
        let mut root = id;
        while let Some(parent) = doc.get_node(root).and_then(|n| n.parent).or_else(|| {
            self.shadows
                .borrow()
                .host(root.as_u64())
                .map(NodeId::from_u64)
        }) {
            root = parent;
        }
        if self.component_connected(doc, root) || doc.get_node(root).is_none() {
            return;
        }
        let nodes = self.component_subtree(doc, root);
        if nodes.iter().any(|id| self.retained.borrow().contains(id)) {
            return;
        }
        let bytes: usize = nodes
            .iter()
            .filter_map(|id| doc.get_node(*id))
            .map(|node| {
                32 + match &node.data {
                    NodeData::Text(text) => text.content.len(),
                    NodeData::Comment { contents } => contents.len(),
                    NodeData::Element(e) | NodeData::AnonymousBlock(e) => e
                        .attrs
                        .iter()
                        .map(|a| a.name.local.len() + a.value.len())
                        .sum(),
                    NodeData::Document(_) => 0,
                }
            })
            .sum();
        for id in &nodes {
            self.components.borrow_mut().release(id.as_u64());
            if let Some(shadow) = self.shadows.borrow_mut().release(id.as_u64())
                && let Ok(id) = shadow.root.parse()
            {
                let _ = wega_dom::mutate(&mut *doc).remove_and_drop(NodeId::from_u64(id).key());
            }
        }
        let _ = wega_dom::mutate(&mut *doc).remove_and_drop(root.key());
        self.allocated
            .set(self.allocated.get().saturating_sub(nodes.len()));
        self.used.set(self.used.get().saturating_sub(bytes));
    }
    pub fn set_allocated(&self, count: usize) {
        self.allocated.set(self.allocated.get().max(count));
    }
    /// Budget counters, for tests and diagnostics.
    pub fn budget(&self) -> (usize, usize) {
        (self.allocated.get(), self.used.get())
    }
    /// The form owner of a control: its `form` attribute, else the nearest ancestor form.
    pub fn form_owner(doc: &BlitzDocument, control: NodeId) -> Option<NodeId> {
        let element = doc.get_node(control)?.element_data()?;
        if let Some(owner) = element.attr(LocalName::from("form")) {
            return doc.get_element_by_id(owner).filter(|id| {
                doc.get_node(*id)
                    .and_then(|n| n.element_data())
                    .is_some_and(|e| &*e.name.local == "form")
            });
        }
        let mut parent = doc.get_node(control)?.parent;
        while let Some(id) = parent {
            let node = doc.get_node(id)?;
            if node
                .element_data()
                .is_some_and(|e| &*e.name.local == "form")
            {
                return Some(id);
            }
            parent = node.parent;
        }
        None
    }
    /// Resets all controls owned by `form` to their default state (defaultValue / defaultChecked).
    pub fn reset_form_controls(doc: &mut BlitzDocument, form: NodeId) {
        let controls: Vec<NodeId> = blitz_dom::traversal::TreeTraverser::new(doc)
            .filter(|id| Self::form_owner(doc, *id) == Some(form))
            .collect();
        for id in controls {
            if let Some(node) = doc.get_node_mut(id) {
                if let Some(element) = node.element_data_mut() {
                    let tag = &*element.name.local;
                    if tag == "input" {
                        let kind = element
                            .attr(LocalName::from("type"))
                            .unwrap_or("text")
                            .to_ascii_lowercase();
                        if matches!(kind.as_str(), "checkbox" | "radio") {
                            let default_checked =
                                element.attr(LocalName::from("checked")).is_some();
                            element.set_checkbox_input_checked(default_checked);
                        } else {
                            let default_value = element
                                .attr(LocalName::from("value"))
                                .unwrap_or_default()
                                .to_string();
                            if let Some(text_input) = element.text_input_data_mut() {
                                text_input.editor.set_text(&default_value);
                            }
                        }
                    } else if tag == "textarea" {
                        let default_value = element
                            .attr(LocalName::from("value"))
                            .unwrap_or_default()
                            .to_string();
                        if let Some(text_input) = element.text_input_data_mut() {
                            text_input.editor.set_text(&default_value);
                        }
                    }
                }
                node.mark_ancestors_dirty();
            }
        }
    }
    /// The form data set of a form (HTML "constructing the entry list"), for
    /// `new FormData(form)`. File inputs contribute empty entries.
    fn form_entries(
        doc: &BlitzDocument,
        form: NodeId,
        submitter: Option<NodeId>,
    ) -> Vec<(String, String)> {
        let mut entries = Vec::new();
        for id in blitz_dom::traversal::TreeTraverser::new(doc) {
            let Some(element) = doc.get_node(id).and_then(|n| n.element_data()) else {
                continue;
            };
            let tag = &*element.name.local;
            if !matches!(tag, "input" | "select" | "textarea" | "button")
                || Self::form_owner(doc, id) != Some(form)
                || element.attr(LocalName::from("disabled")).is_some()
            {
                continue;
            }
            let Some(name) = element
                .attr(LocalName::from("name"))
                .filter(|n| !n.is_empty())
            else {
                continue;
            };
            let kind = element
                .attr(LocalName::from("type"))
                .unwrap_or_default()
                .to_ascii_lowercase();
            let value = || {
                element
                    .text_input_data()
                    .map(|data| data.editor.text().to_string())
                    .or_else(|| element.attr(LocalName::from("value")).map(str::to_owned))
                    .unwrap_or_default()
            };
            match tag {
                "button" => {
                    if Some(id) == submitter {
                        entries.push((name.to_owned(), value()));
                    }
                }
                "select" => {
                    for option in Self::subtree(doc, id).into_iter().rev() {
                        let Some(option) = doc.get_node(option) else {
                            continue;
                        };
                        if option.element_data().is_some_and(|e| {
                            &*e.name.local == "option"
                                && e.attr(LocalName::from("selected")).is_some()
                        }) {
                            let value = option
                                .element_data()
                                .and_then(|e| e.attr(LocalName::from("value")).map(str::to_owned))
                                .unwrap_or_else(|| option.text_content().trim().to_owned());
                            entries.push((name.to_owned(), value));
                        }
                    }
                }
                "textarea" => entries.push((name.to_owned(), value())),
                _ => match kind.as_str() {
                    "submit" | "image" | "button" | "reset" => {
                        if Some(id) == submitter && kind == "submit" {
                            entries.push((name.to_owned(), value()));
                        }
                    }
                    "checkbox" | "radio" => {
                        // Blitz creates checkbox state at layout; before that, use the attribute.
                        let checked = element
                            .checkbox_input_checked()
                            .unwrap_or_else(|| element.attr(LocalName::from("checked")).is_some());
                        if checked {
                            let value = element
                                .attr(LocalName::from("value"))
                                .unwrap_or("on")
                                .to_owned();
                            entries.push((name.to_owned(), value));
                        }
                    }
                    "file" => entries.push((name.to_owned(), String::new())),
                    _ => entries.push((name.to_owned(), value())),
                },
            }
        }
        entries
    }
    fn serialize(doc: &BlitzDocument, id: NodeId, out: &mut String) {
        let Some(node) = doc.get_node(id) else {
            return;
        };
        match &node.data {
            NodeData::Text(text) => {
                let raw = node
                    .parent
                    .and_then(|id| doc.get_node(id))
                    .and_then(|n| n.element_data())
                    .is_some_and(|e| matches!(e.name.local.as_str(), "script" | "style"));
                if raw {
                    out.push_str(&text.content);
                } else {
                    out.push_str(
                        &text
                            .content
                            .replace('&', "&amp;")
                            .replace('<', "&lt;")
                            .replace('>', "&gt;"),
                    );
                }
            }
            NodeData::Comment { contents } => {
                out.push_str("<!--");
                out.push_str(contents);
                out.push_str("-->");
            }
            NodeData::Document(_) => {
                for child in &node.children {
                    Self::serialize(doc, *child, out);
                }
            }
            NodeData::Element(element) | NodeData::AnonymousBlock(element) => {
                if element.name.local.as_str() == "#document-fragment" {
                    for child in &node.children {
                        Self::serialize(doc, *child, out);
                    }
                    return;
                }
                let name = element.name.prefix.as_ref().map_or_else(
                    || element.name.local.to_string(),
                    |p| format!("{p}:{}", element.name.local),
                );
                out.push('<');
                out.push_str(&name);
                for attr in element.attrs.iter() {
                    out.push(' ');
                    if let Some(prefix) = &attr.name.prefix {
                        out.push_str(prefix);
                        out.push(':');
                    }
                    out.push_str(&attr.name.local);
                    out.push_str("=\"");
                    out.push_str(&attr.value.replace('&', "&amp;").replace('"', "&quot;"));
                    out.push('"');
                }
                out.push('>');
                let void = element.name.ns == ns!(html)
                    && matches!(
                        element.name.local.as_str(),
                        "area"
                            | "base"
                            | "br"
                            | "col"
                            | "embed"
                            | "hr"
                            | "img"
                            | "input"
                            | "link"
                            | "meta"
                            | "param"
                            | "source"
                            | "track"
                            | "wbr"
                    );
                if !void {
                    for child in &node.children {
                        Self::serialize(doc, *child, out);
                    }
                    out.push_str("</");
                    out.push_str(&name);
                    out.push('>');
                }
            }
        }
    }
}
