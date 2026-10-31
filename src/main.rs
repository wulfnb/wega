// Browser chrome and engine integration, MPL-2.0.
mod automation;
mod brand;
mod browser;
mod capabilities;
mod capabilities_ui;
mod capture;
mod compat;
mod components;
mod console_model;
mod console_ui;
mod containers_ui;
mod cookie_inspector;
mod debug_driver;
mod debug_model;
mod debugger_ui;
mod devtools_ui;
mod file_handoff;
mod page_safety;
use wega::downloads;
mod downloads_ui;
mod engine;
mod input;
mod inspector_probes;
mod inspector_ui;
#[cfg(target_os = "macos")]
mod mac_application;
mod native_media;
mod native_notifications;
mod native_theme;
mod network;
mod network_controls;
mod performance_ui;
mod probes;
#[cfg(target_os = "windows")]
mod windows_instance;
use wega::profile;
mod accessibility;
#[cfg(feature = "test-harness")]
mod harness;
mod resources;
mod runtime_logging;
mod theme;
mod tool_window;
mod toolbox;
mod ui;
mod ux;

fn browser_icon() -> Option<winit::window::Icon> {
    let image = image::load_from_memory(include_bytes!("../assets/icons/browser-icon-256.png"))
        .ok()?
        .into_rgba8();
    let (width, height) = image.dimensions();
    winit::window::Icon::from_rgba(image.into_raw(), width, height).ok()
}

use browser::Browser;
use engine::Wake;
use std::error::Error;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

use wega::options::Options;

struct App {
    options: Options,
    browser: Option<Browser>,
    proxy: winit::event_loop::EventLoopProxy<Wake>,
    failure: Option<String>,
    pending_open: Vec<url::Url>,
    #[cfg(target_os = "macos")]
    open_events: mac_application::OpenEvents,
}

impl ApplicationHandler<Wake> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "macos")]
        self.open_events.register();
        if self.browser.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_visible(false)
            .with_title(crate::brand::APP_NAME)
            .with_window_icon(browser_icon())
            .with_transparent(cfg!(any(target_os = "macos", target_os = "windows")))
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 850.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(720.0, 480.0));
        // Wayland app id and X11 WM_CLASS (one shared field in winit): launchers
        // match this to wega.desktop to show the right icon. Wayland ignores
        // the window icon itself.
        #[cfg(target_os = "linux")]
        let attributes = winit::platform::wayland::WindowAttributesExtWayland::with_name(
            attributes, "wega", "wega",
        );
        let result = event_loop
            .create_window(attributes)
            .map_err(|e| e.to_string())
            .and_then(|window| Browser::new(window, event_loop, self.proxy.clone(), &self.options));
        match result {
            Ok(browser) => {
                self.browser = Some(browser);
                self.browser
                    .as_mut()
                    .unwrap()
                    .open_launch_urls(std::mem::take(&mut self.pending_open));
            }
            Err(error) => {
                self.failure = Some(error);
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Wake) {
        if let Wake::ContainerLaunch(addresses) = &event {
            let urls = wega::options::external_launch_urls(addresses);
            if let Some(browser) = &mut self.browser {
                browser.window.set_minimized(false);
                browser.open_launch_urls(urls);
            } else {
                self.pending_open
                    .extend(urls.into_iter().take(32 - self.pending_open.len()));
            }
            return;
        }
        if let Wake::ContainerError(message) = &event {
            if let Some(browser) = &mut self.browser {
                browser.notify(crate::ux::Severity::Error, message.clone(), None);
            }
            return;
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Wake::OpenUrls(addresses) = &event {
            let urls = wega::options::external_launch_urls(addresses);
            if let Some(browser) = &mut self.browser {
                browser.open_launch_urls(urls);
            } else {
                self.pending_open
                    .extend(urls.into_iter().take(32 - self.pending_open.len()));
            }
            return;
        }
        if let Some(browser) = &mut self.browser {
            browser.sync_tool_window(event_loop);
            match &event {
                Wake::Media(action) => browser.media_action(Some(action.clone())),
                Wake::ToggleMedia => browser.media_action(None),
                Wake::Accessibility(event) => browser.accessibility_event(event),
                _ => {}
            }
            if matches!(event, Wake::Servo) {
                // Servo also wakes for paint acknowledgements. Repainting here
                // feeds those acknowledgements back into another full paint,
                // keeping software renderers busy even on a static page.
                // Delegates request a redraw when a new frame or UI change is ready.
                browser.pump();
            } else {
                browser.window.request_redraw();
                if let Some(window) = &browser.tool_window {
                    window.window.request_redraw();
                }
            }
            browser.check_background_tasks(event_loop);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(browser) = &mut self.browser else {
            return;
        };
        browser.sync_tool_window(event_loop);
        if browser
            .tool_window
            .as_ref()
            .is_some_and(|w| w.window.id() == window_id)
        {
            browser.tool_window_event(event);
            return;
        }
        if window_id != browser.window.id() {
            return;
        }
        if matches!(event, WindowEvent::CloseRequested) {
            browser.request_quit();
        } else {
            browser.handle_event(event);
        }
        if browser.quit_requested {
            browser.save();
            event_loop.exit();
        }
        browser.check_background_tasks(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(browser) = &mut self.browser {
            browser.sync_tool_window(event_loop);
            browser.tick();
            browser.check_background_tasks(event_loop);
            // Sleep until the next real deadline; an idle window wakes only for events.
            event_loop.set_control_flow(match browser.next_wakeup() {
                Some(when) => ControlFlow::WaitUntil(when),
                None => ControlFlow::Wait,
            });
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        if let Some(mut browser) = self.browser.take() {
            browser.shutdown();
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    #[cfg(feature = "multiprocess")]
    {
        let mut args = std::env::args();
        while let Some(arg) = args.next() {
            if arg == "--content-process" {
                servo::run_content_process(args.next().ok_or("Missing content process token")?);
                return Ok(());
            }
        }
    }
    let mut options =
        Options::parse_from(std::env::args().skip(1)).map_err(std::io::Error::other)?;
    if options.help {
        println!(
            "Wega Browser\nUsage: wega [--engine servo|wega] [--profile-dir PATH] [--capture FILE.png] [URL]\n  --remote-devtools PORT · --webdriver-port PORT\n    Both open UNAUTHENTICATED endpoints on 127.0.0.1. Any local process or user\n    can then read and control open pages (cookies, sessions, input). Use them\n    only on a trusted machine and with a disposable --profile-dir.\n  --multiprocess (requires --features multiprocess)\n  Native tests require --features test-harness and an isolated --profile-dir.\n  Test modes: --engine-test, --smoke-test, --download-test, --toolbox-test, --theme-test, --capabilities-test"
        );
        println!(
            "  --container ID: open Personal (personal), Work (work), Banking (banking),\n    Shopping (shopping), or a custom ID in an isolated container window.\n    --profile-dir selects the parent profile when used with --container."
        );
        return Ok(());
    }
    options.prepare_container().map_err(std::io::Error::other)?;
    let mut container_server = if options.container.is_some() {
        let urls = options
            .url
            .as_deref()
            .map(wega::profile::resolve_address)
            .transpose()?
            .into_iter()
            .map(|url| url.to_string())
            .collect();
        let may_forward = options.engine.is_none()
            && !options.test_mode()
            && options.capture.is_none()
            && options.webdriver_port.is_none()
            && options.remote_devtools.is_none()
            && !options.multiprocess;
        match wega::container_launch::Server::claim(
            options.profile_dir.as_ref().unwrap(),
            urls,
            may_forward,
        )
        .map_err(std::io::Error::other)?
        {
            wega::container_launch::Claim::Primary(server) => Some(server),
            wega::container_launch::Claim::Forwarded => return Ok(()),
        }
    } else {
        None
    };
    runtime_logging::init(
        &options
            .profile_dir
            .clone()
            .unwrap_or_else(profile::Profile::default_dir),
    );
    if (options.engine == Some(wega::engine_choice::EngineKind::Wega) || options.engine_test)
        && wega_page::executable().is_none()
    {
        return Err("Wega engine is not installed. Run python3 scripts/build-wega-engine.py --debug, or use --engine servo.".into());
    }
    #[cfg(target_os = "windows")]
    let Some(instance) = windows_instance::claim(&options)? else {
        return Ok(());
    };
    #[cfg(feature = "media")]
    {
        let profile = options
            .profile_dir
            .clone()
            .unwrap_or_else(crate::profile::Profile::default_dir);
        let engine = profile.join("engine");
        std::fs::create_dir_all(&engine)?;
        // This runs before winit, Servo or worker threads are started. macOS and
        // Windows load Servo's explicit plugin list. Linux needs system plugin
        // discovery, since Servo uses distribution-provided GStreamer plugins.
        unsafe {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if std::env::var_os("GST_PLUGIN_SYSTEM_PATH_1_0").is_none() {
                std::env::set_var("GST_PLUGIN_SYSTEM_PATH_1_0", "");
            }
            if std::env::var_os("GST_REGISTRY_1_0").is_none() {
                std::env::set_var("GST_REGISTRY_1_0", engine.join("gstreamer-registry.bin"));
            }
        }
    }
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| std::io::Error::other("Cannot install TLS provider"))?;
    let event_loop = EventLoop::<Wake>::with_user_event().build()?;
    if let Some(server) = &mut container_server {
        let proxy = event_loop.create_proxy();
        server
            .start(move |urls| proxy.send_event(Wake::ContainerLaunch(urls)).is_ok())
            .map_err(std::io::Error::other)?;
    }
    #[cfg(target_os = "macos")]
    let open_events = mac_application::install(event_loop.create_proxy());
    #[cfg(target_os = "windows")]
    let _launch_receiver = instance.listen(event_loop.create_proxy())?;
    let mut app = App {
        options,
        browser: None,
        proxy: event_loop.create_proxy(),
        failure: None,
        pending_open: Vec::new(),
        #[cfg(target_os = "macos")]
        open_events,
    };
    let loop_result = event_loop.run_app(&mut app);
    if let Some(browser) = &mut app.browser {
        browser.shutdown_wega_workers();
    }
    wega_platform::process::wait_for_shutdowns();
    loop_result?;
    if let Some(error) = app.failure {
        return Err(std::io::Error::other(error).into());
    }
    std::process::exit(0);
}

mod chrome;
mod download_actions;
mod navigation_adapter;
mod permission_adapter;
mod servo_host;
mod wega_page;
mod wega_ui;
