//! UI wakeups from native services and standalone content workers.
#[derive(Clone, Debug)]
pub enum Wake {
    ContainerLaunch(Vec<String>),
    ContainerError(String),
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    OpenUrls(Vec<String>),
    Ui,
    Media(wega::engine_protocol::media::MediaSessionAction),
    ToggleMedia,
    Accessibility(std::sync::Arc<egui_winit::accesskit_winit::Event>),
}
impl From<egui_winit::accesskit_winit::Event> for Wake {
    fn from(event: egui_winit::accesskit_winit::Event) -> Self {
        Self::Accessibility(std::sync::Arc::new(event))
    }
}
