//! Native standalone worker panel state.
pub struct Toolbox {
    pub panel_rect: egui::Rect,
    pub error: Option<String>,
    /// The Console tab is selected (otherwise Status).
    pub console_tab: bool,
    pub elements_tab: bool,
}
impl Default for Toolbox {
    fn default() -> Self {
        Self {
            panel_rect: egui::Rect::NOTHING,
            error: None,
            console_tab: false,
            elements_tab: false,
        }
    }
}
