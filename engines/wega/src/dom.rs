//! Checked operations over the view's single document, used by the V8 host.
use crate::Limits;
use crate::document::WegaDocument;
use html5ever::{LocalName, QualName, ns};
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
use wega_dom::layout::{LayoutError, Point, Rect};
use wega_dom::style::StyleError;
use wega_dom::{DocumentView, Mutator, NodeKey, NodeKind, NodeView};

/// A `<script>` element that script inserted into the document.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScriptRequest {
    pub node: NodeKey,
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
    pub activate: Vec<NodeKey>,
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

/// The deepest nesting a native edit may create. Laying out 121 nested blocks takes about 22 ms
/// and 122 can hang a script-initiated first flush (roadmap B1), so edits stop at 121.
pub(crate) const MAX_EDIT_DEPTH: usize = 121;

pub(crate) struct Dom {
    pub network: crate::network::Network,
    pub hubs: crate::workers::Hubs,
    pub document: Rc<RefCell<WegaDocument>>,
    pub native_document: RefCell<Option<Rc<RefCell<crate::native_engine::NativeDocument>>>>,
    pub limits: Limits,
    pub navigate: Rc<RefCell<Vec<url::Url>>>,
    pub errors: Rc<RefCell<Vec<String>>>,
    pub requests: RefCell<Requests>,
    pub csp: std::sync::Arc<std::sync::RwLock<crate::csp::Csp>>,
    pub dialog: Rc<RefCell<Option<DocumentDialogHandler>>>,
    pub restrictions: crate::view::frames::Restrictions,
    pub frames: Rc<RefCell<crate::view::frames::FrameShared>>,
    pub frame_id: Option<u64>,
    pub document_generation: Rc<Cell<u64>>,
    pub document_revision: u64,
    /// Transient user activation, for popups.
    pub activation: Rc<Cell<Option<Instant>>>,
    pub started_scripts: Rc<RefCell<HashSet<NodeKey>>>,
    /// This document's session history: (index, length), reported by script.
    pub history: Cell<(usize, usize)>,
    allocated: Cell<usize>,
    used: Cell<usize>,
    start: Instant,
    retained: RefCell<HashSet<NodeKey>>,
    components: RefCell<crate::components::Registry>,
    shadows: RefCell<crate::components::ShadowTree>,
    clipboard: RefCell<String>,
    /// This document as a service worker client.
    client_id: String,
    created: Instant,
    pub workers: RefCell<crate::workers::WorkerManager>,
    pub canvas_engine: RefCell<crate::canvas::CanvasEngine>,
    canvas_nodes: RefCell<std::collections::HashMap<wega_dom::NodeKey, crate::canvas::CanvasId>>,
    pub webgl_engine: RefCell<crate::webgl::WebGLEngine>,
    pub media_engine: RefCell<crate::media::MediaEngine>,
    pub permissions: Rc<RefCell<crate::permissions::PermissionRegistry>>,
    pub console: Rc<RefCell<crate::console::ConsoleLog>>,
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
        let allocated = Cell::new({
            let doc = document.borrow();
            wega_dom::preorder(&*doc, doc.root()).count()
        });
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
            native_document: Default::default(),
            limits: limits.clone(),
            navigate: context.navigate,
            errors: Default::default(),
            requests: Default::default(),
            csp: context.csp,
            dialog: context.dialog,
            restrictions: context.restrictions,
            frames: context.frames,
            frame_id: context.frame_id,
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
            canvas_nodes: Default::default(),
            canvas_engine: RefCell::new(crate::canvas::CanvasEngine::new(limits.max_pixels * 4)),
            webgl_engine: RefCell::new(crate::webgl::WebGLEngine::new(limits.max_pixels * 8)),
            media_engine: RefCell::new(crate::media::MediaEngine::new(limits.resource_bytes * 8)),
            permissions: context.permissions,
            console: context.console,
            revision: Cell::new(0),
            layout_dirty: Cell::new(true),
        }
    }
    /// Internal presentation may read tainted pixels; script readback remains checked.
    pub(crate) fn canvas_images(&self) -> Vec<(wega_dom::NodeKey, peniko::ImageData)> {
        self.apply_worker_frames();
        let mut engine = self.canvas_engine.borrow_mut();
        let mut images = Vec::new();
        for (&key, &id) in self.canvas_nodes.borrow().iter() {
            if self.document.borrow().node(key).is_none() {
                continue;
            }
            let Some(ctx) = engine.get_mut(id) else {
                continue;
            };
            if ctx.width == 0 || ctx.height == 0 {
                continue;
            }
            let mut bytes = vec![0; ctx.width as usize * ctx.height as usize * 4];
            let info = skia_safe::ImageInfo::new(
                (ctx.width as i32, ctx.height as i32),
                skia_safe::ColorType::RGBA8888,
                skia_safe::AlphaType::Unpremul,
                None,
            );
            if !ctx
                .surface
                .read_pixels(&info, &mut bytes, ctx.width as usize * 4, (0, 0))
            {
                continue;
            }
            images.push((
                key,
                peniko::ImageData {
                    data: peniko::Blob::new(std::sync::Arc::new(bytes)),
                    format: peniko::ImageFormat::Rgba8,
                    alpha_type: peniko::ImageAlphaType::Alpha,
                    width: ctx.width,
                    height: ctx.height,
                },
            ));
        }
        images
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
    /// The one place new nodes are checked against the document's node budget. Script host ops
    /// and native (inspector) edits must both go through it, or they would count separately and
    /// together exceed the limit. Checks only; `commit_nodes` records them once they exist.
    pub(crate) fn reserve_nodes(&self, count: usize) -> Result<(), String> {
        if self.allocated.get().saturating_add(count) > self.limits.dom_nodes {
            return Err("DOM node limit exceeded".into());
        }
        Ok(())
    }
    pub(crate) fn commit_nodes(&self, count: usize) {
        self.allocated
            .set(self.allocated.get().saturating_add(count));
    }
    /// Check and record `bytes` of new text (attribute values, text nodes, markup, style) against
    /// the text budget in one step.
    pub(crate) fn charge_text(&self, bytes: usize) -> Result<(), String> {
        let next = self.used.get().saturating_add(bytes);
        if next > self.limits.dom_text_bytes {
            return Err("DOM text allocation budget exceeded".into());
        }
        self.used.set(next);
        Ok(())
    }
    /// Reserve everything a native edit will add: `nodes` new nodes, `text_bytes` of new text, and
    /// a resulting nesting `depth`. The engine has no nesting budget for script: past
    /// `wega_dom::layout::MAX_LAYOUT_DEPTH` layout is refused, not hung (see `layout_depth.rs`).
    /// Native edits stay at the older, more conservative `MAX_EDIT_DEPTH` so an inspector edit
    /// never makes a document lose its layout.
    pub(crate) fn reserve_edit(
        &self,
        nodes: usize,
        text_bytes: usize,
        depth: usize,
    ) -> Result<(), String> {
        if depth > MAX_EDIT_DEPTH {
            return Err("DOM nesting limit exceeded".into());
        }
        self.reserve_nodes(nodes)?;
        self.charge_text(text_bytes)?;
        self.commit_nodes(nodes);
        Ok(())
    }
    fn invalidate(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
        self.layout_dirty.set(true);
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
            let caller_origin = self.origin();
            for identity in identities {
                let id_str = identity
                    .as_str()
                    .ok_or("TypeError: invalid binding node identity")?;
                if let Some((target, node_id)) = self.parse_cross_frame_id(id_str) {
                    if caller_origin == "null" {
                        return Err(
                            "SecurityError: Opaque documents cannot access other browsing contexts"
                                .into(),
                        );
                    }
                    match target {
                        CrossFrameTarget::Child { key, generation } => {
                            let shared = self.frames.borrow();
                            let child = shared
                                .children
                                .iter()
                                .find(|c| c.key == key)
                                .cloned()
                                .ok_or("InvalidStateError: frame no longer exists")?;
                            if child.opaque
                                || child.origin == "null"
                                || child.origin != caller_origin
                            {
                                return Err("SecurityError: Blocked a frame from accessing a cross-origin frame.".into());
                            }
                            if child.document_generation.get() != generation {
                                return Err(
                                    "InvalidStateError: The frame document has navigated.".into()
                                );
                            }
                            let child_doc = child
                                .document
                                .try_borrow()
                                .map_err(|_| "InvalidStateError: reentrant DOM borrow")?;
                            if wega_dom::DocumentView::node(
                                &*child_doc,
                                wega_dom::NodeKey::from_u64(node_id),
                            )
                            .is_none()
                            {
                                return Err(
                                    "InvalidStateError: binding node no longer exists".into()
                                );
                            }
                        }
                        CrossFrameTarget::Parent { generation } => {
                            let shared = self.frames.borrow();
                            let parent = shared
                                .parent
                                .clone()
                                .ok_or("InvalidStateError: no parent browsing context")?;
                            if parent.opaque
                                || parent.origin == "null"
                                || parent.origin != caller_origin
                            {
                                return Err("SecurityError: Blocked a frame from accessing a cross-origin frame.".into());
                            }
                            let gen_rc = parent
                                .document_generation
                                .upgrade()
                                .ok_or("Parent document detached")?;
                            let doc_rc = parent
                                .document
                                .upgrade()
                                .ok_or("Parent document detached")?;
                            if gen_rc.get() != generation {
                                return Err(
                                    "InvalidStateError: The parent document has navigated.".into(),
                                );
                            }
                            let parent_doc = doc_rc
                                .try_borrow()
                                .map_err(|_| "InvalidStateError: reentrant DOM borrow")?;
                            if wega_dom::DocumentView::node(
                                &*parent_doc,
                                wega_dom::NodeKey::from_u64(node_id),
                            )
                            .is_none()
                            {
                                return Err(
                                    "InvalidStateError: binding node no longer exists".into()
                                );
                            }
                        }
                    }
                } else {
                    let key = wega_dom::NodeKey::from_u64(
                        id_str
                            .parse::<u64>()
                            .map_err(|_| "TypeError: invalid binding node identity")?,
                    );
                    if wega_dom::DocumentView::node(&*document, key).is_none() {
                        return Err("InvalidStateError: binding node no longer exists".into());
                    }
                }
            }
            return Ok(Value::Null);
        }
        if text(0).starts_with("frame") {
            return self.frame_call(args);
        }
        if text(0) == "origin" {
            return Ok(json!(self.origin()));
        }
        if text(0) == "canvasAttach" {
            let key =
                wega_dom::NodeKey::from_u64(text(1).parse().map_err(|_| "Invalid canvas node")?);
            let id = crate::canvas::CanvasId(text(2).parse().map_err(|_| "Invalid canvas ID")?);
            if !self
                .document
                .borrow()
                .node(key)
                .and_then(NodeView::element_name)
                .is_some_and(|n| n.local == "canvas")
                || self.canvas_engine.borrow().get(id).is_none()
            {
                return Err("InvalidStateError: canvas association".into());
            }
            self.canvas_nodes.borrow_mut().insert(key, id);
            return Ok(Value::Null);
        }
        if text(0).starts_with("canvas") {
            self.invalidate();
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
                | "sheetInsert"
                | "sheetDelete"
                | "sheetDisabledSet"
                | "styleSet"
                | "styleSetImportant"
                | "focus"
                | "blur"
                | "setChecked"
                | "setInputValue"
                | "setSelectionRange"
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
        if matches!(text(0), "error" | "rejection") {
            self.console.borrow_mut().exception(
                if text(0) == "rejection" {
                    wega_protocol::inspector::ExceptionKind::UnhandledRejection
                } else {
                    wega_protocol::inspector::ExceptionKind::Uncaught
                },
                text(1),
                self.document.borrow().url().as_str(),
            );
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
            if self.restrictions.modals_blocked {
                return Ok(Value::Null);
            }
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
        if let Some((target, prefix)) = self.detect_cross_frame_target(args) {
            return self.dispatch_cross_frame(args, target, &prefix);
        }
        let charge = match text(0) {
            "setText" => text(2).len() + 32,
            "setAttr" => text(2).len() + text(3).len() + 32,
            "create" | "createText" | "createComment" | "createFragment" => text(1).len() + 32,
            "createNS" => text(1).len() + text(2).len() + 32,
            "innerHTML" | "contextualFragment" => text(2).len() + 32,
            "write" => text(1).len(),
            "styleSet" | "styleSetImportant" => text(2).len() + text(3).len() + 32,
            "sheetInsert" => text(2).len() + 32,
            _ => 0,
        };
        self.charge_text(charge)?;
        let id = |index: usize| {
            text(index)
                .parse::<u64>()
                .map(NodeKey::from_u64)
                .map_err(|_| "Invalid node identity".to_string())
        };
        if let Some(result) = self.native_geometry_call(args) {
            return result;
        }
        let mut doc = self
            .document
            .try_borrow_mut()
            .map_err(|_| "DOM is currently busy".to_string())?;
        let node_id = || -> Result<NodeKey, String> {
            let id = id(1)?;
            if doc.node(id).is_none() {
                Err("Node no longer exists".into())
            } else {
                Ok(id)
            }
        };
        let value = match text(0) {
            "sheetOwners" => {
                let root = node_id()?;
                let mut owners = Vec::new();
                let mut stack = vec![root];
                while let Some(key) = stack.pop() {
                    if let Some(node) = doc.node(key) {
                        if doc.node_has_stylesheet(key) { owners.push(key.to_string()); }
                        stack.extend(node.children().rev());
                    }
                }
                json!(owners)
            },
            "sheetHas" => json!(doc.node_has_stylesheet(node_id()?)),
            "sheetDisabled" => json!(doc.stylesheet_disabled(node_id()?)),
            "sheetDisabledSet" => {
                let target = node_id()?;
                doc.set_stylesheet_disabled(target, text(2) == "true");
                Value::Null
            },
            "sheetRules" | "sheetInsert" | "sheetDelete" => {
                let target = node_id()?;
                let sheet_url = doc.stylesheet_url(target).ok_or("NotFoundError: stylesheet removed")?;
                if doc.node(target).and_then(NodeView::element_name).is_some_and(|n| n.local == "link")
                    && (sheet_url.origin() != doc.url().origin() || self.restrictions.opaque) {
                    return Err("SecurityError: stylesheet is not origin-clean".into());
                }
                match text(0) {
                    "sheetRules" => {
                        let count = doc.stylesheet_rule_count(target, &[]).unwrap_or(0);
                        let rules: Vec<_> = (0..count).filter_map(|index| {
                            let info = doc.stylesheet_rule_info(target, &[index])?;
                            Some(json!({"cssText":info.css_text,"type":info.rule_type}))
                        }).collect();
                        json!(rules)
                    },
                    "sheetInsert" => {
                        if doc.stylesheet_rule_count(target, &[]).unwrap_or(0) >= 16384 {
                            return Err("QuotaExceededError: stylesheet rule budget".into());
                        }
                        let index = text(3).parse::<usize>().map_err(|_| "IndexSizeError: invalid rule index")?;
                        json!(doc.stylesheet_insert_rule(target, &[], text(2), index)
                            .map_err(|e| format!("{}: insertRule",e.dom_exception_name()))?)
                    },
                    _ => {
                        let index = text(2).parse::<usize>().map_err(|_| "IndexSizeError: invalid rule index")?;
                        doc.stylesheet_delete_rule(target, &[], index)
                            .map_err(|e| format!("{}: deleteRule",e.dom_exception_name()))?;
                        Value::Null
                    }
                }
            },
            "customUpgrade" => {
                let target = node_id()?;
                let name = doc.node(target).and_then(NodeView::element_name).map_or("", |name| name.local);
                json!(self.components.borrow().needs_upgrade(target.as_u64(), name))
            },
            "customCandidates" => {
                let root = node_id()?;
                let definition: u64 = text(2).parse().map_err(|_| "Invalid definition ID")?;
                let registry = self.components.borrow();
                let candidates: Vec<_> = self.component_subtree(&*doc, root).into_iter().filter(|id| {
                    let name = doc.node(*id).and_then(NodeView::element_name).map_or("", |name| name.local);
                    registry.needs_upgrade(id.as_u64(), name) == Some(definition)
                }).map(|id| id.as_u64().to_string()).collect();
                json!(candidates)
            },
            "customBegin" => {
                let target = node_id()?;
                let definition: u64 = text(2).parse().map_err(|_| "Invalid definition ID")?;
                let mut registry = self.components.borrow_mut();
                let name = doc.node(target).and_then(NodeView::element_name).map(|name| name.local).ok_or("Custom element must be an element")?;
                if registry.get(name).is_none_or(|entry| entry.id != definition) { return Err("Custom element definition does not match its node".into()); }
                let began = registry.begin(target.as_u64(), definition)?;
                if began {
                    for (name, value) in doc.node(target).into_iter().flat_map(NodeView::attributes) {
                        registry.attribute(target.as_u64(), name.local.as_ref(), None, Some(value), (!name.ns.as_str().is_empty()).then_some(name.ns.as_str()));
                    }
                    if self.component_connected(&*doc, target) { registry.lifecycle(target.as_u64(), true); }
                }
                json!(began)
            },
            "customComplete" => {
                let target = node_id()?; self.components.borrow_mut().complete(target.as_u64(), text(2) == "true")?; Value::Null
            },
            "customConstruct" => {
                let definition: u64 = text(1).parse().map_err(|_| "Invalid definition ID")?;
                let name = self.components.borrow().by_id(definition).ok_or("TypeError: Illegal constructor")?.name.clone();
                self.reserve_nodes(1)?;
                let created = wega_dom::mutate(&mut *doc).create_element(QualName::new(None, ns!(html), LocalName::from(name)), []);
                self.allocated.set(self.allocated.get()+1); self.invalidate();
                let mut registry = self.components.borrow_mut(); registry.begin(created.as_u64(), definition)?; registry.complete(created.as_u64(), true)?;
                json!(created.as_u64().to_string())
            },
            "customConnected" | "customDisconnected" => {
                let target = node_id()?; let nodes = self.component_subtree(&*doc, target); let mut registry = self.components.borrow_mut();
                for id in nodes { registry.lifecycle(id.as_u64(), text(0) == "customConnected"); } Value::Null
            },
            "customAdopt" => {
                let target = node_id()?; let old = id(2)?; let new = id(3)?;
                if !doc.node(old).is_some_and(|node|node.kind()==NodeKind::Document) || !doc.node(new).is_some_and(|node|node.kind()==NodeKind::Document) { return Err("Invalid adoption documents".into()); }
                let mut registry = self.components.borrow_mut(); for child in self.component_subtree(&*doc, target) { registry.adopted(child.as_u64(), old.as_u64(), new.as_u64()); } Value::Null
            },
            "shadowAttach" => {
                let target = node_id()?;
                if self.shadows.borrow().root(target.as_u64(), true).is_some() { return Err("NotSupportedError: shadow root already attached".into()); }
                let tag = doc.node(target).and_then(NodeView::element_name).ok_or("NotSupportedError: invalid shadow host")?.local.to_string();
                self.reserve_nodes(1)?;
                let root = wega_dom::mutate(&mut *doc).create_element(QualName::new(None, ns!(html), LocalName::from("#document-fragment")), []);
                let entry = match self.shadows.borrow_mut().attach(target.as_u64(), root.as_u64(), text(2), text(3) == "true", &tag) {
                    Ok(entry) => entry, Err(error) => { let _ = wega_dom::mutate(&mut *doc).remove_and_drop(root); return Err(error); }
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
                if let Some(slot) = self.assigned_slot(&*doc, target) { json!(slot.as_u64().to_string()) }
                else if let Some(parent) = wega_dom::DocumentView::node(&*doc, target).and_then(wega_dom::NodeView::parent) { json!(parent.to_string()) }
                else if text(2) != "true" && text(3) == text(1) { Value::Null }
                else { json!(self.shadows.borrow().host(target.as_u64()).map(|id|id.to_string())) }
            },
            "assignedSlot" | "assignedSlotInternal" => {
                let mut assigned = self.assigned_slot(&*doc, node_id()?);
                if text(0) == "assignedSlot" && let Some(slot) = assigned {
                        let root = wega_dom::tree_root(&*doc, slot);
                        if let Some(host) = self.shadows.borrow().host(root.as_u64()) && self.shadows.borrow().root(host, false).is_none() { assigned = None; }
                }
                json!(assigned.map(|id|id.as_u64().to_string()))
            },
            "componentRoot" => {
                let mut root = node_id()?;
                loop {
                    if let Some(parent) = doc.node(root).and_then(NodeView::parent) { root = parent; }
                    else if text(2) == "true" && let Some(host) = self.shadows.borrow().host(root.as_u64()) { root = NodeKey::from_u64(host); }
                    else { break; }
                }
                json!(root.as_u64().to_string())
            },
            "assignedNodes" => {
                let nodes = self.assigned_nodes(&*doc, node_id()?, text(2) == "true", &mut HashSet::new());
                json!(nodes.into_iter().map(|id|id.as_u64().to_string()).collect::<Vec<_>>())
            },
            "cookie"=>if self.restrictions.opaque{json!("")}else{json!(self.network.cookies.get_frame(doc.url(),true,crate::cookies::SiteContext::SameSite,self.frame_id))},
            "setCookie"=>{if !self.restrictions.opaque{self.network.cookies.set_frame(doc.url(),text(1),true,crate::cookies::SiteContext::SameSite,self.frame_id);}Value::Null},
            "templateContent"=>{let target=node_id()?;let creates=doc.owned_fragment(target).is_none();if creates {self.reserve_nodes(1)?;}let content=wega_dom::mutate(&mut *doc).template_contents(target).map_err(Self::mutation_error)?;if creates {self.commit_nodes(1);}json!(content.as_u64().to_string())},
            "active"=>json!(wega_dom::input::FocusState::focused(&*doc).map(|key|key.to_string())),
            "history"=>{let url=doc.url().join(text(1)).map_err(|e|e.to_string())?;if url.origin()!=doc.url().origin()||!matches!(url.scheme(),"http"|"https"){return Err("SecurityError: history URL must have the same tuple origin".into());}doc.set_base_url(url);Value::Null},

            "subtree"=>json!(self.component_subtree(&*doc,node_id()?).iter().map(|id|id.as_u64().to_string()).collect::<Vec<_>>()),
            "documentDomain"=>{
                if doc.url().origin().is_tuple() { Value::Null }
                else {return Err("SecurityError: opaque documents have no domain".into());}
            },
            "resolveURL"=>{let base=url::Url::parse(text(2)).map_err(|e|e.to_string())?;let url=base.join(text(1)).map_err(|e|e.to_string())?;json!({"href":url.as_str(),"origin":url.origin().ascii_serialization(),"protocol":format!("{}:",url.scheme()),"host":url.host_str().map_or(String::new(),|host|url.port().map_or_else(||host.to_owned(),|port|format!("{host}:{port}"))),"hostname":url.host_str().unwrap_or_default(),"port":url.port().map(|p|p.to_string()).unwrap_or_default(),"pathname":url.path(),"search":url.query().map(|q|format!("?{q}")).unwrap_or_default(),"hash":url.fragment().map(|f|format!("#{f}")).unwrap_or_default()})},
            "connected"=>json!(self.component_connected(&*doc, node_id()?)),
            "retain" => {let id=node_id()?;self.retained.borrow_mut().insert(id);Value::Null},
            "release" => {let id=id(1)?;self.retained.borrow_mut().remove(&id);self.collect_detached(&mut *doc,id);Value::Null},
            "info"=>{
                let info=wega_dom::node_info(&*doc,node_id()?).ok_or("InvalidStateError: node no longer exists")?;
                json!({"nodeType":info.node_type,"nodeName":info.node_name,"localName":info.local_name,"namespaceURI":info.namespace,"prefix":info.prefix})
            },
            "attributes"=>json!(doc.node(node_id()?).into_iter().flat_map(NodeView::attributes).map(|(name,value)|json!({"name":name.prefix.as_ref().map_or_else(||name.local.to_string(),|p|format!("{p}:{}",name.local)),"localName":name.local.to_string(),"namespaceURI":if name.ns==ns!(){None}else{Some(name.ns.to_string())},"value":value})).collect::<Vec<_>>()),
            "queryIn"=>json!(doc.query_selector_in(node_id()?,text(2)).map_err(|_|"SyntaxError: Invalid selector")?.map(|id|id.as_u64().to_string())),
            "queryAllIn"=>json!(doc.query_selector_all_in(node_id()?,text(2)).map_err(|_|"SyntaxError: Invalid selector")?.iter().map(|id|id.as_u64().to_string()).collect::<Vec<_>>()),
            "matches"=>json!(doc.matches_selector(node_id()?,text(2)).map_err(|_|"SyntaxError: Invalid selector")?),
            "closest"=>json!(doc.closest(node_id()?,text(2)).map_err(|_|"SyntaxError: Invalid selector")?.map(|id|id.as_u64().to_string())),
            "getById" => {
                let id_str = text(1);
                let target_scope = text(2).parse::<u64>().ok().map(NodeKey::from_u64);
                let found = if let Some(scope) = target_scope.filter(|&s| s != doc.root()) {
                    let mut pending = vec![scope];
                    let mut result = None;
                    while let Some(curr) = pending.pop() {
                        if let Some(node) = doc.node(curr) {
                            if NodeView::attr(node, "id") == Some(id_str) {
                                result = Some(curr);
                                break;
                            }
                            pending.extend(node.children().rev());
                        }
                    }
                    result
                } else {
                    wega_dom::DocumentView::element_by_id(&*doc, id_str)
                };
                json!(found.map(|key| key.to_string()))
            },
            "outerHTML"=>{let mut html=String::new();Self::serialize(&*doc,node_id()?,&mut html);json!(html)},
            "innerHTML" | "contextualFragment"=>{
                let mut target=node_id()?;
                if doc.node(target).and_then(NodeView::element_name).is_some_and(|name|name.local=="template") { target=wega_dom::mutate(&mut *doc).template_contents(target).map_err(Self::mutation_error)?; }
                if text(2).len()>self.limits.html_bytes{return Err("HTML fragment exceeds the size limit".into());}
                // Parse into a detached same-context element first; reject budgets before
                // replacing live children, and retain removed nodes held by JS wrappers.
                let node=doc.node(target).ok_or("Node no longer exists")?;
                let name=if node.is_fragment() {QualName::new(None,ns!(html),LocalName::from("div"))} else {node.qual_name().ok_or("innerHTML needs an element")?.clone()};let staging:NodeKey=wega_dom::mutate(&mut *doc).create_element(name,[]);
                let remaining=self.limits.dom_nodes.saturating_sub(self.allocated.get()).saturating_add(2);
                if wega_html::parse_fragment(&mut *doc,staging,remaining,text(2),&crate::html_report::HtmlReport).is_err() {let _=wega_dom::mutate(&mut *doc).remove_and_drop(staging);return Err("DOM node limit exceeded".into());}
                let parsed=Self::subtree(&*doc,staging);let parsed_count=wega_dom::owned_subtree(&*doc,staging).count();
                // innerHTML scripts stay inert; Range fragment scripts can run on insertion.
                if text(0)=="innerHTML" {for id in &parsed {if doc.node(*id).and_then(NodeView::element_name).is_some_and(|name|name.local=="script"){self.started_scripts.borrow_mut().insert(*id);}}}
                if self.allocated.get().saturating_add(parsed_count)>self.limits.dom_nodes {wega_dom::mutate(&mut *doc).remove_and_drop(staging).map_err(Self::mutation_error)?;return Err("DOM node limit exceeded".into());}
                let children:Vec<wega_dom::NodeKey>=doc.node(staging).map(|node|node.children().collect()).unwrap_or_default();
                if text(0)=="contextualFragment" {
                    let mut mutation=wega_dom::mutate(&mut *doc);let fragment=mutation.create_element(QualName::new(None,ns!(html),LocalName::from("#document-fragment")),[]);
                    mutation.replace_children(fragment,&children).map_err(Self::mutation_error)?;mutation.remove_and_drop(staging).map_err(Self::mutation_error)?;mutation.finish();let fragment:NodeKey=fragment;
                    self.allocated.set(self.allocated.get()+parsed_count);
                    return Ok(json!(fragment.as_u64().to_string()));
                }
                let old:Vec<NodeKey>=doc.node(target).map(|node|node.children().collect()).unwrap_or_default();
                {let mut mutation=wega_dom::mutate(&mut *doc);mutation.replace_children(target,&children).map_err(Self::mutation_error)?;mutation.remove_and_drop(staging).map_err(Self::mutation_error)?;}
                self.allocated.set(self.allocated.get()+parsed_count-1);
                for id in old {self.collect_detached(&mut *doc,id);}
                Value::Null
            },
            "clone"=>{
                let source=node_id()?;let deep=text(2)=="true";let count=if deep{wega_dom::owned_subtree(&*doc,source).count()}else{1};
                self.reserve_nodes(count)?;
                let mut serialized=String::new();Self::serialize(&*doc,source,&mut serialized);let bytes=serialized.len();
                if self.used.get().saturating_add(bytes)>self.limits.dom_text_bytes{return Err("DOM text allocation budget exceeded".into());}
                let cloned=wega_dom::mutate(&mut *doc).clone_node(source,deep).map_err(|error|match error{wega_dom::MutationError::NotCloneable(_)=>"This node cannot be cloned".to_string(),other=>Self::mutation_error(other)})?;
                self.allocated.set(self.allocated.get()+count);self.used.set(self.used.get()+bytes);json!(cloned.as_u64().to_string())
            },
            "insert"=>{
                let parent=node_id()?;let child=id(2)?;
                if doc.node(child).is_none(){return Err("Node no longer exists".into());}
                let anchor=if text(3).is_empty(){None}else{Some(id(3)?)};
                if anchor.is_some_and(|id|doc.node(id).is_none_or(|n|n.parent()!=Some(parent))){return Err("NotFoundError: reference is not a child".into());}
                let mut ancestor=Some(parent);while let Some(id)=ancestor {if id==child{return Err("HierarchyRequestError: DOM cycle".into());}ancestor=doc.node(id).and_then(NodeView::parent).or_else(||self.shadows.borrow().host(id.as_u64()).map(NodeKey::from_u64));}
                if anchor==Some(child){return Ok(json!(child.as_u64().to_string()));}
                {let mut mutation=wega_dom::mutate(&mut *doc);if let Some(anchor)=anchor{mutation.insert_before(anchor,&[child]).map_err(Self::mutation_error)?;}else{mutation.append_children(parent,&[child]).map_err(Self::mutation_error)?;}}
                json!(child.as_u64().to_string())
            },
            "createNS"|"createComment"|"createFragment"|"createDocument"=>{
                self.reserve_nodes(1)?;
                let created=match text(0){
                    "createDocument"=>wega_dom::mutate(&mut *doc).create_document(),
                    "createComment"=>wega_dom::mutate(&mut *doc).create_comment(text(1)),
                    "createFragment"=>wega_dom::mutate(&mut *doc).create_element(QualName::new(None,ns!(html),LocalName::from("#document-fragment")),[]),
                    _=>{let parts=text(2).split_once(':');let (prefix,local)=parts.map_or((None,text(2)),|(p,l)|(Some(html5ever::Prefix::from(p)),l));wega_dom::mutate(&mut *doc).create_element(QualName::new(prefix,html5ever::Namespace::from(text(1)),LocalName::from(local)),[])}
                };self.allocated.set(self.allocated.get()+1);json!(created.as_u64().to_string())
            },
            "attrNS"=>json!(wega_dom::attribute_ns(&*doc,node_id()?,text(2),text(3))),
            "setAttrNS"=>{
                let target=node_id()?;let payload=[text(3).to_owned(),text(4).to_owned()];
                let (prefix,local)=payload[0].split_once(':').map_or((None,payload[0].as_str()),|(p,l)|(Some(html5ever::Prefix::from(p)),l));
                let charge=payload[0].len()+payload[1].len();if self.used.get().saturating_add(charge)>self.limits.dom_text_bytes{return Err("DOM text allocation budget exceeded".into());}self.used.set(self.used.get()+charge);
                let old = wega_dom::attribute_ns(&*doc,target,text(2),local);
                wega_dom::mutate(&mut *doc).set_attribute(target,QualName::new(prefix,html5ever::Namespace::from(text(2)),LocalName::from(local)),&payload[1]).map_err(Self::mutation_error)?;
                self.components.borrow_mut().attribute(target.as_u64(), local, old.as_deref(), Some(&payload[1]), (!text(2).is_empty()).then_some(text(2)));
                Value::Null
            },
            "supports"=>json!(doc.supports_condition(text(1))),
            "supportsProperty"=>json!(doc.supports_declaration(text(1),text(2))),
            "inlineCSS"=>{let mut css=String::new();Self::style_text(doc.inline_style(node_id()?,&mut css),css)?},
            "inlineProperty"=>{let mut css=String::new();Self::style_text(doc.inline_property(node_id()?,text(2),&mut css),css)?},
            // CSSOM setProperty/removeProperty; `false` when the declaration was rejected or
            // changed nothing, so script queues no mutation record for it.
            "styleSet"|"styleSetImportant"=>{
                let target=node_id()?;let old=Self::attribute(&*doc,target,"style");
                let changed=doc.set_inline_property(target,text(2),text(3),text(0)=="styleSetImportant").map_err(Self::mutation_error)?;
                if changed{let new=Self::attribute(&*doc,target,"style");self.components.borrow_mut().attribute(target.as_u64(),"style",old.as_deref(),new.as_deref(),None);self.invalidate();}
                json!(changed)
            },
            "inputValue"=>json!(doc.input_value(node_id()?).ok_or("Input needs an element")?),
            "setInputValue"=>{let target=node_id()?;doc.set_input_value(target,text(2))?;self.invalidate();Value::Null},
            "selectionRange"=>{let range=doc.selection_range(node_id()?);json!([range.0,range.1])},
            "setSelectionRange"=>{let target=node_id()?;doc.set_selection_range(target,text(2).parse().unwrap_or(0),text(3).parse().unwrap_or(0));self.invalidate();Value::Null},
            "checked"=>json!(doc.checked(node_id()?)),
            "optionSelected"=>json!(doc.option_selected(node_id()?)),
            "setChecked"=>{let target=node_id()?;doc.set_checked(target,text(2)=="true");Value::Null},
            "focus"=>{let target=node_id()?;let _=doc.set_focus(target);self.invalidate();Value::Null},
            "blur"=>{doc.clear_focus();self.invalidate();Value::Null},
            "serialize"=>{let mut html=String::new();Self::serialize(&*doc,node_id()?,&mut html);json!(html)},
            "limits" => json!({"resourceBytes":self.limits.resource_bytes}),
            "windowNamed" => json!(wega_dom::preorder(&*doc, doc.root()).filter(|id| {
                doc.node(*id).is_some_and(|node| node.element_name().is_some_and(|element| {
                    NodeView::attr(node,"id")==Some(text(1)) || (matches!(element.local,"form"|"img"|"embed"|"object") && NodeView::attr(node,"name")==Some(text(1)))
                }))
            }).map(|id|id.as_u64().to_string()).collect::<Vec<_>>()),
            "root" => json!(doc.root().as_u64().to_string()),
            "query" => json!(
                doc.query_selector(text(1))
                    .map_err(|_| "SyntaxError: Invalid selector")?
                    .map(|id| id.as_u64().to_string())
            ),
            "queryAll" => json!(
                doc.query_selector_all(text(1))
                    .map_err(|_| "SyntaxError: Invalid selector")?
                    .iter()
                    .map(|id| id.as_u64().to_string())
                    .collect::<Vec<_>>()
            ),
            "text" => json!(wega_dom::content::text_content(&*doc, node_id()?)),
            "tag" => json!(
                doc.node(node_id()?).and_then(NodeView::element_name).map(|name| name.local.to_uppercase())
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
            "childCount" => json!(doc.node(node_id()?).map_or(0, |node| node.children().len())),
            "childAt" => {
                let index=text(2).parse::<usize>().unwrap_or(usize::MAX);
                json!(doc.node(node_id()?).and_then(|node| node.child_at(index)).map(|key| key.to_string()))
            },
            "children" => json!(
                doc.node(node_id()?)
                    .map(|node| node.children().map(|key| key.to_string()).collect::<Vec<_>>())
                    .unwrap_or_default()
            ),
            "parent" => json!(doc.node(node_id()?).and_then(NodeView::parent).map(|key| key.to_string())),
            // ResizeObserver: the border box and the content box.
            "attr" => json!(wega_dom::attribute(&*doc, node_id()?, text(2))),
            "setText" => {
                let id = node_id()?;
                if text(2).len() > 1024 * 1024 {
                    return Err("DOM text budget exceeded".into());
                }
                let credit = |old: usize| self.used.set(self.used.get().saturating_sub(old));
                let node = doc.node(id).unwrap();
                match node.kind() {
                    NodeKind::Comment => {
                        credit(node.text().map_or(0, str::len));
                        wega_dom::mutate(&mut *doc).set_comment(id, text(2)).map_err(Self::mutation_error)?;
                        return Ok(Value::Null);
                    }
                    NodeKind::Document if !node.is_fragment() => return Ok(Value::Null),
                    NodeKind::Text => {
                        credit(node.text().map_or(0, str::len));
                        wega_dom::mutate(&mut *doc).set_text(id, text(2)).map_err(Self::mutation_error)?;
                        return Ok(Value::Null);
                    }
                    _ => {}
                }
                // Replacing an element's children: only a new text child is a new node,
                // and the old children are collected (crediting their budget) unless
                // script still holds them.
                let old: Vec<NodeKey> = doc.node(id).map(|node| node.children().collect()).unwrap_or_default();
                if text(2).is_empty() {
                    wega_dom::mutate(&mut *doc)
                        .replace_children(id, &[])
                        .map_err(Self::mutation_error)?;
                } else {
                    if self.allocated.get() >= self.limits.dom_nodes {
                        return Err("DOM text or node budget exceeded".into());
                    }
                    self.allocated.set(self.allocated.get() + 1);
                    let mut mutation = wega_dom::mutate(&mut *doc);
                    let child = mutation.create_text(text(2));
                    mutation
                        .replace_children(id, &[child])
                        .map_err(Self::mutation_error)?;
                }
                for child in old {
                    self.collect_detached(&mut *doc, child);
                }
                Value::Null
            }
            "setAttr" => {
                let id = node_id()?;
                let old = Self::attribute(&*doc, id, text(2));
                if matches!(text(2),"width"|"height") {
                    if let Some(canvas)=self.canvas_nodes.borrow().get(&id).copied() {
                        let node=doc.node(id).ok_or("Canvas removed")?;
                        let width=if text(2)=="width"{text(3)}else{node.attr("width").unwrap_or("300")}.parse().unwrap_or(300);
                        let height=if text(2)=="height"{text(3)}else{node.attr("height").unwrap_or("150")}.parse().unwrap_or(150);
                        self.canvas_engine.borrow_mut().resize_canvas(canvas,width,height)?;
                    }
                }
                if text(3).len() > 1024 * 1024 {
                    return Err("DOM attribute budget exceeded".into());
                }
                wega_dom::mutate(&mut *doc)
                    .set_attribute(
                        id,
                        QualName::new(None, ns!(), LocalName::from(text(2))),
                        text(3),
                    )
                    .map_err(Self::mutation_error)?;
                self.components.borrow_mut().attribute(id.as_u64(), text(2), old.as_deref(), Some(text(3)), None);
                Value::Null
            }
            "removeAttr" => {
                let id = node_id()?;
                let old = Self::attribute(&*doc, id, text(2));
                wega_dom::mutate(&mut *doc)
                    .clear_attribute(id, QualName::new(None, ns!(), LocalName::from(text(2))))
                    .map_err(Self::mutation_error)?;
                if old.is_some() { self.components.borrow_mut().attribute(id.as_u64(), text(2), old.as_deref(), None, None); }
                Value::Null
            }
            "removeAttrNS" => {
                let target = node_id()?;
                let name = doc.node(target).into_iter().flat_map(NodeView::attributes)
                    .find(|(name, _)| name.ns.as_str() == text(2) && name.local.as_str() == text(3))
                    .map(|(name, value)| (name.clone(), value.to_owned()));
                if let Some((name, old)) = name {
                    wega_dom::mutate(&mut *doc).clear_attribute(target, name).map_err(Self::mutation_error)?;
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
                if doc.node(child).is_none() {
                    return Err("Node no longer exists".into());
                }
                // Reject cycles, including putting a node below itself.
                let mut ancestor = Some(parent);
                while let Some(id) = ancestor {
                    if id == child {
                        return Err("Cannot create a DOM cycle".into());
                    }
                    ancestor = doc.node(id).and_then(NodeView::parent);
                }
                wega_dom::mutate(&mut *doc)
                    .append_children(parent, &[child])
                    .map_err(Self::mutation_error)?;
                json!(child.as_u64().to_string())
            }
            "remove" => {
                let id = node_id()?;
                wega_dom::mutate(&mut *doc)
                    .remove(id)
                    .map_err(Self::mutation_error)?;
                Value::Null
            }
            "scriptInserted" => {
                let node = node_id()?;
                let is_script = doc.node(node).and_then(NodeView::element_name).is_some_and(|name| name.local == "script");
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
                if self.restrictions.popups_blocked || !active || !matches!(url.scheme(), "http" | "https") || requests.opened.len() >= 4 {
                    return Ok(json!(false));
                }
                // One popup per user activation.
                self.activation.set(None);
                requests.opened.push(url);
                json!(true)
            }
            "formEntries" => {
                let form = node_id()?;
                let submitter = if text(2).is_empty() { None } else { Some(id(2)?) };
                json!(Self::form_entries(&doc, form, submitter))
            }
            "submitForm" => {
                let form = node_id()?;
                let submitter = if text(2).is_empty() { form } else { id(2)? };
                if doc.node(submitter).is_none() {
                    return Err("Node no longer exists".into());
                }
                if !self.restrictions.forms_blocked {
                    doc.submit_form(form, submitter);
                }
                Value::Null
            }
            "resetForm" => {
                let form = node_id()?;

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
            "console" => {
                self.console.borrow_mut().ingest(text(1), doc.url().as_str());
                Value::Null
            }
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
            _ => return Err("This Web API is not available".into()),
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
    fn subtree<D: DocumentView>(doc: &D, root: NodeKey) -> Vec<NodeKey> {
        wega_dom::subtree(doc, root, |key| key)
    }
    fn layout_error(error: LayoutError) -> String {
        error.to_string()
    }
    /// A style read as a script value: nodes that are not elements have no style.
    fn style_text(read: Result<(), StyleError>, css: String) -> Result<Value, String> {
        match read {
            Ok(()) | Err(StyleError::NotElement(_)) => Ok(json!(css)),
            Err(error) => Err(error.to_string()),
        }
    }
    fn attribute<D: DocumentView>(doc: &D, id: NodeKey, name: &str) -> Option<String> {
        wega_dom::attribute(doc, id, name)
    }
    fn component_subtree<D: DocumentView>(&self, doc: &D, root: NodeKey) -> Vec<NodeKey> {
        self.component_ownership_subtree(doc, root, false)
    }
    fn component_ownership_subtree<D: DocumentView>(
        &self,
        doc: &D,
        root: NodeKey,
        include_owned: bool,
    ) -> Vec<NodeKey> {
        let mut nodes = Vec::new();
        let mut pending = vec![root];
        let mut seen = HashSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(node) = doc.node(id) else {
                continue;
            };
            nodes.push(id);
            pending.extend(node.children().rev());
            if include_owned {
                pending.extend(doc.owned_fragment(id));
            }
            if let Some(root) = self.shadows.borrow().root(id.as_u64(), true)
                && let Ok(root) = root.root.parse()
            {
                pending.push(NodeKey::from_u64(root));
            }
        }
        nodes
    }
    fn component_connected<D: DocumentView>(&self, doc: &D, mut node: NodeKey) -> bool {
        let mut seen = HashSet::new();
        while seen.insert(node) {
            if node == doc.root() {
                return true;
            }
            let Some(next) = doc.node(node).and_then(NodeView::parent).or_else(|| {
                self.shadows
                    .borrow()
                    .host(node.as_u64())
                    .map(NodeKey::from_u64)
            }) else {
                return false;
            };
            node = next;
        }
        false
    }
    fn assigned_slot<D: DocumentView>(&self, doc: &D, target: NodeKey) -> Option<NodeKey> {
        let node = doc.node(target)?;
        if !matches!(node.kind(), NodeKind::Element | NodeKind::Text) {
            return None;
        }
        let parent = node.parent()?;
        let root = self
            .shadows
            .borrow()
            .root(parent.as_u64(), true)?
            .root
            .parse()
            .ok()?;
        let name = Self::attribute(doc, target, "slot").unwrap_or_default();
        Self::subtree(doc, NodeKey::from_u64(root))
            .into_iter()
            .find(|id| {
                doc.node(*id)
                    .and_then(NodeView::element_name)
                    .is_some_and(|name| name.local == "slot")
                    && Self::attribute(doc, *id, "name").unwrap_or_default() == name
            })
    }
    fn assigned_nodes<D: DocumentView>(
        &self,
        doc: &D,
        slot: NodeKey,
        flatten: bool,
        seen: &mut HashSet<NodeKey>,
    ) -> Vec<NodeKey> {
        if !seen.insert(slot) {
            return vec![];
        }
        let root = wega_dom::tree_root(doc, slot);
        let Some(host) = self
            .shadows
            .borrow()
            .host(root.as_u64())
            .map(NodeKey::from_u64)
        else {
            return vec![];
        };
        let mut assigned: Vec<_> = doc
            .node(host)
            .into_iter()
            .flat_map(|node| node.children())
            .filter(|id| self.assigned_slot(doc, *id) == Some(slot))
            .collect();
        if flatten {
            if assigned.is_empty() {
                assigned = doc
                    .node(slot)
                    .map(|node| node.children().collect())
                    .unwrap_or_default();
            }
            assigned
                .into_iter()
                .flat_map(|id| {
                    if doc
                        .node(id)
                        .and_then(NodeView::element_name)
                        .is_some_and(|element| element.local == "slot")
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
    fn collect_detached<D: wega_dom::DocumentMutation>(&self, doc: &mut D, id: NodeKey) {
        let mut root = id;
        while let Some(parent) = doc.ownership_parent(root).or_else(|| {
            self.shadows
                .borrow()
                .host(root.as_u64())
                .map(NodeKey::from_u64)
        }) {
            root = parent;
        }
        if self.component_connected(doc, root) || doc.node(root).is_none() {
            return;
        }
        let nodes = self.component_ownership_subtree(doc, root, true);
        if nodes.iter().any(|id| self.retained.borrow().contains(id)) {
            return;
        }
        let bytes: usize = nodes
            .iter()
            .filter_map(|id| doc.node(*id))
            .map(|node| {
                32 + match node.kind() {
                    NodeKind::Text | NodeKind::Comment => node.text().map_or(0, str::len),
                    NodeKind::Element | NodeKind::AnonymousBlock => node
                        .attributes()
                        .map(|(name, value)| name.local.len() + value.len())
                        .sum(),
                    NodeKind::Document => 0,
                }
            })
            .sum();
        for id in &nodes {
            self.components.borrow_mut().release(id.as_u64());
            if let Some(shadow) = self.shadows.borrow_mut().release(id.as_u64())
                && let Ok(id) = shadow.root.parse()
            {
                let _ = wega_dom::mutate(&mut *doc).remove_and_drop(NodeKey::from_u64(id));
            }
        }
        let _ = wega_dom::mutate(&mut *doc).remove_and_drop(root);
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
    pub fn form_owner<D: DocumentView>(doc: &D, control: NodeKey) -> Option<NodeKey> {
        wega_dom::content::form_owner(doc, control)
    }
    /// Resets all controls owned by `form` to their default state (defaultValue / defaultChecked).
    pub fn reset_form_controls(doc: &mut WegaDocument, form: NodeKey) {
        doc.reset_form(form);
    }
    fn form_entries(
        doc: &WegaDocument,
        form: NodeKey,
        submitter: Option<NodeKey>,
    ) -> Vec<(String, String)> {
        doc.form_data(form, submitter)
            .0
            .into_iter()
            .map(|entry| {
                (
                    entry.name,
                    match entry.value {
                        wega_network::document::net::EntryValue::String(value) => value,
                        wega_network::document::net::EntryValue::File(_)
                        | wega_network::document::net::EntryValue::EmptyFile => String::new(),
                    },
                )
            })
            .collect()
    }
    fn serialize<D: DocumentView>(doc: &D, id: NodeKey, out: &mut String) {
        wega_dom::content::serialize(doc, id, out);
    }

    fn parse_cross_frame_id(&self, candidate: &str) -> Option<(CrossFrameTarget, u64)> {
        if let Some(rest) = candidate.strip_prefix("f:") {
            let mut parts = rest.splitn(3, ':');
            let key: u64 = parts.next()?.parse().ok()?;
            let generation: u64 = parts.next()?.parse().ok()?;
            let node_id: u64 = parts.next()?.parse().ok()?;
            Some((CrossFrameTarget::Child { key, generation }, node_id))
        } else if let Some(rest) = candidate.strip_prefix("p:") {
            let mut parts = rest.splitn(2, ':');
            let generation: u64 = parts.next()?.parse().ok()?;
            let node_id: u64 = parts.next()?.parse().ok()?;
            Some((CrossFrameTarget::Parent { generation }, node_id))
        } else {
            None
        }
    }

    fn detect_cross_frame_target(
        &self,
        args: &[serde_json::Value],
    ) -> Option<(CrossFrameTarget, String)> {
        use serde_json::Value;
        let op = args.get(0).and_then(Value::as_str).unwrap_or_default();
        let candidate = match op {
            "create" | "createText" | "createComment" | "getById" => {
                args.get(2).and_then(Value::as_str)
            }
            "createNS" => args.get(3).and_then(Value::as_str),
            "createFragment" => args.get(1).and_then(Value::as_str),
            _ => args.get(1).and_then(Value::as_str),
        }?;
        let (target, _) = self.parse_cross_frame_id(candidate)?;
        let prefix = match target {
            CrossFrameTarget::Child { key, generation } => format!("f:{key}:{generation}:"),
            CrossFrameTarget::Parent { generation } => format!("p:{generation}:"),
        };
        Some((target, prefix))
    }

    fn dispatch_cross_frame(
        &self,
        args: &[serde_json::Value],
        target: CrossFrameTarget,
        prefix: &str,
    ) -> Result<serde_json::Value, String> {
        use serde_json::{Value, json};
        let caller_origin = self.origin();
        if caller_origin == "null" {
            return Err(
                "SecurityError: Opaque documents cannot access other browsing contexts".into(),
            );
        }
        let dom = match target {
            CrossFrameTarget::Child { key, generation } => {
                let shared = self.frames.borrow();
                let child = shared.children.iter().find(|c| c.key == key).cloned();
                let Some(child) = child else {
                    return Err("NotFoundError: Frame no longer exists".into());
                };
                if child.opaque || child.origin == "null" || child.origin != caller_origin {
                    return Err(
                        "SecurityError: Blocked a frame from accessing a cross-origin frame."
                            .into(),
                    );
                }
                if child.document_generation.get() != generation {
                    return Err("InvalidStateError: The frame document has navigated.".into());
                }
                child.dom.clone().ok_or("Frame has no DOM")?
            }
            CrossFrameTarget::Parent { generation } => {
                let shared = self.frames.borrow();
                let parent = shared.parent.clone().ok_or("No parent browsing context")?;
                if parent.opaque || parent.origin == "null" || parent.origin != caller_origin {
                    return Err(
                        "SecurityError: Blocked a frame from accessing a cross-origin frame."
                            .into(),
                    );
                }
                let gen_rc = parent
                    .document_generation
                    .upgrade()
                    .ok_or("Parent document detached")?;
                let dom_rc = parent.dom.upgrade().ok_or("Parent document detached")?;
                if gen_rc.get() != generation {
                    return Err("InvalidStateError: The parent document has navigated.".into());
                }
                dom_rc
            }
        };

        let mut forwarded = Vec::with_capacity(args.len());
        for arg in args {
            if let Some(s) = arg.as_str() {
                if let Some(rest) = s.strip_prefix(prefix) {
                    forwarded.push(json!(rest));
                } else {
                    forwarded.push(arg.clone());
                }
            } else {
                forwarded.push(arg.clone());
            }
        }

        let res = dom.call(&forwarded)?;

        let op = args.get(0).and_then(Value::as_str).unwrap_or_default();
        let is_node_result = matches!(
            op,
            "parent"
                | "firstChild"
                | "lastChild"
                | "nextSibling"
                | "previousSibling"
                | "childAt"
                | "children"
                | "root"
                | "query"
                | "queryAll"
                | "queryIn"
                | "queryAllIn"
                | "closest"
                | "insert"
                | "append"
                | "create"
                | "createText"
                | "createComment"
                | "createFragment"
                | "createNS"
                | "clone"
                | "subtree"
                | "getById"
                | "windowNamed"
                | "componentRoot"
                | "assignedSlot"
                | "assignedSlotInternal"
                | "assignedNodes"
                | "templateContent"
                | "eventParent"
                | "shadowHost"
                | "sheetOwners"
                | "elementFromPoint"
        );
        if is_node_result {
            match res {
                Value::String(s) if !s.is_empty() => Ok(json!(format!("{prefix}{s}"))),
                Value::Array(arr) => Ok(Value::Array(
                    arr.into_iter()
                        .map(|item| match item {
                            Value::String(s) if !s.is_empty() => json!(format!("{prefix}{s}")),
                            other => other,
                        })
                        .collect(),
                )),
                other => Ok(other),
            }
        } else {
            Ok(res)
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum CrossFrameTarget {
    Child { key: u64, generation: u64 },
    Parent { generation: u64 },
}

impl Dom {
    /// Native geometry is queried before taking the authoritative DOM borrow;
    /// layout may need to read that DOM after any JavaScript mutation.
    fn native_geometry_call(&self, args: &[Value]) -> Option<Result<Value, String>> {
        let text = |i| args.get(i).and_then(Value::as_str).unwrap_or_default();
        let command = text(0);
        if !matches!(
            command,
            "elementFromPoint"
                | "media"
                | "fragment"
                | "computed"
                | "box"
                | "clientBox"
                | "offsetBox"
                | "scrollSize"
                | "clientRects"
                | "rect"
                | "viewport"
                | "scroll"
                | "scrollViewport"
                | "scrollNode"
                | "scrollIntoView"
        ) {
            return None;
        }
        let Some(shared) = self.native_document.borrow().clone() else {
            return Some(Err("InvalidStateError: native layout is unavailable".into()));
        };
        Some((|| {
            let mut native = shared.borrow_mut();
            native.ensure_layout();
            if command == "media" {
                return Ok(json!(wega_css_parser::matches_media(
                    &wega_css_parser::parse_media_query_list(text(1)),
                    &native.media_context()
                )));
            }
            if command == "fragment" {
                let (fragment, target) = {
                    let mut doc = self.document.borrow_mut();
                    let url = doc.url().join(text(1)).map_err(|e| e.to_string())?;
                    let mut without = url.clone();
                    without.set_fragment(None);
                    let mut current = doc.url().clone();
                    current.set_fragment(None);
                    if without != current {
                        return Err("Fragment navigation must stay in the document".into());
                    }
                    let fragment = url.fragment().unwrap_or_default().to_owned();
                    let target = doc.element_by_id(&fragment).or_else(|| {
                        doc.query_selector_all("a[name]")
                            .ok()?
                            .into_iter()
                            .find(|key| {
                                doc.node(*key).and_then(|n| n.attr("name"))
                                    == Some(fragment.as_str())
                            })
                    });
                    doc.set_base_url(url);
                    (fragment, target)
                };
                if fragment.is_empty() {
                    native.scroll_viewport_to(Point::default());
                } else if let Some(target) = target {
                    let r = native.bounding_rect(target).map_err(Self::layout_error)?;
                    let scroll = native.scroll;
                    native.scroll_viewport_to(Point {
                        x: scroll.x + r.x,
                        y: scroll.y + r.y,
                    });
                }
                return Ok(Value::Null);
            }
            if command == "viewport" {
                let v = native.viewport;
                let scale = (v.scale * v.zoom) as f64;
                return Ok(
                    json!({"width":v.width_px as f64/scale,"height":v.height_px as f64/scale,"dpr":scale,"scrollX":native.scroll.x,"scrollY":native.scroll.y}),
                );
            }
            let number = |i| {
                text(i)
                    .parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .ok_or_else(|| "Invalid scroll offset".to_owned())
            };
            if command == "scrollViewport" {
                native.scroll_viewport_to(Point {
                    x: number(1)?,
                    y: number(2)?,
                });
                return Ok(Value::Null);
            }
            if command == "elementFromPoint" {
                let p = Point {
                    x: number(2)?,
                    y: number(3)?,
                };
                let v = native.media_context();
                if p.x < 0.0 || p.y < 0.0 || p.x >= v.viewport_width || p.y >= v.viewport_height {
                    return Ok(Value::Null);
                }
                return Ok(json!(native.element_at(p).map(|k| k.to_string())));
            }
            let key = wega_dom::NodeKey::from_u64(
                text(1)
                    .parse::<u64>()
                    .map_err(|_| "Invalid node identity")?,
            );
            let rjson = |r: Rect| json!({"x":r.x,"y":r.y,"width":r.width,"height":r.height,"left":r.x,"top":r.y,"right":r.right(),"bottom":r.bottom()});
            if native.doc.borrow().node(key).is_none() {
                match command {
                    "rect" => return Ok(rjson(Rect::ZERO)),
                    "clientRects" => return Ok(json!([])),
                    "clientBox" => {
                        return Ok(json!({"left":0.0,"top":0.0,"width":0.0,"height":0.0}));
                    }
                    "offsetBox" => {
                        return Ok(json!({"x":0.0,"y":0.0,"width":0.0,"height":0.0,"parent":null}));
                    }
                    "scrollSize" => return Ok(json!({"width":0.0,"height":0.0})),
                    "scroll" => return Ok(json!({"x":0.0,"y":0.0})),
                    "box" => {
                        return Ok(
                            json!({"offsetWidth":0.0,"offsetHeight":0.0,"contentWidth":0.0,"contentHeight":0.0,"paddingLeft":0.0,"paddingTop":0.0}),
                        );
                    }
                    _ => return Err("Node no longer exists".into()),
                }
            }
            match command {
                "computed" => {
                    let mut css = String::new();
                    native
                        .resolved_value(key, text(2), &mut css)
                        .map_err(|e| format!("{e:?}"))?;
                    Ok(json!(css))
                }
                "rect" => Ok(rjson(native.bounding_rect(key).unwrap_or(Rect::ZERO))),
                "box" => {
                    let offset = native.offset_box(key).map_err(Self::layout_error)?.rect;
                    let content = native.content_box(key).map_err(Self::layout_error)?;
                    Ok(
                        json!({"offsetWidth":offset.width,"offsetHeight":offset.height,"contentWidth":content.size.width,"contentHeight":content.size.height,"paddingLeft":content.padding_left,"paddingTop":content.padding_top}),
                    )
                }
                "clientBox" => {
                    let b = native.client_box(key).map_err(Self::layout_error)?;
                    Ok(json!({"left":b.left,"top":b.top,"width":b.width,"height":b.height}))
                }
                "offsetBox" => {
                    let b = native.offset_box(key).map_err(Self::layout_error)?;
                    Ok(
                        json!({"x":b.rect.x,"y":b.rect.y,"width":b.rect.width,"height":b.rect.height,"parent":b.parent.map(|p|p.to_string())}),
                    )
                }
                "scrollSize" => {
                    let size = native.scroll_size(key).map_err(Self::layout_error)?;
                    Ok(json!({"width":size.width,"height":size.height}))
                }
                "scroll" => {
                    let p = native.scroll_offset(key).map_err(Self::layout_error)?;
                    Ok(json!({"x":p.x,"y":p.y}))
                }
                "clientRects" => Ok(json!(
                    native
                        .client_rects(key)
                        .map_err(Self::layout_error)?
                        .into_iter()
                        .map(rjson)
                        .collect::<Vec<_>>()
                )),
                "scrollNode" => {
                    native
                        .scroll_node_to(
                            key,
                            Point {
                                x: number(2)?,
                                y: number(3)?,
                            },
                        )
                        .map_err(Self::layout_error)?;
                    Ok(Value::Null)
                }
                "scrollIntoView" => {
                    let r = native.bounding_rect(key).map_err(Self::layout_error)?;
                    let current = native.scroll;
                    native.scroll_viewport_to(Point {
                        x: current.x + r.x,
                        y: current.y + r.y,
                    });
                    Ok(Value::Null)
                }
                _ => unreachable!(),
            }
        })())
    }
}
