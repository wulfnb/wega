//! Native chrome and Servo share one platform accessibility tree.
//! Servo 0.7 and egui 0.37 use adjacent AccessKit schema versions. Convert through
//! their supported serde formats rather than casting node IDs or enum layouts.
use crate::{browser::Browser, ux::Severity};
use egui_winit::accesskit_winit::{Event, WindowEvent};

#[derive(Default)]
pub struct State {
    pub active: bool,
    pub grafted: Option<servo::WebViewId>,
    pub pending: Vec<(servo::WebViewId, servo::accesskit::TreeUpdate)>,
    root: Option<accesskit::TreeId>,
    document: Option<accesskit::TreeId>,
    trees: std::collections::HashSet<accesskit::TreeId>,
}
pub(crate) fn tree(
    update: servo::accesskit::TreeUpdate,
) -> Result<accesskit::TreeUpdate, serde_json::Error> {
    serde_json::from_value(serde_json::to_value(update)?)
}
impl Browser {
    pub(crate) fn accessibility_event(&mut self, event: &Event) {
        if event.window_id != self.window.id() {
            if let Some(window) = self
                .tool_window
                .as_mut()
                .filter(|w| w.window.id() == event.window_id)
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
                self.accessibility.grafted = None;
                self.gui.as_ref().unwrap().egui_ctx.enable_accesskit();
                if let Some(view) = self.tab().view() {
                    view.set_accessibility_active(false);
                    view.set_accessibility_active(true);
                }
                if let Some(wega) = self.tab().wega.as_ref() {
                    wega.enable_accessibility(true);
                }
            }
            WindowEvent::ActionRequested(request) => {
                if request.target_tree == accesskit::TreeId::ROOT {
                    self.chrome.page_focused = request.target_node
                        == egui::Id::unique("web-page-accessibility").accesskit_id();
                    if let Some(view) = self.tab().view() {
                        view.set_focused(self.chrome.page_focused);
                    }
                    self.gui
                        .as_mut()
                        .unwrap()
                        .egui_winit
                        .on_accesskit_action_request(request.clone());
                } else if self.is_wega() {
                    self.chrome.page_focused = true;
                    if let Some(wega) = self.tab().wega.as_ref() {
                        let action = format!("{:?}", request.action);
                        let target = request.target_node.0;
                        let data = match &request.data {
                            Some(accesskit::ActionData::Value(val)) => Some(val.to_string()),
                            _ => None,
                        };
                        wega.accessibility_action(&action, target, data.as_deref());
                    }
                } else {
                    match serde_json::to_value(request).and_then(serde_json::from_value) {
                        Ok(request) => {
                            self.chrome.page_focused = true;
                            self.servo.forward_accessibility_action(request);
                        }
                        Err(error) => self.notify(
                            Severity::Warning,
                            format!("Accessibility action unavailable: {error}"),
                            None,
                        ),
                    }
                }
            }
            WindowEvent::AccessibilityDeactivated => {
                self.accessibility = State::default();
                for view in self
                    .session()
                    .tabs()
                    .iter()
                    .filter_map(crate::browser::Tab::view)
                {
                    view.set_accessibility_active(false);
                }
            }
        }
        self.window.request_redraw();
    }
    pub(crate) fn accessibility_viewport(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        if !self.accessibility.active {
            return;
        }
        if self.is_wega() {
            if let Some(wega) = self.tab().wega.as_ref() {
                wega.enable_accessibility(true);
            }
            let response = ui.interact(
                rect,
                egui::Id::unique("web-page-accessibility"),
                egui::Sense::focusable_noninteractive(),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::GenericContainer, true, "Web page")
            });
            let scale = ui.ctx().pixels_per_point() as f64;
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_role(accesskit::Role::GenericContainer);
                node.set_bounds(accesskit::Rect::new(
                    0.0,
                    0.0,
                    rect.width() as f64 * scale,
                    rect.height() as f64 * scale,
                ));
                node.set_transform(accesskit::Affine::translate((
                    rect.min.x as f64 * scale,
                    rect.min.y as f64 * scale,
                )));
                node.set_label("Web page");
            });
            if self.chrome.page_focused {
                response.request_focus();
            }
            return;
        }
        let view = self.view().clone();
        if self.accessibility.grafted != Some(view.id()) {
            // Removing a native graft removes its subtree. Revisiting a tab
            // therefore needs a fresh full tree, not incremental stale nodes.
            self.accessibility.pending.clear();
            self.accessibility.trees.clear();
            self.accessibility.document = None;
            view.set_accessibility_active(false);
        }
        let Some(id) = view.set_accessibility_active(true) else {
            return;
        };
        let Ok(id) = serde_json::from_value::<accesskit::TreeId>(serde_json::to_value(id).unwrap())
        else {
            return;
        };
        self.accessibility.root = Some(id);
        self.accessibility.trees.insert(id);
        let response = ui.interact(
            rect,
            egui::Id::unique("web-page-accessibility"),
            egui::Sense::focusable_noninteractive(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::GenericContainer, true, "Web page")
        });
        // Read context values before entering the node builder: it holds the
        // context write lock, so re-entering Context from the callback deadlocks.
        let scale = ui.ctx().pixels_per_point() as f64;
        ui.ctx().accesskit_node_builder(response.id, |node| {
            node.set_role(accesskit::Role::GenericContainer);
            node.set_bounds(accesskit::Rect::new(
                0.0,
                0.0,
                rect.width() as f64 * scale,
                rect.height() as f64 * scale,
            ));
            node.set_transform(accesskit::Affine::translate((
                rect.min.x as f64 * scale,
                rect.min.y as f64 * scale,
            )));
            node.set_label("Web page");
            node.set_tree_id(id);
        });
        if self.chrome.page_focused {
            response.request_focus();
        }
        self.accessibility.grafted = Some(view.id());
    }
    pub(crate) fn flush_accessibility(&mut self, gui: &mut egui_glow::EguiGlow) {
        // EguiGlow has now published the parent graft node. Publishing a child
        // earlier violates AccessKit's subtree ordering contract.
        let updates = std::mem::take(&mut self.accessibility.pending);
        for (view, update) in updates {
            if !self.accessibility.active || self.accessibility.grafted != Some(view) {
                continue;
            }
            match tree(update) {
                Ok(update) => {
                    if !self.accessibility.trees.contains(&update.tree_id) {
                        continue;
                    }
                    let grafts: Vec<_> = update
                        .nodes
                        .iter()
                        .filter_map(|(_, node)| node.tree_id())
                        .collect();
                    if Some(update.tree_id) == self.accessibility.root
                        && let Some(document) = grafts.first().copied()
                        && self.accessibility.document != Some(document)
                    {
                        self.accessibility.document = Some(document);
                        self.accessibility.trees.clear();
                        self.accessibility.trees.insert(update.tree_id);
                    }
                    self.accessibility.trees.extend(grafts);
                    if let Some(adapter) = gui.egui_winit.accesskit.as_mut() {
                        adapter.update_if_active(|| update);
                    }
                }
                Err(error) => self.notify(
                    Severity::Warning,
                    format!("Accessibility update unavailable: {error}"),
                    None,
                ),
            }
        }
    }
}

#[cfg(test)]
#[path = "accessibility_tests.rs"]
mod tests;
