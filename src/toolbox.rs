//! Native standalone worker panel state.
pub struct Toolbox {
    pub panel_rect: egui::Rect,
    pub error: Option<String>,
}
impl Default for Toolbox {
    fn default() -> Self {
        Self {
            panel_rect: egui::Rect::NOTHING,
            error: None,
        }
    }
}
