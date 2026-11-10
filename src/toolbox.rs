//! Native standalone-worker toolbox state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tool {
    Inspector,
    #[default]
    Console,
    Network,
}
pub struct Toolbox {
    pub active: Tool,
    pub split_console: bool,
    pub panel_rect: egui::Rect,
    pub error: Option<String>,
}

impl Default for Toolbox {
    fn default() -> Self {
        Self {
            active: Tool::Console,
            split_console: false,
            panel_rect: egui::Rect::NOTHING,
            error: None,
        }
    }
}
