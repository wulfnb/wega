//! The toolbox shares the active page's state, but owns its OS window and GPU context.
use crate::browser::Browser;
use servo::{RenderingContext, WindowRenderingContext};
use std::time::{Duration, Instant};
use winit::{
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    raw_window_handle::{HasDisplayHandle, HasWindowHandle},
    window::Window,
};

pub use wega::model::Dock;
pub struct ToolWindow {
    pub window: Window,
    context: WindowRenderingContext,
    pub(crate) gui: egui_glow::EguiGlow,
    last_frame: Instant,
    pub frames: u64,
    modifiers: winit::keyboard::ModifiersState,
}
impl Drop for ToolWindow {
    fn drop(&mut self) {
        if self.context.make_current().is_ok() {
            self.gui.destroy();
        }
    }
}
impl Browser {
    pub fn cancel_element_picker(&mut self) -> bool {
        let active = self.tools.pages.values().any(|page| page.inspector.picking);
        if active {
            for page in self.tools.pages.values_mut() {
                page.inspector.picking = false;
            }
            for view in self
                .session()
                .tabs()
                .iter()
                .filter_map(crate::browser::Tab::view)
            {
                view.evaluate_javascript("globalThis.__wegaPickerCancel?.()".to_owned(), |_| {});
            }
        }
        active
    }
    pub fn close_developer_tools(&mut self) {
        self.chrome.show_console = false;
        self.tools.resume_all();
        for view in self
            .session()
            .tabs()
            .iter()
            .filter_map(crate::browser::Tab::view)
        {
            view.evaluate_javascript("globalThis.__wegaPickerCancel?.()".to_owned(), |_| {});
        }
    }
    pub fn toggle_developer_tools(&mut self) {
        if self.is_wega() {
            self.notify(
                crate::ux::Severity::Info,
                "Page developer tools require Servo in this version.",
                None,
            );
            return;
        }
        if self.chrome.show_console {
            self.close_developer_tools();
        } else {
            self.chrome.show_console = true;
        }
        self.window.request_redraw();
    }
    pub fn sync_tool_window(&mut self, event_loop: &ActiveEventLoop) {
        let needed = self.chrome.show_console && self.profile().devtools_dock == Dock::Window;
        if !needed {
            if self.tool_window.take().is_some() {
                let _ = self.context.make_current();
                self.window.request_redraw();
            }
            return;
        }
        if self.tool_window.is_none() {
            let created = (|| -> Result<ToolWindow, String> {
                let window = event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_visible(false)
                            .with_title(format!("Developer Tools — {}", crate::brand::APP_NAME))
                            .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 720.0))
                            .with_min_inner_size(winit::dpi::LogicalSize::new(480.0, 360.0)),
                    )
                    .map_err(|e| e.to_string())?;
                let context = WindowRenderingContext::new(
                    event_loop.display_handle().map_err(|e| e.to_string())?,
                    window.window_handle().map_err(|e| e.to_string())?,
                    window.inner_size(),
                )
                .map_err(|e| format!("{e:?}"))?;
                context.make_current().map_err(|e| format!("{e:?}"))?;
                let mut gui = egui_glow::EguiGlow::new(
                    event_loop,
                    context.glow_gl_api(),
                    None,
                    Some(window.scale_factor() as f32),
                    false,
                );
                gui.egui_winit
                    .init_accesskit(event_loop, &window, self.event_proxy.clone());
                window.set_visible(true);
                Ok(ToolWindow {
                    window,
                    context,
                    gui,
                    last_frame: Instant::now() - Duration::from_secs(1),
                    frames: 0,
                    modifiers: Default::default(),
                })
            })();
            match created {
                Ok(window) => {
                    self.tool_window = Some(window);
                }
                Err(error) => {
                    self.tools.error = Some(format!("Cannot open developer window: {error}"));
                    self.apply(wega::application::Command::Dock(Dock::Bottom));
                }
            }
            let _ = self.context.make_current();
            self.window.request_redraw();
        }
        if let Some(window) = &self.tool_window
            && window.last_frame.elapsed() >= Duration::from_millis(100)
        {
            window.window.request_redraw();
        }
    }
    pub fn tool_window_event(&mut self, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            self.close_developer_tools();
            self.tool_window.take();
            let _ = self.context.make_current();
            self.window.request_redraw();
            return;
        }
        if let WindowEvent::ModifiersChanged(modifiers) = &event
            && let Some(window) = &mut self.tool_window
        {
            window.modifiers = modifiers.state();
        }
        if let WindowEvent::KeyboardInput { event: ref key, .. } = event
            && key.state.is_pressed()
        {
            use winit::keyboard::{Key, NamedKey};
            let shift = self
                .tool_window
                .as_ref()
                .is_some_and(|w| w.modifiers.shift_key());
            if self.debugger_shortcut(&key.logical_key, shift) {
                return;
            }
            if matches!(&key.logical_key, Key::Character(key) if key.eq_ignore_ascii_case("w"))
                && self.tool_window.as_ref().is_some_and(|w| {
                    if cfg!(target_os = "macos") {
                        w.modifiers.super_key()
                    } else {
                        w.modifiers.control_key()
                    }
                })
            {
                self.tool_window_event(WindowEvent::CloseRequested);
                return;
            }
            if key.logical_key == Key::Named(NamedKey::F12) {
                self.close_developer_tools();
                return;
            }
            if key.logical_key == Key::Named(NamedKey::Escape) {
                let cancelled = self.cancel_element_picker();
                if !cancelled && self.tools.active != crate::toolbox::Tool::Console {
                    self.tools.split_console = !self.tools.split_console;
                }
            }
        }
        let Some(mut window) = self.tool_window.take() else {
            return;
        };
        let response = window.gui.on_window_event(&window.window, &event);
        if response.repaint && !matches!(event, WindowEvent::RedrawRequested) {
            window.window.request_redraw();
        }
        match event {
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                window.context.resize(size);
                window.window.request_redraw();
            }
            WindowEvent::RedrawRequested
                if window.window.inner_size().width > 0
                    && window.window.inner_size().height > 0 =>
            {
                if let Err(e) = window.context.make_current() {
                    self.tools.error = Some(format!("Developer window GPU error: {e:?}"));
                } else {
                    crate::theme::install(
                        &window.gui.egui_ctx,
                        self.profile().theme.palette(
                            self.window.theme() != Some(winit::window::Theme::Light),
                            false,
                        ),
                    );
                    window.gui.run(&window.window, |ui| {
                        egui::CentralPanel::default()
                            .frame(
                                egui::Frame::new()
                                    .fill(crate::theme::palette(ui.ctx()).inset)
                                    .inner_margin(10),
                            )
                            .show(ui, |ui| self.devtools_contents(ui));
                    });
                    window.context.prepare_for_rendering();
                    let color = crate::theme::palette(&window.gui.egui_ctx)
                        .inset
                        .to_array()
                        .map(|c| c as f32 / 255.0);
                    window
                        .gui
                        .painter
                        .clear(window.window.inner_size().into(), color);
                    window.gui.paint(&window.window);
                    #[cfg(feature = "test-harness")]
                    if let Some(test) = &self.harness.toolbox_test
                        && let Some(name) = &test.capture
                        && name.starts_with("detached-")
                    {
                        let size = window.window.inner_size();
                        let rect = servo::DeviceIntRect::from_size(servo::DeviceIntSize::new(
                            size.width as i32,
                            size.height as i32,
                        ));
                        if let Some(image) = window.context.read_to_image(rect) {
                            image
                                .save(self.profile_dir.join(name))
                                .expect("Save detached toolbox frame");
                        }
                    }
                    window.context.present();
                    window.last_frame = Instant::now();
                    window.frames += 1;
                }
            }
            _ => {}
        }
        self.tool_window = Some(window);
        let _ = self.context.make_current();
    }
}
