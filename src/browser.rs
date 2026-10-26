use crate::{
    Options,
    engine::{self, Event, Wake},
    input,
    profile::{self, Bookmark, Profile, TabLayout},
};
use servo::{InputEvent, RenderingContext, WebView, WebViewBuilder, WindowRenderingContext};
use std::cell::RefCell;
use std::{
    collections::VecDeque,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use wega::{
    application::{Application, Command as AppCommand, Event as AppEvent},
    engine_choice::EngineKind,
    session::{Command, Session},
    storage::FileProfileStore,
};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoopProxy},
    keyboard::{Key, NamedKey},
    raw_window_handle::{HasDisplayHandle, HasWindowHandle},
    window::{Fullscreen, Window},
};

/// Engine resources of a loaded tab.
pub struct Live {
    pub(crate) view: WebView,
    pub(crate) surface: Rc<servo::OffscreenRenderingContext>,
}

/// A tab is either loaded (`live`) or unloaded: restored but not yet selected, or
/// discarded after being idle. Unloaded tabs keep only their address, title and
/// lifecycle data, and get a fresh engine view when they are selected.
pub struct Tab {
    pub(crate) live: Option<Live>,
    pub(crate) wega: Option<crate::wega_page::Page>,
    pub(crate) engine: EngineKind,
    /// Stable for the life of the tab, across unloading and reloading.
    pub(crate) key: u64,
    pub(crate) initial_url: String,
    pub(crate) saved_title: String,
    pub(crate) crash: Option<String>,
    pub(crate) history: wega::navigation::History,
    pub(crate) last_selected: Instant,
    /// Script-opened views keep an opener relationship; never discard them.
    pub(crate) opened_by_page: bool,
}

impl Tab {
    pub(crate) fn view(&self) -> Option<&WebView> {
        self.live.as_ref().map(|live| &live.view)
    }
    pub(crate) fn id(&self) -> Option<servo::WebViewId> {
        self.view().map(WebView::id)
    }
    pub(crate) fn loaded(&self) -> bool {
        self.live.is_some() || self.wega.is_some()
    }
    /// WebDriver handle and drag-and-drop identity; survives unloading.
    pub(crate) fn handle(&self) -> String {
        format!("tab-{}", self.key)
    }
    pub(crate) fn url(&self) -> String {
        if let Some(page) = &self.wega {
            return page.url();
        }
        self.view()
            .and_then(WebView::url)
            .map(|u| u.to_string())
            .unwrap_or_else(|| self.initial_url.clone())
    }
    pub(crate) fn title(&self) -> String {
        if let Some(page) = &self.wega {
            let title = page.title();
            return if title.is_empty() {
                url::Url::parse(&page.url())
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_owned))
                    .unwrap_or_else(|| "Wega page".into())
            } else {
                title
            };
        }
        let fallback = |url: Option<url::Url>| {
            url.and_then(|u| u.host_str().map(str::to_owned))
                .unwrap_or_else(|| "New tab".into())
        };
        match self.view() {
            Some(view) => view
                .page_title()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| fallback(view.url())),
            None if !self.saved_title.is_empty() => self.saved_title.clone(),
            None => fallback(url::Url::parse(&self.initial_url).ok()),
        }
    }
    pub(crate) fn loading(&self) -> bool {
        if let Some(page) = &self.wega {
            return page.loading();
        }
        self.view()
            .is_some_and(|view| view.load_status() != servo::LoadStatus::Complete)
    }
}

pub struct Browser {
    pub(crate) chrome: crate::chrome::Chrome,
    pub(crate) automation: Option<crossbeam_channel::Receiver<crate::automation::Job>>,
    pub(crate) automation_navigation: Option<crate::automation::PendingNavigation>,
    #[cfg(feature = "test-harness")]
    pub(crate) harness: crate::harness::Harness,
    pub(crate) favicons: std::collections::HashMap<String, egui::TextureHandle>,
    pub(crate) features: crate::capabilities::State,
    pub(crate) documents: crate::capabilities::Documents,
    pub(crate) window: Rc<Window>,
    pub(crate) event_proxy: EventLoopProxy<Wake>,
    pub(crate) native_media: crate::native_media::NativeMedia,
    pub(crate) native_theme: Option<crate::native_theme::NativeTheme>,
    session: Session<Tab>,
    pub(crate) servo: crate::servo_host::Host,
    pub(crate) content_manager: std::cell::OnceCell<Rc<servo::UserContentManager>>,
    pub(crate) context: Rc<WindowRenderingContext>,
    pub(crate) delegate: Rc<engine::Delegate>,
    pub(crate) gui: Option<egui_glow::EguiGlow>,
    application: Application,
    pub(crate) profile_dir: PathBuf,
    last_window_title: String,
    pub(crate) tool_window: Option<crate::tool_window::ToolWindow>,
    pub(crate) tools: crate::toolbox::Toolbox,
    pub(crate) console: VecDeque<(servo::WebViewId, String)>,
    pub(crate) dialogs: crate::capabilities::Dialogs,
    pub(crate) status: Option<String>,
    pub(crate) accessibility: crate::accessibility::State,
    pub(crate) resources: crate::resources::Monitor,
    pub(crate) downloads: crate::downloads::Manager,
    pub(crate) download_ui: crate::downloads_ui::State,
    pub(crate) fullscreen: bool,
    pub(crate) quit_requested: bool,
    pub(crate) destructive_action: Option<crate::page_safety::Action>,
    pub(crate) repaint_deadline: Arc<std::sync::Mutex<Option<Instant>>>,
    pub(crate) pending_save: Option<Instant>,
    pub(crate) capture: Option<crate::capture::Capture>,
    next_tab_key: u64,
    discard_checked: Instant,
    /// The window is minimized or fully covered; skip work that only updates pixels.
    pub(crate) occluded: bool,
    pub(crate) focused: bool,
}

impl Browser {
    pub(crate) fn new(
        window: Window,
        event_loop: &ActiveEventLoop,
        proxy: EventLoopProxy<Wake>,
        options: &Options,
    ) -> Result<Self, String> {
        let window = Rc::new(window);
        let profile_dir = options
            .profile_dir
            .clone()
            .unwrap_or_else(Profile::default_dir);
        let mut downloads = crate::downloads::Manager::new(&profile_dir);
        if options.download_test {
            downloads.directory = profile_dir.join("test-downloads");
        }
        downloads.marker = Some(Arc::new(crate::download_actions::mark_downloaded));
        std::fs::create_dir_all(&profile_dir).map_err(|e| format!("Cannot create profile: {e}"))?;
        let (application, warning) =
            Application::load(Box::new(FileProfileStore::open(&profile_dir)?));
        let profile = application.profile();
        let context = Rc::new(
            WindowRenderingContext::new(
                event_loop.display_handle().map_err(|e| e.to_string())?,
                window.window_handle().map_err(|e| e.to_string())?,
                window.inner_size(),
            )
            .map_err(|e| format!("Cannot create GPU context: {e:?}"))?,
        );
        context
            .make_current()
            .map_err(|e| format!("Cannot activate GPU context: {e:?}"))?;
        let mut gui = egui_glow::EguiGlow::new(
            event_loop,
            context.glow_gl_api(),
            None,
            Some(window.scale_factor() as f32),
            false,
        );
        gui.egui_winit
            .init_accesskit(event_loop, &window, proxy.clone());
        let native_theme = crate::native_theme::NativeTheme::new(&window).ok();
        let system_dark = window.theme() != Some(winit::window::Theme::Light);
        if let Some(native) = &native_theme {
            native.apply(profile.theme, profile.theme.dark(system_dark));
        }
        crate::theme::install(
            &gui.egui_ctx,
            profile.theme.palette(
                system_dark,
                native_theme
                    .as_ref()
                    .is_some_and(|native| native.available()),
            ),
        );
        let repaint_proxy = proxy.clone();
        let repaint_deadline = Arc::new(std::sync::Mutex::new(None::<Instant>));
        let deadline = repaint_deadline.clone();
        gui.egui_ctx.set_request_repaint_callback(move |info| {
            if let Some(when) = Instant::now().checked_add(info.delay) {
                let mut deadline = deadline
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if deadline.is_none_or(|current| when < current) {
                    *deadline = Some(when);
                }
            }
            if info.delay.is_zero() {
                let _ = repaint_proxy.send_event(Wake::Ui);
            }
        });
        let engine_dir = profile_dir.join("engine");
        std::fs::create_dir_all(&engine_dir).map_err(|e| e.to_string())?;
        let opts = servo::Opts {
            config_dir: Some(engine_dir),
            multiprocess: options.multiprocess,
            ..Default::default()
        };
        let tools = crate::toolbox::Toolbox::new(proxy.clone())?;
        let native_media = crate::native_media::NativeMedia::new(proxy.clone());
        let documents = Default::default();
        let mut protocols = servo::protocol_handler::ProtocolRegistry::default();
        protocols
            .register(
                "wega",
                crate::capabilities::InternalPages(std::sync::Arc::clone(&documents)),
            )
            .map_err(|e| format!("Internal protocol: {e:?}"))?;
        let automation = options
            .webdriver_port
            .map(|port| crate::automation::start(port, proxy.clone()))
            .transpose()?;
        let mut builder = servo::ServoBuilder::default()
            .protocol_registry(protocols)
            .preferences(servo::Preferences {
                devtools_server_enabled: options.remote_devtools.is_some(),
                accessibility_enabled: true,
                // Exercise permission responders in the native harness without
                // exposing Servo's experimental notification API in releases.
                #[cfg(feature = "test-harness")]
                dom_notification_enabled: true,
                devtools_server_listen_address: format!(
                    "127.0.0.1:{}",
                    options.remote_devtools.unwrap_or(7000)
                ),
                ..Default::default()
            })
            .opts(opts)
            .event_loop_waker(Box::new(engine::Waker(proxy.clone())));
        if options.remote_devtools.is_none() {
            builder = builder.devtools_event_sender(tools.bridge.sender.clone());
        }
        let delegate = Rc::new(engine::Delegate {
            active_view: std::cell::Cell::new(None),
            window: window.clone(),
            events: RefCell::new(VecDeque::new()),
            policy: RefCell::new(profile.policy.clone()),
            handlers: RefCell::new(Default::default()),
            openers: RefCell::new(Default::default()),
        });
        let servo = crate::servo_host::Host::new(builder, delegate.clone());
        let content_manager = std::cell::OnceCell::new();
        let features = crate::capabilities::State {
            script_source: profile.policy.user_script.clone(),
            ..Default::default()
        };
        let favicons = profile
            .favicons
            .iter()
            .filter(|(_, image)| image.valid(64))
            .filter_map(|(url, image)| {
                let (width, height, rgba) = decode_favicon(image)?;
                Some((
                    url.clone(),
                    gui.egui_ctx.load_texture(
                        "favicon",
                        egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba),
                        egui::TextureOptions::LINEAR,
                    ),
                ))
            })
            .collect();
        let mut browser = Self {
            chrome: Default::default(),
            #[cfg(feature = "test-harness")]
            harness: crate::harness::Harness::new(options),
            automation,
            automation_navigation: None,
            features,
            documents,
            favicons,
            window,
            event_proxy: proxy.clone(),
            native_theme,
            native_media,
            session: Session::default(),
            servo,
            content_manager,
            context,
            delegate,
            gui: Some(gui),
            application,
            profile_dir,
            last_window_title: String::new(),
            resources: crate::resources::Monitor::new(),
            downloads,
            download_ui: Default::default(),
            tool_window: None,
            tools,
            console: VecDeque::new(),
            dialogs: Default::default(),
            accessibility: Default::default(),
            status: if options.remote_devtools.is_some() {
                Some("Remote DevTools mode: native Network/Debugger recording is disabled. Connect Firefox via about:debugging.".into())
            } else {
                warning
            },
            fullscreen: false,
            quit_requested: false,
            destructive_action: None,
            repaint_deadline,
            pending_save: None,
            next_tab_key: 0,
            discard_checked: Instant::now(),
            occluded: false,
            focused: true,
            capture: options
                .capture
                .as_ref()
                .map(|path| crate::capture::Capture::new(path.clone())),
        };
        #[cfg(all(target_os = "windows", not(feature = "media")))]
        browser.notify(
            crate::ux::Severity::Warning,
            "Video and audio are not available in this build.",
            None,
        );
        let restore = options.url.is_none() && !options.test_mode();
        let urls = if (options.smoke_test || options.theme_test || options.capabilities_test)
            && options.url.is_none()
        {
            vec![
                url::Url::from_file_path(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/fixtures/interaction.html"),
                )
                .map_err(|_| "Invalid fixture path")?
                .to_string(),
            ]
        } else if options.toolbox_test && options.url.is_none() {
            vec!["http://127.0.0.1:8767/toolbox.html".into()]
        } else if let Some(url) = &options.url {
            vec![browser.resolve(url)?.to_string()]
        } else {
            browser.profile().urls.clone()
        };
        let titles = if restore {
            browser.profile().titles.clone()
        } else {
            Vec::new()
        };
        let saved_active = browser.profile().active;
        // Restored tabs start unloaded; only the selected one gets an engine view below.
        for (index, address) in urls.iter().take(100).enumerate() {
            if let Ok(url) = browser.resolve(address) {
                let title = titles.get(index).cloned().unwrap_or_default();
                let tab_index = browser.push_tab(None, url.to_string(), title, false);
                browser.session.tabs_mut()[tab_index].engine =
                    options.engine.unwrap_or_else(|| {
                        browser
                            .application
                            .profile()
                            .engines
                            .get(index)
                            .copied()
                            .unwrap_or_default()
                    });
            }
        }
        if browser.session.tabs().is_empty() {
            browser.push_tab(None, profile::HOME.into(), String::new(), false);
        }
        browser.select_tab(saved_active.min(browser.session.tabs().len() - 1));
        browser.migrate_favicons();
        browser.window.set_visible(true);
        browser.window.set_ime_allowed(true);
        browser.window.request_redraw();
        Ok(browser)
    }

    pub(crate) fn session(&self) -> &Session<Tab> {
        &self.session
    }
    pub(crate) fn move_tab(&mut self, from: usize, to: usize) {
        if !matches!(
            self.session.apply(Command::Move { from, to }),
            wega::session::Event::Unchanged
        ) {
            self.changed();
        }
    }
    pub(crate) fn pending_permission(&self) -> Option<(wega::permissions::RequestId, &str, &str)> {
        self.dialogs.permissions.front()
    }
    pub(crate) fn answer_permission(&mut self, id: wega::permissions::RequestId, allow: bool) {
        self.dialogs.permissions.answer(id, allow);
    }
    pub(crate) fn profile(&self) -> &Profile {
        self.application.profile()
    }

    pub(crate) fn apply(&mut self, command: AppCommand) {
        match self.application.apply(command) {
            AppEvent::PolicyChanged => {
                *self.delegate.policy.borrow_mut() = self.profile().policy.clone();
                for page in self.session.tabs().iter().filter_map(|t| t.wega.as_ref()) {
                    let policy = &self.profile().policy;
                    let _ = page.send(wega::engine_protocol::Command::Policy {
                        offline: policy.offline,
                        blocked_hosts: policy.blocked_hosts.clone(),
                        allowed_hosts: policy.allowed_hosts.clone(),
                    });
                }
                self.changed();
            }
            AppEvent::Changed => self.changed(),
            AppEvent::Unchanged => {}
        }
    }

    pub(crate) fn start_download(
        &mut self,
        url: &str,
        filename: &str,
        connections: usize,
        cookies: String,
    ) -> Result<(), String> {
        self.application
            .start_download(&mut self.downloads, url, filename, connections, cookies)
    }

    pub(crate) fn tab(&self) -> &Tab {
        &self.session.tabs()[self.session.active()]
    }
    pub(crate) fn is_wega(&self) -> bool {
        self.tab().engine == EngineKind::Wega
    }
    pub(crate) fn current_url(&self) -> Option<url::Url> {
        url::Url::parse(&self.tab().url()).ok()
    }
    pub(crate) fn page_zoom(&self) -> f32 {
        self.tab()
            .wega
            .as_ref()
            .map_or_else(|| self.view().page_zoom(), |page| page.zoom)
    }
    pub(crate) fn can_go_back(&self) -> bool {
        self.tab()
            .wega
            .as_ref()
            .map_or_else(|| self.view().can_go_back(), |page| page.can_back())
    }
    pub(crate) fn can_go_forward(&self) -> bool {
        self.tab()
            .wega
            .as_ref()
            .map_or_else(|| self.view().can_go_forward(), |page| page.can_forward())
    }
    pub(crate) fn wega_mut(&mut self) -> Option<&mut crate::wega_page::Page> {
        let active = self.session.active();
        self.session.tabs_mut()[active].wega.as_mut()
    }
    pub(crate) fn switch_engine(&mut self, target: EngineKind) {
        if self.confirm_page_action(crate::page_safety::Action::Switch(self.tab().key, target)) {
            return;
        }
        self.switch_engine_confirmed(target);
    }
    pub(crate) fn switch_engine_confirmed(&mut self, target: EngineKind) {
        let url = self
            .current_url()
            .unwrap_or_else(|| url::Url::parse("about:blank").unwrap());
        if target == EngineKind::Wega
            && !matches!(url.scheme(), "http" | "https" | "file" | "about")
        {
            self.notify(crate::ux::Severity::Warning,"Open a web or HTML file page before choosing Wega. Internal browser pages use Servo.",None);
            return;
        }
        let page = if target == EngineKind::Wega {
            match crate::wega_page::Page::new(
                &url,
                &self.profile().policy,
                self.event_proxy.clone(),
                &self.profile_dir,
            ) {
                Ok(page) => Some(page),
                Err(error) => {
                    self.notify(crate::ux::Severity::Error, error, None);
                    return;
                }
            }
        } else {
            None
        };
        self.release_pointer();
        if let Some(id) = self.tab().id() {
            self.forget_view(id);
        }
        let active = self.session.active();
        let tab = &mut self.session.tabs_mut()[active];
        tab.initial_url = url.to_string();
        tab.saved_title = tab.title();
        tab.live = None;
        tab.wega = page;
        tab.engine = target;
        tab.crash = None;
        tab.history = Default::default();
        self.chrome.show_console = false;
        self.features.site_info = false;
        self.features.find_open = false;
        self.tool_window.take();
        self.load_tab(active);
        self.select_tab(active);
        if target == EngineKind::Wega {
            self.notify(crate::ux::Severity::Info,"Wega is experimental: basic JavaScript and article rendering. Forms, media, site sign-in and page tools still need Servo. Switching engines reloads the page.",None);
        }
        self.changed();
    }
    /// Engine resources of the selected tab, which is always loaded.
    pub(crate) fn live(&self) -> &Live {
        self.tab()
            .live
            .as_ref()
            .expect("the selected tab is always loaded")
    }
    pub(crate) fn view(&self) -> &WebView {
        &self.live().view
    }
    pub(crate) fn tab_index(&self, id: servo::WebViewId) -> Option<usize> {
        self.session.tabs().iter().position(|t| t.id() == Some(id))
    }
    /// Resolve typed or saved input with the configured search engine.
    pub(crate) fn resolve(&self, address: &str) -> Result<url::Url, String> {
        profile::resolve_address_with(address, &self.profile().policy)
    }

    pub(crate) fn add_tab(&mut self, address: &str) {
        match self.resolve(address) {
            Ok(url) => self.add_tab_url(url),
            Err(error) => self.status = Some(error),
        }
    }

    pub(crate) fn new_tab(&mut self) {
        self.add_tab("wega://newtab");
        self.focus_content();
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) fn open_launch_urls(&mut self, urls: Vec<url::Url>) {
        if urls.is_empty() {
            self.window.set_visible(true);
            self.window.focus_window();
            return;
        }
        for url in urls {
            self.add_tab_url(url);
        }
        self.focus_content();
        self.window.set_visible(true);
        self.window.focus_window();
        self.window.request_redraw();
    }

    fn focus_content(&mut self) {
        self.chrome.focus_address = false;
        self.gui.as_ref().unwrap().egui_ctx.memory_mut(|memory| {
            memory.surrender_focus(egui::Id::unique("address"));
        });
        self.chrome.page_focused = true;
        if let Some(view) = self.tab().view() {
            view.set_focused(true);
        }
    }

    fn add_tab_url(&mut self, url: url::Url) {
        let live = self.create_view(Some(&url));
        let index = self.push_tab(Some(live), url.to_string(), String::new(), false);
        self.select_tab(index);
        self.changed();
    }

    fn push_tab(
        &mut self,
        live: Option<Live>,
        url: String,
        title: String,
        opened_by_page: bool,
    ) -> usize {
        self.next_tab_key += 1;
        self.session.push(Tab {
            live,
            wega: None,
            engine: EngineKind::Servo,
            key: self.next_tab_key,
            initial_url: url,
            saved_title: title,
            crash: None,
            history: Default::default(),
            last_selected: Instant::now(),
            opened_by_page,
        });
        self.session.tabs().len() - 1
    }

    /// Offscreen surface sized like the page area, so selecting the tab needs no resize.
    fn tab_surface(&self) -> Rc<servo::OffscreenRenderingContext> {
        let scale = self.window.scale_factor() as f32;
        let rect = self.chrome.page_rect;
        let size = if rect.width() >= 1.0 && rect.height() >= 1.0 {
            PhysicalSize::new(
                (rect.width() * scale).round() as u32,
                (rect.height() * scale).round() as u32,
            )
        } else {
            PhysicalSize::new(1000, 700)
        };
        Rc::new(self.context.offscreen_context(size))
    }

    fn create_view(&mut self, url: Option<&url::Url>) -> Live {
        let surface = self.tab_surface();
        let manager = self
            .content_manager
            .get_or_init(|| crate::compat::content_manager(&self.servo))
            .clone();
        if self.features.script.is_none() && !self.profile().policy.user_script.trim().is_empty() {
            let script = Rc::new(servo::UserScript::new(
                self.profile().policy.user_script.clone(),
                None,
            ));
            manager.add_script(script.clone());
            self.features.script = Some(script);
        }
        let mut builder = WebViewBuilder::new(&self.servo, surface.clone())
            .user_content_manager(manager)
            .hidpi_scale_factor(euclid::Scale::new(self.window.scale_factor() as f32))
            .delegate(self.delegate.clone())
            .clipboard_delegate(Rc::new(engine::Clipboard));
        if let Some(url) = url {
            builder = builder.url(url.clone());
        }
        let view = builder.build();
        view.notify_theme_change(
            if self
                .profile()
                .theme
                .dark(self.window.theme() != Some(winit::window::Theme::Light))
            {
                servo::Theme::Dark
            } else {
                servo::Theme::Light
            },
        );
        Live { view, surface }
    }

    /// Give an unloaded tab a fresh engine view at its saved address.
    fn load_tab(&mut self, index: usize) {
        let Some(tab) = self.session.tabs().get(index) else {
            return;
        };
        if tab.loaded() {
            return;
        }
        let url = profile::resolve_address(&tab.initial_url)
            .unwrap_or_else(|_| url::Url::parse("about:blank").expect("static URL"));
        if tab.engine == EngineKind::Wega {
            match crate::wega_page::Page::new(
                &url,
                &self.profile().policy,
                self.event_proxy.clone(),
                &self.profile_dir,
            ) {
                Ok(page) => {
                    self.session.tabs_mut()[index].wega = Some(page);
                    return;
                }
                Err(error) => {
                    self.status = Some(format!("{error} Opening this tab in Servo."));
                    self.session.tabs_mut()[index].engine = EngineKind::Servo;
                }
            }
        }
        let live = self.create_view(Some(&url));
        live.view.hide();
        let tab = &mut self.session.tabs_mut()[index];
        tab.live = Some(live);
        tab.history = Default::default();
    }

    /// Release state that belongs to a view that is closing or being unloaded.
    fn forget_view(&mut self, id: servo::WebViewId) {
        self.features.media.remove(&id);
        if self.features.media_owner == Some(id) {
            self.features.media_owner = None;
            self.native_media.clear();
        }
        self.features.traversals.remove(&id);
        self.tools.closed(id);
        self.dialogs.forget(id);
        self.delegate.openers.borrow_mut().remove(&id);
        if self.accessibility.grafted == Some(id) {
            self.accessibility.grafted = None;
        }
        self.accessibility.pending.retain(|(owner, _)| *owner != id);
        self.console.retain(|(owner, _)| *owner != id);
    }

    /// Whether unloading this tab could lose something the user would notice.
    fn tab_busy(&self, tab: &Tab) -> bool {
        let Some(id) = tab.id() else {
            return false;
        };
        let owner = crate::network::owner(id);
        tab.opened_by_page
            || tab.loading()
            || tab.crash.is_some()
            || self.features.media_owner == Some(id)
            || self.features.media.get(&id).is_some_and(|m| m.playing)
            || self.features.traversals.contains_key(&id)
            || self.dialogs.has_controls_for(id)
            // Every page has a debugger record; only paused or instrumented ones matter.
            || self
                .tools
                .debug
                .get(&owner)
                .is_some_and(|d| d.paused || !d.breakpoints.is_empty())
            || self
                .automation_navigation
                .as_ref()
                .is_some_and(|p| p.owner == id)
    }

    /// Unload a background tab, keeping its address, title and favicon.
    /// Returns false for the selected tab and for tabs that are already unloaded.
    pub(crate) fn discard_tab(&mut self, index: usize) -> bool {
        if index == self.session.active() {
            return false;
        }
        let Some(tab) = self.session.tabs().get(index) else {
            return false;
        };
        if self.confirm_page_action(crate::page_safety::Action::Discard(tab.key)) {
            return false;
        }
        self.discard_tab_confirmed(index)
    }
    pub(crate) fn discard_tab_confirmed(&mut self, index: usize) -> bool {
        if index == self.session.active()
            || !self.session.tabs().get(index).is_some_and(Tab::loaded)
        {
            return false;
        }
        let tab = &self.session.tabs()[index];
        let (url, title, id) = (tab.url(), tab.title(), tab.id());
        if let Some(id) = id {
            self.forget_view(id);
        }
        let tab = &mut self.session.tabs_mut()[index];
        tab.initial_url = url;
        tab.saved_title = title;
        tab.crash = None;
        // Dropping the last handle closes the engine view and frees its surface.
        tab.live = None;
        tab.wega = None;
        self.changed();
        true
    }

    fn discard_timeout(&self) -> Option<Duration> {
        // Native test scenarios assert on live tabs; never unload behind their back.
        if self.smoke_testing() {
            return None;
        }
        wega::lifecycle::timeout(self.profile().policy.discard_after_minutes)
    }

    /// Unload background tabs that have been idle longer than the configured timeout.
    fn discard_idle_tabs(&mut self) {
        let timeout = self.discard_timeout();
        if self
            .next_discard_check(timeout)
            .is_none_or(|due| due > Instant::now())
        {
            return;
        }
        self.discard_checked = Instant::now();
        // Dialogs are not attributed to tabs reliably; wait until they are answered.
        let blocked = !self.dialogs.is_empty() || self.chrome.show_console;
        let candidates: Vec<usize> = self
            .session
            .tabs()
            .iter()
            .enumerate()
            .filter(|(index, tab)| {
                wega::lifecycle::may_discard(
                    wega::lifecycle::Activity {
                        active: *index == self.session.active(),
                        loaded: tab.loaded(),
                        busy: blocked || self.tab_busy(tab),
                        idle: tab.last_selected.elapsed(),
                    },
                    timeout,
                ) && !wega::lifecycle::needs_confirmation(tab.loaded(), &tab.url())
            })
            .map(|(index, _)| index)
            .collect();
        for index in candidates {
            self.discard_tab(index);
        }
    }

    fn next_discard_check(&self, timeout: Option<Duration>) -> Option<Instant> {
        let active = self.session.active();
        wega::lifecycle::next_check(
            self.session
                .tabs()
                .iter()
                .enumerate()
                .filter(|(index, tab)| {
                    *index != active
                        && tab.loaded()
                        && !wega::lifecycle::needs_confirmation(tab.loaded(), &tab.url())
                })
                .map(|(_, tab)| tab.last_selected),
            timeout,
            self.discard_checked,
        )
    }

    pub(crate) fn select_tab(&mut self, index: usize) {
        if index >= self.session.tabs().len() {
            return;
        }
        self.release_pointer();
        self.load_tab(index);
        let now = Instant::now();
        let previous = self.session.active();
        if let Some(tab) = self.session.tabs_mut().get_mut(previous) {
            tab.last_selected = now;
        }
        for (i, tab) in self.session.tabs().iter().enumerate() {
            if let Some(view) = tab.view() {
                if i == index {
                    view.show();
                } else {
                    view.hide();
                }
                view.set_focused(false);
            }
        }
        self.session.apply(Command::Select(index));
        self.session.tabs_mut()[index].last_selected = now;
        self.delegate.active_view.set(self.tab().id());
        for (i, tab) in self.session.tabs_mut().iter_mut().enumerate() {
            if let Some(page) = &mut tab.wega {
                page.freeze(i != index);
            }
        }
        self.chrome.page_focused = false;
        self.chrome.address = self.tab().url();
        self.chrome.last_url = self.chrome.address.clone();
        self.sync_window_title();
        self.changed();
    }

    pub(crate) fn close_tab(&mut self, index: usize) {
        let Some(tab) = self.session.tabs().get(index) else {
            return;
        };
        if self.confirm_page_action(crate::page_safety::Action::Close(tab.key)) {
            return;
        }
        self.close_tab_confirmed(index);
    }
    pub(crate) fn close_tab_confirmed(&mut self, index: usize) {
        if index >= self.session.tabs().len() {
            return;
        }
        let was_active = index == self.session.active();
        if was_active {
            self.release_pointer();
        }
        self.chrome
            .closed_tabs
            .push(self.session.tabs()[index].url());
        if self.chrome.closed_tabs.len() > 30 {
            self.chrome.closed_tabs.remove(0);
        }
        if let Some(id) = self.session.tabs()[index].id() {
            self.forget_view(id);
        }
        self.session.apply(Command::Close(index));
        if self.session.tabs().is_empty() {
            self.add_tab("about:blank");
            self.chrome.focus_address = true;
        } else {
            let index = self.session.active();
            self.select_tab(index);
        }
        self.changed();
    }

    /// Re-encode favicons from profiles that stored raw pixels; the next save
    /// writes the compact PNG form.
    fn migrate_favicons(&mut self) {
        let legacy: Vec<(String, profile::SavedFavicon)> = self
            .profile()
            .favicons
            .iter()
            .filter(|(_, icon)| matches!(icon, profile::SavedFavicon::Rgba { .. }))
            .map(|(url, icon)| (url.clone(), icon.clone()))
            .collect();
        for (url, icon) in legacy {
            if let profile::SavedFavicon::Rgba {
                width,
                height,
                rgba,
            } = icon
                && let Some(png) = encode_favicon(width, height, rgba)
            {
                self.apply(AppCommand::Favicon {
                    url,
                    image: profile::SavedFavicon::Png(png),
                });
            }
        }
    }

    pub(crate) fn media_action(&self, action: Option<servo::MediaSessionActionType>) {
        let Some(owner) = self.features.media_owner else {
            return;
        };
        if let Some(view) = self
            .session
            .tabs()
            .iter()
            .filter_map(Tab::view)
            .find(|view| view.id() == owner)
        {
            let playing = self.features.media.get(&owner).is_some_and(|m| m.playing);
            view.notify_media_session_action_event(action.unwrap_or(if playing {
                servo::MediaSessionActionType::Pause
            } else {
                servo::MediaSessionActionType::Play
            }));
        }
    }
    pub(crate) fn dispatch_navigation(
        &mut self,
        command: wega::navigation::Command,
    ) -> Result<bool, String> {
        if self.is_wega()
            && matches!(
                command,
                wega::navigation::Command::Load(_)
                    | wega::navigation::Command::Reload
                    | wega::navigation::Command::Traverse { .. }
            )
            && self.confirm_page_action(crate::page_safety::Action::Navigate(
                self.tab().key,
                command.clone(),
            ))
        {
            return Ok(false);
        }
        self.dispatch_navigation_confirmed(command)
    }
    pub(crate) fn dispatch_navigation_confirmed(
        &mut self,
        command: wega::navigation::Command,
    ) -> Result<bool, String> {
        if self.is_wega()
            && let wega::navigation::Command::Load(address) = &command
            && profile::resolve_address(address)?.scheme() == "wega"
        {
            self.switch_engine_confirmed(EngineKind::Servo);
        }
        let active = self.session.active();
        if let Some(page) = &mut self.session.tabs_mut()[active].wega {
            let result = match command {
                wega::navigation::Command::Load(address) => page
                    .load_url(profile::resolve_address(&address)?)
                    .map(|_| true),
                command => wega::navigation::dispatch(command, page),
            };
            self.window.request_redraw();
            return result;
        }
        let view = self.view().clone();
        let result = wega::navigation::dispatch(
            command,
            &mut crate::navigation_adapter::Navigation {
                view: &view,
                traversals: &mut self.features.traversals,
            },
        );
        self.window.request_redraw();
        result
    }
    pub(crate) fn reload(&mut self) {
        let _ = self.dispatch_navigation(wega::navigation::Command::Reload);
    }
    pub(crate) fn reset_zoom(&mut self) {
        let _ = self.dispatch_navigation(wega::navigation::Command::ResetZoom);
    }
    pub(crate) fn traverse(&mut self, forward: bool) {
        let _ = self.dispatch_navigation(wega::navigation::Command::Traverse { forward });
    }
    pub(crate) fn navigate(&mut self, address: &str) {
        if address == "wega://settings" {
            self.features.settings = true;
        }
        match self.resolve(address) {
            Ok(url) => {
                match self.dispatch_navigation(wega::navigation::Command::Load(url.to_string())) {
                    Ok(true) => {}
                    Ok(false) => return,
                    Err(error) => {
                        self.status = Some(error);
                        return;
                    }
                }
                let active = self.session.active();
                self.session.tabs_mut()[active].crash = None;
                self.chrome.address = url.to_string();
                self.chrome.last_url = self.chrome.address.clone();
                self.chrome.page_focused = true;
                if let Some(view) = self.tab().view() {
                    view.set_focused(true);
                }
                self.status = None;
                self.changed();
            }
            Err(error) => self.status = Some(error),
        }
    }

    pub(crate) fn toggle_layout(&mut self) {
        self.apply(AppCommand::Layout(match self.profile().layout {
            TabLayout::Side => TabLayout::Top,
            TabLayout::Top => TabLayout::Side,
        }));
        self.changed();
    }

    pub(crate) fn set_theme(&mut self, ctx: &egui::Context, theme: crate::theme::Theme) {
        self.apply(AppCommand::Theme(theme));
        if theme == crate::theme::Theme::Glass
            && let Some(native) = &self.native_theme
        {
            native.apply(theme, false);
        }
        let dark = theme.dark(self.window.theme() != Some(winit::window::Theme::Light));
        if let Some(native) = &self.native_theme {
            native.apply(theme, dark);
        }
        crate::theme::install(
            ctx,
            theme.palette(
                dark,
                self.native_theme
                    .as_ref()
                    .is_some_and(|native| native.available()),
            ),
        );
        let p = theme.palette(dark, false);
        let css = serde_json::to_string(&crate::capabilities::theme_stylesheet(p)).unwrap();
        let script = format!(
            "document.getElementById('wega-theme')?.replaceChildren(document.createTextNode({css}))"
        );
        for view in self.session.tabs().iter().filter_map(Tab::view) {
            view.notify_theme_change(if dark {
                servo::Theme::Dark
            } else {
                servo::Theme::Light
            });
            if view.url().is_some_and(|u| u.scheme() == "wega") {
                view.evaluate_javascript(script.clone(), |_| {});
            }
        }
        self.changed();
    }
    pub(crate) fn bookmark(&mut self) {
        let url = self.tab().url();
        self.apply(AppCommand::ToggleBookmark {
            title: self.tab().title(),
            url,
        });
        self.changed();
    }

    pub(crate) fn changed(&mut self) {
        self.update_documents();
        if self.pending_save.is_none() {
            self.pending_save = Some(Instant::now());
        }
        self.window.request_redraw();
    }

    pub(crate) fn save(&mut self) {
        let tabs = self
            .session
            .tabs()
            .iter()
            .map(|tab| (tab.url(), tab.title(), tab.engine))
            .collect();
        if let Err(error) = self
            .application
            .save_engine_tabs(tabs, self.session.active())
        {
            self.status = Some(format!("Could not save browser profile: {error}"));
            eprintln!("{}", self.status.as_deref().unwrap());
        }
        self.pending_save = None;
    }

    pub(crate) fn pump(&mut self) {
        self.pump_wega();
        let jobs: Vec<_> = self
            .automation
            .as_ref()
            .map(|rx| rx.try_iter().take(32).collect())
            .unwrap_or_default();
        for job in jobs {
            job(self);
        }
        for message in crate::native_notifications::updates() {
            self.status = Some(message);
        }
        if self.servo.initialized() {
            self.servo.spin_event_loop();
        }
        self.tools.pump();
        if !self.chrome.show_console && self.tools.network.disable_cache {
            self.servo
                .set_preference("network_http_cache_disabled", servo::PrefValue::Bool(false));
            self.tools.network.disable_cache = false;
        }
        if !self.chrome.show_console && self.tools.attention.is_none() {
            self.tools.resume_all();
            if !self.accessibility.active {
                for view in self.session.tabs().iter().filter_map(Tab::view) {
                    view.set_accessibility_active(false);
                }
            }
        }
        if let Some(owner) = self.tools.attention.take() {
            if let Some(index) = self.session.tabs().iter().position(|tab| {
                tab.id()
                    .is_some_and(|id| crate::network::owner(id) == owner)
            }) {
                self.select_tab(index);
            }
            self.chrome.show_console = true;
            self.tools.active = crate::toolbox::Tool::Debugger;
        }
        let events: Vec<_> = self.delegate.events.borrow_mut().drain(..).collect();
        for event in events {
            match event {
                Event::Download(url, name) => self.offer_download(&url, &name),
                Event::Load(owner, status) => {
                    self.automation_load_status(owner, status);
                    if status == servo::LoadStatus::Complete
                        && self.tab().id() == Some(owner)
                        && self.tab().url() == "wega://newtab"
                        && self.chrome.page_focused
                        && !self.modal()
                    {
                        self.view().evaluate_javascript(
                            "document.getElementById('search')?.focus()",
                            |_| {},
                        );
                    }
                }
                Event::History(owner, urls, active) => {
                    if let Some(index) = self.tab_index(owner) {
                        self.session.tabs_mut()[index].history.update(urls, active);
                    }
                }
                Event::Traversal(owner, id) => {
                    self.automation_traversal(owner, &id);
                    if self.features.traversals.get(&owner) == Some(&id) {
                        self.features.traversals.remove(&owner);
                    }
                    self.window.request_redraw();
                }
                Event::Bluetooth(request) => self.features.bluetooth.push_back(request),
                Event::Media(id, event) => {
                    let info = self.features.media.entry(id).or_default();
                    info.update(event);
                    if info.playing {
                        self.features.media_owner = Some(id);
                    }
                    if self.features.media_owner == Some(id) {
                        self.native_media.update(info);
                    }
                    self.window.request_redraw();
                }
                Event::Favicon(url, image) => {
                    let width = image.width as usize;
                    let height = image.height as usize;
                    let pixels = match image.format {
                        servo::PixelFormat::RGBA8 => image.data().to_vec(),
                        servo::PixelFormat::BGRA8 => image
                            .data()
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .flat_map(|p| [p[2], p[1], p[0], p[3]])
                            .collect(),
                        servo::PixelFormat::RGB8 => image
                            .data()
                            .as_chunks::<3>()
                            .0
                            .iter()
                            .flat_map(|p| [p[0], p[1], p[2], 255])
                            .collect(),
                        _ => Vec::new(),
                    };
                    if width > 0
                        && height > 0
                        && pixels.len() == width * height * 4
                        && let Some(gui) = &self.gui
                    {
                        if self.favicons.len() >= 1000 {
                            self.favicons.clear();
                        }
                        let image =
                            egui::ColorImage::from_rgba_unmultiplied([width, height], &pixels);
                        self.favicons.insert(
                            url.clone(),
                            gui.egui_ctx.load_texture(
                                "favicon",
                                image,
                                egui::TextureOptions::LINEAR,
                            ),
                        );
                    }
                    if width > 0 && height > 0 && pixels.len() == width * height * 4 {
                        let w = width.min(32);
                        let h = height.min(32);
                        let mut rgba = Vec::with_capacity(w * h * 4);
                        for y in 0..h {
                            for x in 0..w {
                                let at = ((y * height / h) * width + x * width / w) * 4;
                                rgba.extend_from_slice(&pixels[at..at + 4]);
                            }
                        }
                        // Unchanged icons do not mark the profile dirty.
                        if let Some(png) = encode_favicon(w, h, rgba) {
                            self.apply(AppCommand::Favicon {
                                url,
                                image: profile::SavedFavicon::Png(png),
                            });
                        }
                    }
                    self.window.request_redraw();
                }
                Event::Prompt(prompt) => {
                    if !self.dialogs.push_prompt(prompt) {
                        self.status =
                            Some("Too many pending site requests; the latest was declined.".into());
                    }
                }
                Event::Settings => self.features.settings = true,
                Event::Notice(notice) => {
                    self.status = None;
                    self.chrome.notice = Some(notice);
                }
                Event::NativeAccessibility(id, update) => {
                    #[cfg(feature = "test-harness")]
                    if self.harness.toolbox_test.is_some() {
                        crate::accessibility::tree(update.clone()).expect(
                            "Real Servo accessibility tree must convert without losing semantics",
                        );
                    }

                    if self.accessibility.active && self.tab().id() == Some(id) {
                        self.accessibility.pending.push((id, update));
                    }
                }
                Event::Status(message) => {
                    eprintln!("{message}");
                    self.status = Some(message);
                }
                Event::Changed => self.changed(),
                Event::Closed(id) => {
                    if let Some(index) = self.tab_index(id) {
                        self.close_tab(index);
                    }
                }
                Event::Crashed(id, reason) => {
                    if self.features.media_owner == Some(id) {
                        self.features.media_owner = None;
                        self.native_media.clear();
                    }
                    if let Some(index) = self.tab_index(id) {
                        self.session.tabs_mut()[index].crash = Some(reason);
                    }
                }
                Event::Popup(request, opener) => {
                    let surface = self.tab_surface();
                    let view = request
                        .builder(surface.clone())
                        .user_content_manager(
                            self.content_manager
                                .get()
                                .expect("Servo popup has an initialized manager")
                                .clone(),
                        )
                        .delegate(self.delegate.clone())
                        .clipboard_delegate(Rc::new(engine::Clipboard))
                        .hidpi_scale_factor(euclid::Scale::new(self.window.scale_factor() as f32))
                        .build();
                    if let Some(opener) = opener {
                        self.delegate.openers.borrow_mut().insert(view.id(), opener);
                    }
                    let index = self.push_tab(
                        Some(Live { view, surface }),
                        "about:blank".into(),
                        String::new(),
                        true,
                    );
                    self.select_tab(index);
                }
                Event::Permission(origin, request) => {
                    let feature = format!("{:?}", request.feature());
                    if self.profile().policy.permission_blocked(&origin, &feature) {
                        request.deny();
                    } else {
                        self.dialogs.permissions.push(
                            origin,
                            feature,
                            crate::permission_adapter::Permission(request),
                        );
                    }
                }
                Event::Authentication(request) => {
                    self.dialogs.push_authentication(request);
                }
                Event::Control(id, servo::EmbedderControl::InputMethod(control)) => {
                    let position = control.position();
                    let scale = self.window.scale_factor();
                    self.window.set_ime_cursor_area(
                        PhysicalPosition::new(
                            position.min.x as f64 + self.chrome.page_rect.min.x as f64 * scale,
                            position.min.y as f64 + self.chrome.page_rect.min.y as f64 * scale,
                        ),
                        PhysicalSize::new(
                            position.size().width.max(1) as u32,
                            position.size().height.max(1) as u32,
                        ),
                    );
                    let _ = id;
                }
                Event::Control(id, control) => {
                    self.dialogs.push_control(id, control);
                    self.window.request_redraw();
                }
                Event::HideControl(id) => self
                    .dialogs
                    .controls
                    .retain(|(_, control)| control.id() != id),
                Event::Console(id, text) => {
                    let owner = crate::network::owner(id);
                    if !self.tools.debug.contains_key(&owner) {
                        let (level, message) = text.split_once(": ").unwrap_or(("Log", &text));
                        self.tools.log.text(
                            &owner,
                            if matches!(level, "Trace" | "Dir") {
                                "Log"
                            } else {
                                level
                            },
                            "JS",
                            message.into(),
                            serde_json::Value::Null,
                        );
                    }
                    self.console.push_back((id, text));
                    while self.console.len() > 500 {
                        self.console.pop_front();
                    }
                }
                Event::Visited(url, title) => {
                    self.apply(AppCommand::Visit(Bookmark {
                        title,
                        url,
                        visited_at: profile::visit_time(),
                        visit_count: 0,
                    }));
                }
                Event::Accessibility(id, nodes) => {
                    if let Some(view) = self
                        .tab_index(id)
                        .and_then(|index| self.session.tabs()[index].view().cloned())
                    {
                        let page = self.tools.page(&view);
                        for (id, node) in nodes {
                            if page.ax.len() < 2000 || page.ax.contains_key(&id) {
                                page.ax.insert(id, node);
                            }
                        }
                    }
                }
                Event::Fullscreen(enabled) => {
                    self.fullscreen = enabled;
                    self.window
                        .set_fullscreen(enabled.then_some(Fullscreen::Borderless(None)));
                }
            }
        }
        let url = self.tab().url();
        if url != self.chrome.last_url {
            self.features.site_info = false;
            self.features.cookies.clear();
            self.chrome.last_url = url.clone();
            // Do not replace an address while the user is typing.
            let editing = self.gui.as_ref().is_some_and(|g| {
                g.egui_ctx
                    .memory(|m| m.has_focus(egui::Id::unique("address")))
            });
            if !editing {
                self.chrome.address = url;
            }
        }
        self.sync_window_title();
        self.automation_poll();
    }

    fn sync_window_title(&mut self) {
        // Setting an unchanged X11 title emits property events. Doing it on
        // every pump feeds those events back into another title update.
        let title = format!("{} — {}", self.tab().title(), crate::brand::APP_NAME);
        if title != self.last_window_title {
            self.window.set_title(&title);
            self.last_window_title = title;
        }
    }

    pub(crate) fn animating(&self) -> bool {
        // Hidden views are not painted, so only the selected page drives redraws.
        self.tab().view().is_some_and(WebView::animating)
    }
    pub(crate) fn smoke_testing(&self) -> bool {
        #[cfg(feature = "test-harness")]
        if self.harness.running() {
            return true;
        }
        self.capture.is_some()
    }

    pub(crate) fn check_background_tasks(&mut self, event_loop: &ActiveEventLoop) {
        self.check_capture(event_loop);
        #[cfg(feature = "test-harness")]
        self.check_smoke_test(event_loop);
    }

    /// The CPU/RAM readout in the status bar is only drawn when the window is visible.
    fn resources_visible(&self) -> bool {
        !self.fullscreen && !self.occluded && self.focused
    }

    /// Work that runs after every batch of events. Nothing here assumes a fixed
    /// tick rate: `next_wakeup` schedules the next call for every pending deadline.
    pub(crate) fn tick(&mut self) {
        self.pump();
        if self.resources_visible() && self.resources.refresh() {
            self.window.request_redraw();
        }
        let repaint_due = {
            let mut deadline = self
                .repaint_deadline
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if deadline.is_some_and(|when| when <= Instant::now()) {
                *deadline = None;
                true
            } else {
                false
            }
        };
        if repaint_due {
            self.window.request_redraw();
        }
        if self.animating() || self.smoke_testing() || self.tools.pending() {
            self.window.request_redraw();
        }
        if self
            .pending_save
            .is_some_and(|t| t.elapsed() > Duration::from_secs(1))
        {
            self.save();
        }
        self.discard_idle_tabs();
    }

    /// When the event loop must wake without an external event; `None` sleeps until one
    /// arrives. Servo, automation, devtools and media threads wake the loop themselves.
    pub(crate) fn next_wakeup(&self) -> Option<Instant> {
        let now = Instant::now();
        if self.animating() || self.smoke_testing() || self.tools.pending() {
            return Some(now + Duration::from_millis(16));
        }
        let mut next: Option<Instant> = None;
        let mut at = |when: Instant| {
            next = Some(next.map_or(when, |current| current.min(when)));
        };
        if let Some(when) = *self
            .repaint_deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            at(when);
        }
        if let Some(saved) = self.pending_save {
            at(saved + Duration::from_millis(1001));
        }
        if self.resources_visible() {
            at(self.resources.next_sample());
        }
        if self.tool_window.is_some() {
            // The detached toolbox mirrors live page state without its own events.
            at(now + Duration::from_millis(100));
        } else if self.chrome.show_console {
            at(now + Duration::from_millis(250));
        }
        if let Some(pending) = &self.automation_navigation {
            at(pending.deadline.min(now + Duration::from_millis(250)));
        }
        if let Some(when) = self.next_discard_check(self.discard_timeout()) {
            at(when);
        }
        next.map(|when| when.max(now))
    }

    fn page_point(&self) -> servo::WebViewPoint {
        let scale = self.window.scale_factor() as f32;
        servo::DevicePoint::new(
            self.chrome.pointer.x as f32 - self.chrome.page_rect.min.x * scale,
            self.chrome.pointer.y as f32 - self.chrome.page_rect.min.y * scale,
        )
        .into()
    }

    fn over_page(&self) -> bool {
        let scale = self.window.scale_factor() as f32;
        self.chrome.page_rect.contains(egui::pos2(
            self.chrome.pointer.x as f32 / scale,
            self.chrome.pointer.y as f32 / scale,
        ))
    }

    fn ui_blocks_pointer(&self) -> bool {
        let Some(gui) = &self.gui else {
            return false;
        };
        let scale = self.window.scale_factor() as f32;
        let point = egui::pos2(
            self.chrome.pointer.x as f32 / scale,
            self.chrome.pointer.y as f32 / scale,
        );
        gui.egui_ctx.egui_is_using_pointer()
            || gui
                .egui_ctx
                .layer_id_at(point)
                .is_some_and(|layer| layer.order != egui::Order::Background)
    }

    pub(crate) fn modal(&self) -> bool {
        self.destructive_action.is_some()
            || self.features.settings
            || self.features.clear_data
            || self.features.delete_site.is_some()
            || self.chrome.confirm_clear_history
            || !self.features.bluetooth.is_empty()
            || !self.dialogs.is_empty()
    }

    fn mouse_button(button: MouseButton) -> servo::MouseButton {
        match button {
            MouseButton::Left => servo::MouseButton::Primary,
            MouseButton::Middle => servo::MouseButton::Auxiliary,
            MouseButton::Right => servo::MouseButton::Secondary,
            MouseButton::Back => servo::MouseButton::Back,
            MouseButton::Forward => servo::MouseButton::Forward,
            MouseButton::Other(n) => servo::MouseButton::Other(n),
        }
    }

    fn release_pointer(&mut self) {
        if self.session.tabs().is_empty() {
            self.chrome.pressed_buttons.clear();
            return;
        }
        let buttons = std::mem::take(&mut self.chrome.pressed_buttons);
        let Some(view) = self.tab().view() else {
            return;
        };
        for button in buttons {
            view.notify_input_event(InputEvent::MouseButton(servo::MouseButtonEvent::new(
                servo::MouseButtonAction::Up,
                Self::mouse_button(button),
                self.page_point(),
            )));
        }
    }

    pub(crate) fn shortcut_key(&mut self, key: &Key, key_state: ElementState) -> bool {
        let state = self.chrome.modifiers.state();
        let command = if cfg!(target_os = "macos") {
            state.super_key()
        } else {
            state.control_key()
        };
        let pressed = key_state == ElementState::Pressed;
        let character = match key {
            Key::Character(c) => c.to_lowercase(),
            _ => String::new(),
        };
        if pressed
            && command
            && (state.shift_key() || state.alt_key())
            && matches!(character.as_str(), "i" | "e" | "k")
        {
            if self.is_wega() {
                self.notify(
                    crate::ux::Severity::Info,
                    "Page developer tools require Servo in this version.",
                    None,
                );
                return true;
            }
            self.chrome.show_console = true;
            self.tools.active = match character.as_str() {
                "e" => crate::toolbox::Tool::Network,
                "k" => crate::toolbox::Tool::Console,
                _ => crate::toolbox::Tool::Inspector,
            };
            let view = self.view().clone();
            self.tools.refresh(&view, &self.servo);
            self.window.request_redraw();
            return true;
        }
        if pressed && command {
            match character.as_str() {
                "l" if state.shift_key() => self.toggle_layout(),
                "l" => {
                    self.chrome.focus_address = true;
                    self.chrome.page_focused = false;
                    if let Some(view) = self.tab().view() {
                        view.set_focused(false);
                    }
                }
                "t" if state.shift_key() => {
                    if let Some(url) = self.chrome.closed_tabs.pop() {
                        self.add_tab(&url);
                    }
                }
                "t" => {
                    self.new_tab();
                }
                "w" => self.close_tab(self.session.active()),
                "q" => self.request_quit(),
                "[" => {
                    self.traverse(false);
                }
                "]" => {
                    self.traverse(true);
                }
                "f" => self.features.find_open = true,
                "r" if state.shift_key() => self.reload_without_cache(),
                "r" => self.reload(),
                "d" => self.bookmark(),
                "h" => {
                    self.chrome.show_history = !self.chrome.show_history;
                    self.chrome.show_bookmarks = false;
                    self.chrome.library_query.clear();
                }
                "b" if state.shift_key() => {
                    self.chrome.show_bookmarks = !self.chrome.show_bookmarks;
                    self.chrome.show_history = false;
                    self.chrome.library_query.clear();
                }
                "+" | "=" => self.zoom(1.1),
                "-" => self.zoom(1.0 / 1.1),
                "0" => self.reset_zoom(),
                n if n.len() == 1 && n.as_bytes()[0].is_ascii_digit() && n != "0" => {
                    let n = character.parse::<usize>().unwrap();
                    self.select_tab(if n == 9 {
                        self.session.tabs().len() - 1
                    } else {
                        (n - 1).min(self.session.tabs().len() - 1)
                    });
                }
                _ => return false,
            }
            self.window.request_redraw();
            return true;
        }
        if pressed {
            if self.debugger_shortcut(key, state.shift_key()) {
                return true;
            }
            match key {
                Key::Named(NamedKey::F12) => self.toggle_developer_tools(),
                Key::Named(NamedKey::F6) => {
                    if self.chrome.page_focused {
                        self.chrome.focus_address = true;
                        self.chrome.page_focused = false;
                        if let Some(view) = self.tab().view() {
                            view.set_focused(false);
                        }
                    } else {
                        if let Some(gui) = &self.gui {
                            gui.egui_ctx.memory_mut(|m| {
                                if let Some(id) = m.focused() {
                                    m.surrender_focus(id);
                                }
                            });
                        }
                        self.chrome.page_focused = true;
                        if let Some(view) = self.tab().view() {
                            view.set_focused(true);
                        }
                    }
                }
                Key::Named(NamedKey::Escape)
                    if self.chrome.show_history || self.chrome.show_bookmarks =>
                {
                    self.chrome.show_history = false;
                    self.chrome.show_bookmarks = false;
                }
                Key::Named(NamedKey::Escape) if self.cancel_element_picker() => {}
                Key::Named(NamedKey::Escape)
                    if self.chrome.show_console
                        && !self.chrome.page_focused
                        && !self.fullscreen =>
                {
                    self.tools.split_console = !self.tools.split_console;
                }
                Key::Named(NamedKey::F5) => self.reload(),
                Key::Named(NamedKey::Tab) if state.control_key() => {
                    let next = if state.shift_key() {
                        (self.session.active() + self.session.tabs().len() - 1)
                            % self.session.tabs().len()
                    } else {
                        (self.session.active() + 1) % self.session.tabs().len()
                    };
                    self.select_tab(next);
                }
                Key::Named(NamedKey::ArrowLeft) if state.alt_key() => {
                    self.traverse(false);
                }
                Key::Named(NamedKey::ArrowRight) if state.alt_key() => {
                    self.traverse(true);
                }
                Key::Named(NamedKey::Escape) if self.fullscreen => {
                    if let Some(view) = self.tab().view() {
                        view.exit_fullscreen();
                    }
                    self.window.set_fullscreen(None);
                    self.fullscreen = false;
                }
                _ => return false,
            }
            self.window.request_redraw();
            return true;
        }
        false
    }

    pub(crate) fn zoom(&mut self, factor: f32) {
        if let Err(error) = self.dispatch_navigation(wega::navigation::Command::Zoom { factor }) {
            self.status = Some(error);
        }
    }

    pub(crate) fn handle_event(&mut self, event: WindowEvent) {
        if let WindowEvent::ModifiersChanged(mods) = &event {
            self.chrome.modifiers = *mods;
        }
        if let WindowEvent::KeyboardInput { event, .. } = &event {
            if !self.modal() && self.shortcut_key(&event.logical_key, event.state) {
                self.pump();
                return;
            }
            if self.chrome.page_focused && !self.modal() {
                if self.is_wega() {
                    if let Some(page) = self.tab().wega.as_ref()
                        && let Err(error) = page.keyboard(event, self.chrome.modifiers.state())
                    {
                        self.status = Some(error);
                    }
                } else {
                    input::keyboard(self.view(), event, self.chrome.modifiers);
                }
                self.pump();
                return;
            }
        }
        if let WindowEvent::Ime(ime) = &event
            && self.is_wega()
            && self.chrome.page_focused
            && !self.modal()
        {
            if let Some(page) = self.tab().wega.as_ref()
                && let Err(error) = page.ime(ime)
            {
                self.status = Some(error);
            }
            self.pump();
            return;
        }
        let response = self
            .gui
            .as_mut()
            .unwrap()
            .on_window_event(&self.window, &event);
        // RedrawRequested already paints below. Scheduling it again would
        // continuously repaint idle windows (egui marks that event repaintable).
        if response.repaint && !matches!(event, WindowEvent::RedrawRequested) {
            self.window.request_redraw();
        }
        if self.is_wega() {
            if let WindowEvent::CursorMoved { position, .. } = &event {
                self.chrome.pointer = *position;
            }
            if let WindowEvent::PinchGesture { delta, .. } = &event {
                self.zoom(1.0 + *delta as f32);
            }
            if matches!(
                event,
                WindowEvent::CursorMoved { .. }
                    | WindowEvent::CursorLeft { .. }
                    | WindowEvent::MouseInput { .. }
                    | WindowEvent::MouseWheel { .. }
                    | WindowEvent::PinchGesture { .. }
                    | WindowEvent::Ime(_)
                    | WindowEvent::KeyboardInput { .. }
            ) {
                self.pump();
                return;
            }
        }
        match event {
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                self.context.resize(size);
                self.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                for view in self.session.tabs().iter().filter_map(Tab::view) {
                    view.set_hidpi_scale_factor(euclid::Scale::new(scale_factor as f32));
                }
                self.window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let was_over = self.over_page();
                self.chrome.pointer = position;
                if (self.over_page() || !self.chrome.pressed_buttons.is_empty())
                    && !self.modal()
                    && !self.ui_blocks_pointer()
                {
                    self.view().notify_input_event(InputEvent::MouseMove(
                        servo::MouseMoveEvent::new(self.page_point()),
                    ));
                    self.window.set_cursor(input::cursor(self.view().cursor()));
                } else if was_over {
                    self.view()
                        .notify_input_event(InputEvent::MouseLeftViewport(
                            servo::MouseLeftViewportEvent::default(),
                        ));
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.view()
                    .notify_input_event(InputEvent::MouseLeftViewport(
                        servo::MouseLeftViewportEvent::default(),
                    ));
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let up = state == ElementState::Released;
                if (!up && self.over_page() && !self.modal() && !self.ui_blocks_pointer())
                    || (up && self.chrome.pressed_buttons.contains(&button))
                {
                    if up {
                        self.chrome.pressed_buttons.retain(|b| *b != button);
                    } else {
                        self.chrome.pressed_buttons.push(button);
                        self.chrome.page_focused = true;
                        self.gui
                            .as_ref()
                            .unwrap()
                            .egui_ctx
                            .memory_mut(|m| m.surrender_focus(egui::Id::unique("address")));
                        if let Some(view) = self.tab().view() {
                            view.set_focused(true);
                        }
                    }
                    self.view().notify_input_event(InputEvent::MouseButton(
                        servo::MouseButtonEvent::new(
                            if up {
                                servo::MouseButtonAction::Up
                            } else {
                                servo::MouseButtonAction::Down
                            },
                            Self::mouse_button(button),
                            self.page_point(),
                        ),
                    ));
                } else if !up {
                    self.chrome.page_focused = false;
                    if let Some(view) = self.tab().view() {
                        view.set_focused(false);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. }
                if self.over_page() && !self.modal() && !self.ui_blocks_pointer() =>
            {
                let (x, y, mode) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        // Servo's compositor consumes these deltas as device pixels.
                        (
                            (x * 76.0) as f64,
                            (y * 76.0) as f64,
                            servo::WheelMode::DeltaLine,
                        )
                    }
                    MouseScrollDelta::PixelDelta(p) => (p.x, p.y, servo::WheelMode::DeltaPixel),
                };
                self.view()
                    .notify_input_event(InputEvent::Wheel(servo::WheelEvent::new(
                        servo::WheelDelta { x, y, z: 0.0, mode },
                        self.page_point(),
                    )));
            }
            WindowEvent::PinchGesture { delta, .. } if self.over_page() && !self.modal() => {
                let scale = self.window.scale_factor() as f32;
                let center = servo::DevicePoint::new(
                    self.chrome.pointer.x as f32 - self.chrome.page_rect.min.x * scale,
                    self.chrome.pointer.y as f32 - self.chrome.page_rect.min.y * scale,
                );
                self.view()
                    .adjust_pinch_zoom((1.0 + delta as f32).max(0.01), center);
                self.window.request_redraw();
            }
            WindowEvent::Ime(event) if self.chrome.page_focused && !self.modal() => {
                let (state, data) = match event {
                    Ime::Preedit(text, _) => {
                        if !self.chrome.ime_composing {
                            self.chrome.ime_composing = true;
                            self.view().notify_input_event(InputEvent::Ime(
                                servo::ImeEvent::Composition(servo::CompositionEvent {
                                    state: servo::CompositionState::Start,
                                    data: String::new(),
                                }),
                            ));
                        }
                        (servo::CompositionState::Update, text)
                    }
                    Ime::Commit(text) => {
                        self.chrome.ime_composing = false;
                        (servo::CompositionState::End, text)
                    }
                    _ => {
                        self.pump();
                        return;
                    }
                };
                self.view()
                    .notify_input_event(InputEvent::Ime(servo::ImeEvent::Composition(
                        servo::CompositionEvent { state, data },
                    )));
            }
            WindowEvent::ThemeChanged(_) => {
                if (cfg!(target_os = "windows")
                    || self.profile().theme == crate::theme::Theme::Glass)
                    && let Some(gui) = &self.gui
                {
                    let ctx = gui.egui_ctx.clone();
                    self.set_theme(&ctx, self.profile().theme);
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if !occluded {
                    self.window.request_redraw();
                }
            }
            WindowEvent::Focused(true) => {
                self.focused = true;
                #[cfg(target_os = "windows")]
                if let Some(gui) = &self.gui {
                    let ctx = gui.egui_ctx.clone();
                    self.set_theme(&ctx, self.profile().theme);
                }
            }
            WindowEvent::Focused(false) => {
                self.focused = false;
                self.release_pointer();
                self.chrome.page_focused = false;
                if let Some(view) = self.tab().view() {
                    view.set_focused(false);
                }
            }
            _ => {}
        }
        self.pump();
    }

    fn redraw(&mut self) {
        if self.window.inner_size().width == 0 || self.window.inner_size().height == 0 {
            return;
        }
        if let Err(error) = self.context.make_current() {
            self.status = Some(format!("GPU error: {error:?}"));
            return;
        }
        self.pump();
        let mut gui = self.gui.take().unwrap();
        let window = self.window.clone();
        gui.run(&window, |ui| self.ui(ui));
        self.flush_accessibility(&mut gui);
        // Servo may bind its framebuffer while painting. Restore the window target before egui.
        if self.tab().crash.is_none() && !self.is_wega() {
            self.view().paint();
        }
        self.context.prepare_for_rendering();
        gui.painter.clear(
            window.inner_size().into(),
            if self.profile().theme.glass()
                && self
                    .native_theme
                    .as_ref()
                    .is_some_and(|native| native.available())
            {
                [0.0, 0.0, 0.0, 0.0]
            } else {
                self.profile()
                    .theme
                    .palette(
                        self.window.theme() != Some(winit::window::Theme::Light),
                        false,
                    )
                    .chrome
                    .to_array()
                    .map(|c| c as f32 / 255.0)
            },
        );
        gui.paint(&window);
        if let Some(capture) = &self.capture
            && capture.ready_for_frame()
        {
            let output = capture.output.with_extension("window.png");
            if !output.exists() {
                let size = window.inner_size();
                let rect = servo::DeviceIntRect::from_size(servo::DeviceIntSize::new(
                    size.width as i32,
                    size.height as i32,
                ));
                if let Some(image) = self.context.read_to_image(rect) {
                    image.save(output).expect("Save browser capture");
                }
            }
        }
        #[cfg(feature = "test-harness")]
        if self.harness.smoke.as_ref().is_some_and(|s| {
            ((8..=10).contains(&s.stage) && s.stage_at.elapsed() > Duration::from_millis(300))
                || (s.stage == 7 && self.profile().layout == TabLayout::Top)
        }) {
            let output = self
                .profile_dir
                .join(if self.profile().layout == TabLayout::Top {
                    "smoke-top-tabs.png"
                } else {
                    match self.harness.smoke.as_ref().unwrap().stage {
                        9 => "smoke-history.png",
                        10 => "smoke-bookmarks.png",
                        _ => "smoke-window.png",
                    }
                });
            if self.profile().layout == TabLayout::Top || !output.exists() {
                let size = window.inner_size();
                let rect = servo::DeviceIntRect::from_size(servo::DeviceIntSize::new(
                    size.width as i32,
                    size.height as i32,
                ));
                if let Some(image) = self.context.read_to_image(rect)
                    && let Err(error) = image.save(output)
                {
                    eprintln!("Cannot save smoke window image: {error}");
                }
            }
        }
        #[cfg(feature = "test-harness")]
        if let Some(test) = &self.harness.toolbox_test
            && let Some(name) = &test.capture
            && !name.starts_with("detached-")
        {
            let size = window.inner_size();
            let rect = servo::DeviceIntRect::from_size(servo::DeviceIntSize::new(
                size.width as i32,
                size.height as i32,
            ));
            if let Some(image) = self.context.read_to_image(rect) {
                image
                    .save(self.profile_dir.join(name))
                    .expect("Toolbox frame export");
            }
        }
        #[cfg(feature = "test-harness")]
        if let Some(test) = &mut self.harness.capabilities_test
            && test.frame_ready()
            && let Some(output) = &test.frame_output
        {
            let size = window.inner_size();
            let rect = servo::DeviceIntRect::from_size(servo::DeviceIntSize::new(
                size.width as i32,
                size.height as i32,
            ));
            if let Some(image) = self.context.read_to_image(rect) {
                image.save(output).expect("Capability UI frame export");
                test.frame_checked = true;
            }
        }
        #[cfg(feature = "test-harness")]
        if let Some(test) = &mut self.harness.theme_test
            && !test.frame_checked
            && let Some(output) = &test.output
        {
            let size = window.inner_size();
            let rect = servo::DeviceIntRect::from_size(servo::DeviceIntSize::new(
                size.width as i32,
                size.height as i32,
            ));
            if let Some(image) = self.context.read_to_image(rect) {
                let scale = window.scale_factor() as f32;
                let chrome = image.get_pixel((5.0 * scale) as u32, (300.0 * scale) as u32)[3];
                let center = self.chrome.page_rect.center();
                let page = image.get_pixel((center.x * scale) as u32, (center.y * scale) as u32)[3];
                let corner = image.get_pixel(
                    (self.chrome.page_rect.min.x * scale) as u32 + 1,
                    (self.chrome.page_rect.min.y * scale) as u32 + 1,
                )[3];
                let glass = self.application.profile().theme.glass()
                    && self
                        .native_theme
                        .as_ref()
                        .is_some_and(|native| native.available());
                if glass {
                    assert!(
                        chrome > 0 && chrome < 255,
                        "Glass chrome has composited alpha: {chrome}"
                    );
                    assert!(
                        corner < 255,
                        "Rounded corners expose the native backdrop: {corner}"
                    );
                } else {
                    assert_eq!(chrome, 255);
                }
                assert_eq!(page, 255, "Web pages remain opaque");
                image.save(output).expect("Save theme frame");
                test.frame_checked = true;
            }
        }
        self.context.present();
        self.gui = Some(gui);
    }

    pub(crate) fn viewport(&mut self, ui: &mut egui::Ui) {
        let outer = ui.available_rect_before_wrap();
        let rect = if self.fullscreen {
            outer
        } else {
            outer.shrink(8.0)
        };
        self.chrome.page_rect = rect;
        if self.is_wega() {
            self.wega_viewport(ui, rect, outer);
            return;
        }
        self.accessibility_viewport(ui, rect);
        if let Some(reason) = self.tab().crash.clone() {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("This page stopped responding");
                    ui.label(reason);
                    if ui.button("Reload page").clicked() {
                        let active = self.session.active();
                        self.session.tabs_mut()[active].crash = None;
                        self.reload();
                    }
                });
            });
            return;
        }
        let scale = ui.ctx().pixels_per_point();
        let size = PhysicalSize::new(
            (rect.width() * scale).round().max(1.0) as u32,
            (rect.height() * scale).round().max(1.0) as u32,
        );
        if self.live().surface.size() != size {
            self.view().resize(size);
        }
        if let Some(blit) = self.live().surface.render_to_parent_callback() {
            ui.painter().add(egui::PaintCallback {
                rect,
                callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                    let viewport = info.viewport_in_pixels();
                    blit(
                        painter.gl(),
                        euclid::default::Rect::new(
                            euclid::point2(viewport.left_px, viewport.from_bottom_px),
                            euclid::size2(viewport.width_px, viewport.height_px),
                        ),
                    );
                })),
            });
        }
        if !self.fullscreen {
            crate::ui::page_corners(ui.painter(), rect);
        }
        ui.allocate_rect(outer, egui::Sense::hover());
    }

    pub(crate) fn shutdown(&mut self) {
        self.tool_window.take();
        let _ = self.context.make_current();
        self.save();
        self.tools.shutdown();
        self.dialogs.clear();
        self.session.clear();
        self.delegate.events.borrow_mut().clear();
        if self.servo.initialized() {
            self.servo.spin_event_loop();
        }
        let _ = self.context.make_current();
        if let Some(gui) = &mut self.gui {
            gui.destroy();
        }
    }
}

/// Encode a small RGBA favicon as PNG for the profile.
fn encode_favicon(width: usize, height: usize, rgba: Vec<u8>) -> Option<Vec<u8>> {
    let image = image::RgbaImage::from_raw(width as u32, height as u32, rgba)?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// Pixels of a saved favicon, decoding the PNG form.
fn decode_favicon(icon: &profile::SavedFavicon) -> Option<(usize, usize, Vec<u8>)> {
    match icon {
        profile::SavedFavicon::Rgba {
            width,
            height,
            rgba,
        } => Some((*width, *height, rgba.clone())),
        profile::SavedFavicon::Png(png) => {
            let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
                .ok()?
                .into_rgba8();
            let (width, height) = image.dimensions();
            Some((width as usize, height as usize, image.into_raw()))
        }
    }
}
