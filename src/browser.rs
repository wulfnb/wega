use crate::window_context::WindowRenderingContext;
use crate::{
    Options,
    engine::Wake,
    profile::{self, Profile, TabLayout},
};
use std::{
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
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoopProxy},
    keyboard::{Key, NamedKey},
    raw_window_handle::{HasDisplayHandle, HasWindowHandle},
    window::Window,
};

/// A tab is either loaded (`live`) or unloaded: restored but not yet selected, or
/// discarded after being idle. Unloaded tabs keep only their address, title and
/// lifecycle data, and get a fresh engine view when they are selected.
pub struct Tab {
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
    pub(crate) fn loaded(&self) -> bool {
        self.wega.is_some() || self.is_internal()
    }
    /// WebDriver handle and drag-and-drop identity; survives unloading.
    pub(crate) fn handle(&self) -> String {
        format!("tab-{}", self.key)
    }
    pub(crate) fn url(&self) -> String {
        self.wega
            .as_ref()
            .map_or_else(|| self.initial_url.clone(), |page| page.url())
    }
    pub(crate) fn title(&self) -> String {
        if let Some(page) = &self.wega {
            let title = page.title();
            if !title.is_empty() {
                return title;
            }
        }
        if self.is_internal() {
            return match self
                .url()
                .strip_prefix("wega://")
                .unwrap_or("newtab")
                .split('?')
                .next()
                .unwrap_or("newtab")
            {
                "newtab" => "New tab",
                "history" => "History",
                "bookmarks" => "Bookmarks",
                "settings" => "Settings",
                "about" => "About Wega",
                _ => "Wega",
            }
            .into();
        }
        if !self.saved_title.is_empty() {
            return self.saved_title.clone();
        }
        url::Url::parse(&self.url())
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| "New tab".into())
    }
    pub(crate) fn loading(&self) -> bool {
        self.wega.as_ref().is_some_and(|page| page.loading())
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
    pub(crate) window: Rc<Window>,
    pub(crate) event_proxy: EventLoopProxy<Wake>,
    pub(crate) native_media: crate::native_media::NativeMedia,
    pub(crate) native_theme: Option<crate::native_theme::NativeTheme>,
    session: Session<Tab>,
    pub(crate) context: Rc<WindowRenderingContext>,
    pub(crate) gui: Option<egui_glow::EguiGlow>,
    application: Application,
    pub(crate) profile_dir: PathBuf,
    pub(crate) containers: crate::containers_ui::State,
    last_window_title: String,
    pub(crate) tool_window: Option<crate::tool_window::ToolWindow>,
    pub(crate) tools: crate::toolbox::Toolbox,
    pub(crate) dialogs: crate::capabilities::Dialogs,
    pub(crate) status: Option<String>,
    pub(crate) accessibility: crate::accessibility::State,
    pub(crate) resources: crate::resources::Monitor,
    pub(crate) downloads: crate::downloads::Manager,
    pub(crate) download_ui: crate::downloads_ui::State,
    pub(crate) fullscreen: bool,
    pub(crate) quit_requested: bool,
    pub(crate) destructive_action: Option<crate::page_safety::Action>,
    pub(crate) page_check: Option<crate::page_safety::Check>,
    pub(crate) page_check_id: u64,
    pub(crate) repaint_deadline: Arc<std::sync::Mutex<Option<Instant>>>,
    pub(crate) pending_save: Option<Instant>,
    pub(crate) capture: Option<crate::capture::Capture>,
    pub(crate) internal_state: crate::internal_pages::InternalPagesState,
    next_tab_key: u64,
    discard_checked: Instant,
    /// The window is minimized or fully covered; skip work that only updates pixels.
    pub(crate) occluded: bool,
    pub(crate) focused: bool,
}

impl Browser {
    /// Close every content owner before main drains reapers and exits the process.
    pub(crate) fn shutdown_wega_workers(&mut self) {
        for tab in self.session.tabs_mut() {
            tab.wega.take();
        }
    }
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
        let containers = crate::containers_ui::State::new(options, &profile_dir)?;
        let mut downloads = crate::downloads::Manager::new(&profile_dir);
        if options.container.is_some() {
            downloads.directory = profile_dir.join("Downloads");
        }
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
        let automation = options
            .webdriver_port
            .map(|port| crate::automation::start(port, proxy.clone()))
            .transpose()?;
        let tools = crate::toolbox::Toolbox::default();
        let native_media = crate::native_media::NativeMedia::new(proxy.clone());
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
            favicons,
            window,
            event_proxy: proxy.clone(),
            native_theme,
            native_media,
            session: Session::default(),
            context,
            gui: Some(gui),
            application,
            profile_dir,
            containers,
            last_window_title: String::new(),
            resources: crate::resources::Monitor::new(),
            downloads,
            download_ui: Default::default(),
            tool_window: None,
            tools,
            dialogs: Default::default(),
            accessibility: Default::default(),
            status: warning,
            fullscreen: false,
            quit_requested: false,
            destructive_action: None,
            page_check: None,
            page_check_id: 0,
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
            internal_state: Default::default(),
        };
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
        } else if options.container.is_some() && !browser.profile_dir.join("browser.json").is_file()
        {
            vec!["wega://newtab".into()]
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
                browser.push_tab(url.to_string(), title, false);
            }
        }
        if browser.session.tabs().is_empty() {
            browser.push_tab(profile::HOME.into(), String::new(), false);
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
                for page in self
                    .session
                    .tabs()
                    .iter()
                    .filter_map(|tab| tab.wega.as_ref())
                {
                    let policy = &self.profile().policy;
                    let _ = page.send(wega::engine_protocol::Command::Policy {
                        offline: policy.offline,
                        blocked_hosts: policy.blocked_hosts.clone(),
                        allowed_hosts: policy.allowed_hosts.clone(),
                        user_script: policy.user_script.clone(),
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
    pub(crate) fn tab_mut(&mut self) -> &mut Tab {
        let active = self.session.active();
        &mut self.session.tabs_mut()[active]
    }
    pub(crate) fn current_url(&self) -> Option<url::Url> {
        url::Url::parse(&self.tab().url()).ok()
    }
    pub(crate) fn page_zoom(&self) -> f32 {
        self.tab().wega.as_ref().map_or(1.0, |page| page.zoom)
    }
    pub(crate) fn can_go_back(&self) -> bool {
        if self.is_internal() {
            self.tab().history.can_back()
        } else {
            self.tab().wega.as_ref().is_some_and(|page| page.can_back())
        }
    }
    pub(crate) fn can_go_forward(&self) -> bool {
        if self.is_internal() {
            self.tab().history.can_forward()
        } else {
            self.tab()
                .wega
                .as_ref()
                .is_some_and(|page| page.can_forward())
        }
    }
    pub(crate) fn history_target(&self, forward: bool) -> Option<String> {
        if self.tab().is_internal() {
            return self.tab().history.target(forward).map(str::to_owned);
        }
        if let Some(page) = &self.tab().wega {
            return page.history_target(forward).map(str::to_owned);
        }
        self.tab().history.target(forward).map(str::to_owned)
    }
    pub(crate) fn wega_mut(&mut self) -> Option<&mut crate::wega_page::Page> {
        let active = self.session.active();
        self.session.tabs_mut()[active].wega.as_mut()
    }
    /// Engine resources of the selected tab, which is always loaded.
    #[cfg_attr(not(feature = "test-harness"), allow(dead_code))]
    #[cfg_attr(not(feature = "test-harness"), allow(dead_code))]
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
        self.chrome.clear_address_focus = true;
        if let Some(gui) = &self.gui {
            gui.egui_ctx.memory_mut(|memory| {
                memory.surrender_focus(egui::Id::unique("address"));
            });
            self.chrome.clear_address_focus = false;
        }
        self.chrome.page_focused = true;
    }

    fn add_tab_url(&mut self, url: url::Url) {
        let index = self.push_tab(url.to_string(), String::new(), false);
        self.session.tabs_mut()[index].history.push(url.to_string());
        self.select_tab(index);
        self.changed();
    }

    fn push_tab(&mut self, url: String, title: String, opened_by_page: bool) -> usize {
        self.next_tab_key = self
            .next_tab_key
            .checked_add(1)
            .expect("Tab identity exhausted");
        self.session.push(Tab {
            wega: None,
            engine: EngineKind::Wega,
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

    /// Give an unloaded tab a fresh engine view at its saved address.
    fn load_tab(&mut self, index: usize) {
        let Some(tab) = self.session.tabs().get(index) else {
            return;
        };
        if tab.loaded() {
            return;
        }
        let url = match profile::resolve_address(&tab.initial_url) {
            Ok(url) => url,
            Err(error) => {
                self.status = Some(error);
                return;
            }
        };
        match crate::wega_page::Page::new(
            &url,
            &self.profile().policy,
            self.event_proxy.clone(),
            &self.profile_dir,
            tab.key,
        ) {
            Ok(page) => {
                self.session.tabs_mut()[index].wega = Some(page);
                self.session.tabs_mut()[index].crash = None;
            }
            Err(error) => {
                self.session.tabs_mut()[index].crash = Some(error.clone());
                self.status = Some(error);
            }
        }
    }

    /// Whether unloading this tab could lose something the user would notice.
    fn tab_busy(&self, tab: &Tab) -> bool {
        tab.opened_by_page
            || tab.loading()
            || tab.crash.is_some()
            || self.features.media_owner == Some(tab.key)
            || self
                .features
                .media
                .get(&tab.key)
                .is_some_and(|media| media.playing)
            || self
                .automation_navigation
                .as_ref()
                .is_some_and(|pending| pending.tab_key == tab.key)
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
        let (url, title) = (tab.url(), tab.title());
        let tab = &mut self.session.tabs_mut()[index];
        tab.initial_url = url;
        tab.saved_title = title;
        tab.crash = None;
        // Dropping the last handle closes the engine view and frees its surface.
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
                ) && !wega::lifecycle::requires_state_check(tab.loaded(), &tab.url())
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
                        && !wega::lifecycle::requires_state_check(tab.loaded(), &tab.url())
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
        self.session.apply(Command::Select(index));
        self.session.tabs_mut()[index].last_selected = now;
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
        let closed_tab = &self.session.tabs()[index];
        if let Some(pending) = &self.automation_navigation
            && pending.tab_key == closed_tab.key
        {
            if let Some(pending) = self.automation_navigation.take() {
                let _ = pending.reply.send(Err(crate::automation::error(
                    webdriver::error::ErrorStatus::NoSuchWindow,
                    "Window was closed",
                )));
            }
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

    pub(crate) fn media_action(
        &self,
        action: Option<wega::engine_protocol::media::MediaSessionAction>,
    ) {
        use wega::engine_protocol::media::MediaSessionAction;
        if let Some(owner) = self.features.media_owner
            && let Some(tab) = self.session.tabs().iter().find(|tab| tab.key == owner)
            && let Some(page) = &tab.wega
        {
            let action = action.unwrap_or_else(|| {
                if self
                    .features
                    .media
                    .get(&owner)
                    .is_some_and(|media| media.playing)
                {
                    MediaSessionAction::Pause
                } else {
                    MediaSessionAction::Play
                }
            });
            let _ = page.send(wega::engine_protocol::Command::MediaSessionAction(action));
        }
    }
    pub(crate) fn dispatch_navigation(
        &mut self,
        command: wega::navigation::Command,
    ) -> Result<bool, String> {
        if matches!(
            command,
            wega::navigation::Command::Load(_)
                | wega::navigation::Command::Reload
                | wega::navigation::Command::ReloadBypassCache
                | wega::navigation::Command::Traverse { .. }
        ) && self.confirm_page_action(crate::page_safety::Action::Navigate(
            self.tab().key,
            command.clone(),
        )) {
            return Ok(false);
        }
        self.dispatch_navigation_confirmed(command)
    }
    pub(crate) fn dispatch_navigation_confirmed(
        &mut self,
        command: wega::navigation::Command,
    ) -> Result<bool, String> {
        use wega::navigation::Command;
        if let Command::Load(address) = &command {
            let url = profile::resolve_address(address)?;
            if url.scheme() == "wega" && url.host_str() == Some("download") {
                let fields: std::collections::HashMap<_, _> =
                    url.query_pairs().into_owned().collect();
                if let Some(url) = fields.get("url") {
                    self.offer_download(url, fields.get("name").map_or("", String::as_str));
                }
                return Ok(true);
            }
            if url.scheme() == "wega" {
                let tab = self.tab_mut();
                tab.saved_title = tab.title();
                tab.wega.take();
                tab.history.push(url.to_string());
                tab.initial_url = url.to_string();
                self.window.request_redraw();
                return Ok(true);
            }
            if self.tab().wega.is_none() {
                let page = crate::wega_page::Page::new(
                    &url,
                    &self.profile().policy,
                    self.event_proxy.clone(),
                    &self.profile_dir,
                    self.tab().key,
                )?;
                let tab = self.tab_mut();
                tab.initial_url = url.to_string();
                tab.history.push(url.to_string());
                tab.wega = Some(page);
                tab.crash = None;
                self.window.request_redraw();
                return Ok(true);
            }
        }
        if let Command::Traverse { forward } = command
            && self.is_internal()
        {
            let destination = self.tab_mut().history.traverse(forward).map(str::to_owned);
            if let Some(destination) = destination {
                return self.dispatch_navigation_confirmed(Command::Load(destination));
            }
            return Ok(false);
        }
        if matches!(command, Command::Reload) && self.is_internal() {
            self.window.request_redraw();
            return Ok(true);
        }
        let active = self.session.active();
        if let Some(page) = &mut self.session.tabs_mut()[active].wega {
            let result = match command {
                Command::Load(address) => page
                    .load_url(profile::resolve_address(&address)?)
                    .map(|_| true),
                command => wega::navigation::dispatch(command, page),
            };
            self.window.request_redraw();
            return result;
        }
        if matches!(command, Command::Reload) {
            self.load_tab(active);
        }
        Ok(false)
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
        let identity = self
            .containers
            .current
            .as_ref()
            .map(|c| format!(" [{}]", c.name))
            .unwrap_or_default();
        let title = format!(
            "{}{} — {}",
            self.tab().title(),
            identity,
            crate::brand::APP_NAME
        );
        if title != self.last_window_title {
            self.window.set_title(&title);
            self.last_window_title = title;
        }
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
        self.expire_page_check();
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
        if self.smoke_testing() {
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
    /// arrives. Content, automation, devtools and media threads wake the loop themselves.
    pub(crate) fn next_wakeup(&self) -> Option<Instant> {
        let now = Instant::now();
        if self.smoke_testing() {
            return Some(now + Duration::from_millis(16));
        }
        let mut next: Option<Instant> = None;
        let mut at = |when: Instant| {
            next = Some(next.map_or(when, |current| current.min(when)));
        };
        if let Some(check) = &self.page_check {
            at(check.deadline);
        }
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

    pub(crate) fn modal(&self) -> bool {
        self.destructive_action.is_some()
            || self.page_check.is_some()
            || self.features.settings
            || self.features.clear_data
            || self.features.delete_site.is_some()
            || self.chrome.confirm_clear_history
            || !self.dialogs.is_empty()
    }

    fn release_pointer(&mut self) {
        self.chrome.pressed_buttons.clear();
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
            self.chrome.show_console = true;
            self.window.request_redraw();
            return true;
        }
        if pressed && command {
            match character.as_str() {
                "l" if state.shift_key() => self.toggle_layout(),
                "l" => {
                    self.chrome.focus_address = true;
                    self.chrome.page_focused = false;
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
                "f" => {
                    self.features.find_open = true;
                    self.chrome.page_focused = false;
                }
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
            match key {
                Key::Named(NamedKey::F12) => self.toggle_developer_tools(),
                Key::Named(NamedKey::F6) => {
                    if self.chrome.page_focused {
                        self.chrome.focus_address = true;
                        self.chrome.page_focused = false;
                    } else {
                        if let Some(gui) = &self.gui {
                            gui.egui_ctx.memory_mut(|m| {
                                if let Some(id) = m.focused() {
                                    m.surrender_focus(id);
                                }
                            });
                        }
                        self.chrome.page_focused = true;
                    }
                }
                Key::Named(NamedKey::Escape)
                    if self.chrome.show_history || self.chrome.show_bookmarks =>
                {
                    self.chrome.show_history = false;
                    self.chrome.show_bookmarks = false;
                }
                Key::Named(NamedKey::Escape)
                    if self.chrome.show_console
                        && !self.chrome.page_focused
                        && !self.fullscreen =>
                {
                    self.close_developer_tools()
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
        if let WindowEvent::ModifiersChanged(modifiers) = &event {
            self.chrome.modifiers = *modifiers;
        }
        if let WindowEvent::KeyboardInput { event, .. } = &event {
            if !self.modal() && self.shortcut_key(&event.logical_key, event.state) {
                self.pump();
                return;
            }
            if self.chrome.page_focused && !self.modal() && !self.is_internal() {
                if let Some(page) = &self.tab().wega
                    && let Err(error) = page.keyboard(event, self.chrome.modifiers.state())
                {
                    self.status = Some(error);
                }
                self.pump();
                return;
            }
        }
        if let WindowEvent::Ime(ime) = &event
            && self.chrome.page_focused
            && !self.modal()
            && !self.is_internal()
        {
            if let Some(page) = &self.tab().wega
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
        if response.repaint && !matches!(event, WindowEvent::RedrawRequested) {
            self.window.request_redraw();
        }
        match event {
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                self.context.resize(size);
                self.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => self.window.request_redraw(),
            WindowEvent::CursorMoved { position, .. } => self.chrome.pointer = position,
            WindowEvent::PinchGesture { delta, .. } => self.zoom(1.0 + delta as f32),
            WindowEvent::Occluded(value) => self.occluded = value,
            WindowEvent::Focused(value) => {
                self.focused = value;
                if !value {
                    self.release_pointer();
                    self.chrome.page_focused = false;
                }
            }
            WindowEvent::ThemeChanged(_) => {
                let context = self.gui.as_ref().map(|gui| gui.egui_ctx.clone());
                if let Some(context) = context {
                    self.set_theme(&context, self.profile().theme);
                }
                self.window.request_redraw();
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
        if std::mem::take(&mut self.chrome.clear_address_focus) {
            gui.egui_ctx
                .memory_mut(|memory| memory.surrender_focus(egui::Id::unique("address")));
            self.chrome.page_focused = true;
        }
        self.context.prepare_for_rendering();
        let color = self
            .profile()
            .theme
            .palette(
                self.window.theme() != Some(winit::window::Theme::Light),
                false,
            )
            .chrome
            .to_array()
            .map(|c| c as f32 / 255.0);
        let color = if self.profile().theme.glass()
            && self
                .native_theme
                .as_ref()
                .is_some_and(|native| native.available())
        {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            color
        };
        gui.painter.clear(window.inner_size().into(), color);
        gui.paint(&window);
        if let Some(capture) = &self.capture
            && capture.ready_for_frame()
        {
            let output = capture.output.with_extension("window.png");
            if !output.exists()
                && let Some(image) = self.context.read_to_image()
            {
                let _ = image.save(output);
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
        self.accessibility_viewport(ui, rect);
        if self.is_internal() {
            self.internal_viewport(ui, rect, outer);
            return;
        }
        if let Some(reason) = self.tab().crash.clone() {
            let mut retry = false;
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Page unavailable");
                    ui.label(reason);
                    retry = ui.button("Retry").clicked();
                });
            });
            if retry {
                let active = self.session.active();
                self.load_tab(active);
            }
            return;
        }
        self.wega_viewport(ui, rect, outer);
    }

    pub(crate) fn shutdown(&mut self) {
        self.tool_window.take();
        let _ = self.context.make_current();
        self.save();
        self.dialogs.clear();
        self.session.clear();
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
pub(crate) fn decode_favicon(icon: &profile::SavedFavicon) -> Option<(usize, usize, Vec<u8>)> {
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
