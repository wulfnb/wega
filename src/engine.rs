//! Servo delegates queue work for the UI; they never borrow browser state during spin_event_loop.
use servo::{self, WebView, WebViewId};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};
use winit::{event_loop::EventLoopProxy, window::Window};

#[derive(Clone, Debug)]
pub enum Wake {
    ContainerLaunch(Vec<String>),
    ContainerError(String),
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    OpenUrls(Vec<String>),
    Servo,
    Ui,
    Media(servo::MediaSessionActionType),
    ToggleMedia,
    Accessibility(std::sync::Arc<egui_winit::accesskit_winit::Event>),
}

impl From<egui_winit::accesskit_winit::Event> for Wake {
    fn from(event: egui_winit::accesskit_winit::Event) -> Self {
        Self::Accessibility(std::sync::Arc::new(event))
    }
}

#[derive(Clone)]
pub struct Waker(pub EventLoopProxy<Wake>);

impl servo::EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn servo::EventLoopWaker> {
        Box::new(self.clone())
    }
    fn wake(&self) {
        let _ = self.0.send_event(Wake::Servo);
    }
}

pub enum Event {
    PageState(u64, u64, bool),
    Download(String, String),
    Changed,
    Load(servo::WebViewId, servo::LoadStatus),
    Traversal(servo::WebViewId, servo::TraversalId),
    History(servo::WebViewId, Vec<String>, usize),
    Bluetooth(servo::BluetoothDeviceSelectionRequest),
    Media(servo::WebViewId, servo::MediaSessionEvent),
    Favicon(String, servo::Image),
    Prompt(crate::capabilities::Prompt),
    Status(String),
    Notice(crate::ux::Notice),
    NativeAccessibility(WebViewId, servo::accesskit::TreeUpdate),
    Settings,
    Closed(WebViewId),
    Crashed(WebViewId, String),
    Popup(servo::CreateNewWebViewRequest, Option<url::Url>),
    Permission(String, servo::PermissionRequest),
    Authentication(servo::AuthenticationRequest),
    Control(WebViewId, servo::EmbedderControl),
    HideControl(servo::EmbedderControlId),
    Console(WebViewId, String),
    Visited(String, String),
    Accessibility(WebViewId, Vec<(String, serde_json::Value)>),
    Fullscreen(bool),
}

fn origin(view: &WebView) -> String {
    view.url()
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_default()
}

pub struct Delegate {
    pub window: Rc<Window>,
    pub active_view: Cell<Option<WebViewId>>,
    pub policy: RefCell<crate::capabilities::Policy>,
    pub handlers: RefCell<std::collections::HashMap<String, url::Url>>,
    /// Opener document of script-created views, used as the initiator of their first load.
    pub openers: RefCell<std::collections::HashMap<WebViewId, url::Url>>,
    pub events: RefCell<VecDeque<Event>>,
}

impl Delegate {
    pub fn report(&self, message: String) {
        self.push(Event::Status(message));
    }
    pub fn notice(
        &self,
        severity: crate::ux::Severity,
        message: String,
        path: Option<std::path::PathBuf>,
    ) {
        self.push(Event::Notice(crate::ux::Notice::new(
            severity, message, path,
        )));
    }
    fn push(&self, event: Event) {
        self.events.borrow_mut().push_back(event);
        self.window.request_redraw();
    }
}

impl servo::WebViewDelegate for Delegate {
    fn show_bluetooth_device_dialog(
        &self,
        _: WebView,
        request: servo::BluetoothDeviceSelectionRequest,
    ) {
        self.push(Event::Bluetooth(request));
    }
    fn notify_media_session_event(&self, view: WebView, event: servo::MediaSessionEvent) {
        self.push(Event::Media(view.id(), event));
    }
    fn show_notification(&self, view: WebView, notification: servo::Notification) {
        if let Err(e) = crate::native_notifications::show(
            notification,
            &view
                .url()
                .map(|u| u.origin().ascii_serialization())
                .unwrap_or_default(),
        ) {
            self.report(e);
        }
    }
    fn request_unload(&self, view: WebView, request: servo::AllowOrDenyRequest) {
        self.push(Event::Prompt(crate::capabilities::Prompt {
            origin: origin(&view),
            title: "Leave site?".into(),
            detail: format!(
                "{} may have changes that have not been saved.",
                view.url()
                    .map(|u| u.origin().ascii_serialization())
                    .unwrap_or_default()
            ),
            decision: crate::capabilities::Decision::Servo(request),
        }));
    }
    fn request_navigation(&self, view: WebView, request: servo::NavigationRequest) {
        if request.url.scheme() == "wega"
            && request.url.host_str() == Some("settings")
            && view.url().is_some_and(|u| u.scheme() == "wega")
        {
            self.push(Event::Settings);
        }

        if request.url.scheme() == "wega" && request.url.host_str() == Some("download") {
            let fields: std::collections::HashMap<_, _> =
                request.url.query_pairs().into_owned().collect();
            if let Some(url) = fields.get("url") {
                self.push(Event::Download(
                    url.clone(),
                    fields.get("name").cloned().unwrap_or_default(),
                ));
            }
            request.deny();
            return;
        }
        if self.handlers.borrow().contains_key(request.url.scheme()) {
            request.allow();
            return;
        }
        match request.url.scheme() {
            "http" | "https" if !self.policy.borrow().permits(&request.url) => {
                self.push(Event::Status(format!(
                    "Navigation blocked: {}",
                    request.url
                )));
                request.deny();
            }
            "data" | "file"
                if !wega::navigation::content_navigation_allowed(
                    &request.url,
                    view.url()
                        .filter(|url| url.as_str() != "about:blank")
                        .or_else(|| self.openers.borrow().get(&view.id()).cloned())
                        .as_ref(),
                ) =>
            {
                self.push(Event::Status(format!(
                    "Blocked a page from opening {}: URLs. Enter the address yourself to open it.",
                    request.url.scheme()
                )));
                request.deny();
            }
            "http" | "https" | "about" | "data" | "file" | "blob" | "wega" => request.allow(),
            "mailto" | "tel" | "sms" | "zoommtg" => {
                self.push(Event::Prompt(crate::capabilities::Prompt {
                    origin: origin(&view),
                    title: "Open another application?".into(),
                    detail: request.url.to_string(),
                    decision: crate::capabilities::Decision::External(request.url.clone()),
                }));
                request.deny();
            }
            _ => {
                self.push(Event::Status(format!(
                    "Unsupported external protocol: {}",
                    request.url.scheme()
                )));
                request.deny();
            }
        }
    }
    fn request_protocol_handler(
        &self,
        view: WebView,
        registration: servo::protocol_handler::ProtocolHandlerRegistration,
        request: servo::AllowOrDenyRequest,
    ) {
        self.push(Event::Prompt(crate::capabilities::Prompt {
            origin: origin(&view),
            title: "Register link handler?".into(),
            detail: format!(
                "{} requests {:?} for {} using {}",
                view.url()
                    .map(|u| u.origin().ascii_serialization())
                    .unwrap_or_default(),
                registration.register_or_unregister,
                registration.scheme,
                registration.url
            ),
            decision: crate::capabilities::Decision::Protocol(request, registration),
        }));
    }
    fn load_web_resource(&self, _: WebView, load: servo::WebResourceLoad) {
        crate::capabilities::intercept(&self.policy.borrow(), load);
    }
    fn notify_favicon_changed(&self, view: WebView) {
        if let (Some(url), Some(image)) = (view.url(), view.favicon())
            && image.width <= 256
            && image.height <= 256
        {
            self.push(Event::Favicon(url.to_string(), image.clone()));
        }
    }
    fn notify_history_changed(&self, view: WebView, urls: Vec<url::Url>, active: usize) {
        self.push(Event::History(
            view.id(),
            urls.into_iter().map(|url| url.to_string()).collect(),
            active,
        ));
        self.push(Event::Changed);
    }
    fn notify_traversal_complete(&self, view: WebView, id: servo::TraversalId) {
        self.push(Event::Traversal(view.id(), id));
    }
    fn screen_geometry(&self, _: WebView) -> Option<servo::ScreenGeometry> {
        let monitor = self.window.current_monitor()?;
        let size = monitor.size();
        let outer = self.window.outer_size();
        let pos = self.window.outer_position().unwrap_or_default();
        Some(servo::ScreenGeometry {
            size: servo::DeviceIntSize::new(size.width as i32, size.height as i32),
            available_size: {
                let (w, h) = crate::native_theme::available_size(&self.window)
                    .unwrap_or((size.width as i32, size.height as i32));
                servo::DeviceIntSize::new(w, h)
            },
            window_rect: servo::DeviceIntRect::from_origin_and_size(
                servo::DeviceIntPoint::new(pos.x, pos.y),
                servo::DeviceIntSize::new(outer.width as i32, outer.height as i32),
            ),
        })
    }
    fn request_move_to(&self, _: WebView, point: servo::DeviceIntPoint) {
        if self.policy.borrow().allow_window_changes {
            self.window
                .set_outer_position(winit::dpi::PhysicalPosition::new(point.x, point.y));
        }
    }
    fn request_resize_to(&self, _: WebView, size: servo::DeviceIntSize) {
        if self.policy.borrow().allow_window_changes {
            let max = self
                .window
                .current_monitor()
                .map(|m| m.size())
                .unwrap_or(winit::dpi::PhysicalSize::new(3840, 2160));
            let _ = self
                .window
                .request_inner_size(winit::dpi::PhysicalSize::new(
                    (size.width as u32).clamp(720, max.width.max(720)),
                    (size.height as u32).clamp(480, max.height.max(480)),
                ));
        }
    }
    fn notify_accessibility_tree_update(
        &self,
        view: WebView,
        update: servo::accesskit::TreeUpdate,
    ) {
        self.push(Event::NativeAccessibility(view.id(), update.clone()));
        let tree = update.tree_id;
        let nodes = update
            .nodes
            .into_iter()
            .take(2000)
            .map(|(id, node)| {
                (
                    format!("{tree:?}/{id:?}"),
                    serde_json::to_value(node).unwrap_or(serde_json::Value::Null),
                )
            })
            .collect();
        self.push(Event::Accessibility(view.id(), nodes));
    }
    fn notify_new_frame_ready(&self, view: WebView) {
        // Hidden offscreen contexts may stay dirty until selected and painted.
        // Repainting the active view cannot clear their dirty flag, so forwarding
        // those notifications creates an endless foreground redraw loop.
        if self.active_view.get() == Some(view.id()) {
            self.window.request_redraw();
        }
    }
    fn notify_url_changed(&self, _: WebView, _: url::Url) {
        self.push(Event::Changed);
    }
    fn notify_page_title_changed(&self, _: WebView, _: Option<String>) {
        self.push(Event::Changed);
    }
    fn notify_cursor_changed(&self, _: WebView, _: servo::Cursor) {
        self.window.request_redraw();
    }
    fn notify_load_status_changed(&self, view: WebView, status: servo::LoadStatus) {
        self.push(Event::Load(view.id(), status));
        if status == servo::LoadStatus::Complete
            && let Some(url) = view.url()
        {
            self.push(Event::Visited(
                url.to_string(),
                view.page_title().unwrap_or_default(),
            ));
        }
        self.push(Event::Changed);
    }
    fn notify_animating_changed(&self, _: WebView, _: bool) {
        self.window.request_redraw();
    }
    fn notify_closed(&self, view: WebView) {
        self.push(Event::Closed(view.id()));
    }
    fn notify_crashed(&self, view: WebView, reason: String, _: Option<String>) {
        self.push(Event::Crashed(view.id(), reason));
    }
    fn request_create_new(&self, parent: WebView, request: servo::CreateNewWebViewRequest) {
        self.push(Event::Popup(request, parent.url()));
    }
    fn request_permission(&self, view: WebView, request: servo::PermissionRequest) {
        self.push(Event::Permission(
            view.url()
                .map(|u| u.origin().ascii_serialization())
                .unwrap_or_default(),
            request,
        ));
    }
    fn request_authentication(&self, _: WebView, request: servo::AuthenticationRequest) {
        self.push(Event::Authentication(request));
    }
    fn show_embedder_control(&self, view: WebView, control: servo::EmbedderControl) {
        self.push(Event::Control(view.id(), control));
    }
    fn hide_embedder_control(&self, _: WebView, id: servo::EmbedderControlId) {
        self.push(Event::HideControl(id));
    }
    fn show_console_message(&self, view: WebView, level: servo::ConsoleLogLevel, message: String) {
        if matches!(format!("{level:?}").as_str(), "Error" | "Warn" | "Assert") {
            let mut diagnostic = wega::diagnostic_event::Diagnostic::error(
                wega::diagnostic_event::Category::Javascript,
                message.clone(),
                view.url().map(|u| u.to_string()),
            );
            if format!("{level:?}") == "Warn" {
                diagnostic.severity = "warning".into();
            }
            wega::diagnostics::report(&format!("servo:{:?}:console", view.id()), diagnostic);
        }
        self.push(Event::Console(view.id(), format!("{level:?}: {message}")));
    }
    fn notify_fullscreen_state_changed(&self, _: WebView, fullscreen: bool) {
        self.push(Event::Fullscreen(fullscreen));
    }
}

pub struct Clipboard;

impl servo::ClipboardDelegate for Clipboard {
    fn get_text(&self, _: WebView, request: servo::StringRequest) {
        match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
            Ok(text) => request.success(text),
            Err(e) => request.failure(e.to_string()),
        }
    }
    fn set_text(&self, _: WebView, text: String) {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(text);
        }
    }
    fn clear(&self, _: WebView) {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.clear();
        }
    }
}

impl servo::ServoDelegate for Delegate {
    fn show_notification(&self, notification: servo::Notification) {
        if let Err(e) =
            crate::native_notifications::show(notification, "Background web notification")
        {
            self.report(e);
        }
    }
    fn notify_error(&self, error: servo::ServoError) {
        wega::diagnostics::report(
            "servo:engine",
            wega::diagnostic_event::Diagnostic::error(
                wega::diagnostic_event::Category::Browser,
                format!("{error:?}"),
                None,
            ),
        );
        self.push(Event::Status(format!("Servo error: {error:?}")));
    }
    fn notify_devtools_server_started(&self, port: u16, _: String) {
        self.push(Event::Status(format!(
            "Remote DevTools listening on port {port}"
        )));
    }
    fn request_devtools_connection(&self, request: servo::AllowOrDenyRequest) {
        self.push(Event::Prompt(crate::capabilities::Prompt {
            origin: "devtools".into(),
            title: "Allow remote developer tools?".into(),
            detail: "The connecting debugger can read and change open pages.".into(),
            decision: crate::capabilities::Decision::Servo(request),
        }));
    }
    fn load_web_resource(&self, load: servo::WebResourceLoad) {
        crate::capabilities::intercept(&self.policy.borrow(), load);
    }
    fn show_console_message(&self, level: servo::ConsoleLogLevel, message: String) {
        if matches!(format!("{level:?}").as_str(), "Error" | "Warn" | "Assert") {
            let mut diagnostic = wega::diagnostic_event::Diagnostic::error(
                wega::diagnostic_event::Category::Javascript,
                message.clone(),
                None,
            );
            if format!("{level:?}") == "Warn" {
                diagnostic.severity = "warning".into();
            }
            wega::diagnostics::report("servo:console", diagnostic);
        }
        self.push(Event::Status(format!("{level:?}: {message}")));
    }
}
