//! Native chrome and standalone content accessibility.
use crate::browser::Browser;
use egui_winit::accesskit_winit::{Event, WindowEvent};
#[derive(Default)]
pub struct State {
    pub active: bool,
}
impl Browser {
    pub(crate) fn accessibility_event(&mut self, event: &Event) {
        if event.window_id != self.window.id() {
            if let Some(window) = self
                .tool_window
                .as_mut()
                .filter(|window| window.window.id() == event.window_id)
            {
                match &event.window_event {
                    WindowEvent::InitialTreeRequested => window.gui.egui_ctx.enable_accesskit(),
                    WindowEvent::ActionRequested(request) => window
                        .gui
                        .egui_winit
                        .on_accesskit_action_request(request.clone()),
                    WindowEvent::AccessibilityDeactivated => {}
                }
                window.window.request_redraw();
            }
            return;
        }
        match &event.window_event {
            WindowEvent::InitialTreeRequested => {
                self.accessibility.active = true;
                self.gui.as_ref().unwrap().egui_ctx.enable_accesskit();
                if let Some(page) = &self.tab().wega {
                    page.enable_accessibility(true);
                }
            }
            WindowEvent::ActionRequested(request) => {
                if request.target_tree == accesskit::TreeId::ROOT {
                    self.gui
                        .as_mut()
                        .unwrap()
                        .egui_winit
                        .on_accesskit_action_request(request.clone());
                } else if let Some(page) = &self.tab().wega {
                    let data = match &request.data {
                        Some(accesskit::ActionData::Value(value)) => Some(value.to_string()),
                        _ => None,
                    };
                    page.accessibility_action(
                        &format!("{:?}", request.action),
                        request.target_node.0,
                        data.as_deref(),
                    );
                    self.chrome.page_focused = true;
                }
            }
            WindowEvent::AccessibilityDeactivated => {
                self.accessibility.active = false;
                for tab in self.session().tabs() {
                    if let Some(page) = &tab.wega {
                        page.enable_accessibility(false);
                    }
                }
            }
        }
        self.window.request_redraw();
    }
    pub(crate) fn accessibility_viewport(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        if !self.accessibility.active {
            return;
        }
        if let Some(page) = &self.tab().wega {
            page.enable_accessibility(true);
        }
        let response = ui.interact(
            rect,
            egui::Id::unique("web-page-accessibility"),
            egui::Sense::focusable_noninteractive(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::GenericContainer, true, "Web page")
        });
        if self.chrome.page_focused {
            response.request_focus();
        }
    }
}
