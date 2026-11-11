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
    wega: Option<WegaTree>,
}
type WegaOwner = (
    u64,
    wega::engine_protocol::identity::SessionIdentity,
    wega::engine_protocol::DocumentContext,
);
struct WegaTree {
    owner: WegaOwner,
    id: accesskit::TreeId,
    pending: Option<accesskit::TreeUpdate>,
    published: bool,
    parent_ready: bool,
}
impl State {
    fn wega_tree(&mut self, owner: WegaOwner) -> Result<&mut WegaTree, String> {
        if self.wega.as_ref().is_none_or(|tree| tree.owner != owner) {
            let mut bytes = [0; 16];
            getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
            bytes[6] = (bytes[6] & 15) | 64;
            bytes[8] = (bytes[8] & 63) | 128;
            let uuid = bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            self.wega = Some(WegaTree {
                owner,
                id: accesskit::TreeId(
                    uuid.parse()
                        .map_err(|_| "Invalid page accessibility tree identifier")?,
                ),
                pending: None,
                published: false,
                parent_ready: false,
            });
        }
        Ok(self.wega.as_mut().unwrap())
    }
    pub(crate) fn queue_wega(
        &mut self,
        tab: u64,
        requester: &wega::engine_protocol::authority::TrustedRequest,
        json: &str,
    ) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }
        let mut update: accesskit::TreeUpdate =
            serde_json::from_str(json).map_err(|error| error.to_string())?;
        validate_wega_tree(&update)?;
        let tree = self.wega_tree((tab, requester.session, requester.document))?;
        update.tree_id = tree.id;
        // Only the coordinator can allocate grafts. Renderer node identities are
        // scoped to this document, and must never replace the chrome root tree.
        let root = update.tree.as_ref().unwrap().root;
        for (id, node) in &mut update.nodes {
            node.clear_tree_id();
            if node.role() == accesskit::Role::Window {
                node.set_role(if *id == root {
                    accesskit::Role::RootWebArea
                } else {
                    accesskit::Role::GenericContainer
                });
            }
        }
        tree.pending = Some(update);
        Ok(())
    }
}
fn validate_wega_tree(update: &accesskit::TreeUpdate) -> Result<(), String> {
    let root = update
        .tree
        .as_ref()
        .ok_or("Missing page tree metadata")?
        .root;
    if update.nodes.len() > 30_002 {
        return Err("Page accessibility tree exceeds the node budget".into());
    }
    let nodes: std::collections::HashMap<_, _> =
        update.nodes.iter().map(|(id, node)| (*id, node)).collect();
    if nodes.len() != update.nodes.len() || !nodes.contains_key(&update.focus) {
        return Err("Invalid page accessibility node identities".into());
    }
    let mut visited = std::collections::HashSet::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            return Err("Page accessibility tree has a cycle or multiple parents".into());
        }
        let node = nodes.get(&id).ok_or("Missing page accessibility child")?;
        pending.extend_from_slice(node.children());
    }
    if visited.len() != nodes.len() {
        return Err("Page accessibility tree contains detached nodes".into());
    }
    Ok(())
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
                self.accessibility.wega = None;
                self.gui.as_ref().unwrap().egui_ctx.enable_accesskit();
                if let Some(view) = self.tab().view() {
                    view.set_accessibility_active(false);
                    view.set_accessibility_active(true);
                }
                if let Some(wega) = self.tab().wega.as_ref() {
                    wega.enable_accessibility(false);
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
                    if self.accessibility.wega.as_ref().is_some_and(|tree| {
                        tree.id == request.target_tree
                            && self.wega_accessibility_owner() == Some(tree.owner)
                    }) {
                        self.chrome.page_focused = true;
                        let action = format!("{:?}", request.action);
                        let target = request.target_node.0;
                        let data = match &request.data {
                            Some(accesskit::ActionData::Value(val)) => Some(val.to_string()),
                            _ => None,
                        };
                        if let Some(wega) = self.tab().wega.as_ref() {
                            wega.accessibility_action(&action, target, data.as_deref());
                        }
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
                for page in self
                    .session()
                    .tabs()
                    .iter()
                    .filter_map(|tab| tab.wega.as_ref())
                {
                    page.enable_accessibility(false);
                }
            }
        }
        self.window.request_redraw();
    }
    fn wega_accessibility_owner(&self) -> Option<WegaOwner> {
        let state = self.tab().wega.as_ref()?.state.lock().ok()?;
        Some((
            self.tab().key,
            state.authority.as_ref()?.session(),
            wega::engine_protocol::DocumentContext {
                document: state.requests.document(),
                revision: state.requests.revision(),
            },
        ))
    }
    pub(crate) fn accessibility_viewport(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        if !self.accessibility.active {
            return;
        }
        if self.is_wega() {
            let Some(owner) = self.wega_accessibility_owner() else {
                self.accessibility.wega = None;
                return;
            };
            let Ok(tree) = self.accessibility.wega_tree(owner) else {
                return;
            };
            let id = tree.id;
            let published = tree.published;
            tree.parent_ready = true;
            self.accessibility.grafted = None;
            self.accessibility.pending.clear();
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
            let page_scale = scale * self.page_zoom() as f64;
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_role(accesskit::Role::GenericContainer);
                node.set_bounds(accesskit::Rect::new(
                    0.0,
                    0.0,
                    rect.width() as f64 * scale / page_scale,
                    rect.height() as f64 * scale / page_scale,
                ));
                node.set_transform(
                    accesskit::Affine::translate((
                        rect.min.x as f64 * scale,
                        rect.min.y as f64 * scale,
                    )) * accesskit::Affine::scale(page_scale),
                );
                node.set_label("Web page");
                node.set_tree_id(id);
            });
            if !published {
                // AccessKit forbids focusing a graft before its first subtree
                // update. Restore content focus on the next UI frame.
                response.surrender_focus();
            } else if self.chrome.page_focused {
                response.request_focus();
            }
            return;
        }
        self.accessibility.wega = None;
        let Some(view) = self.tab().view().cloned() else {
            return;
        };
        if self.accessibility.grafted != Some(view.id()) {
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
        let owner = self.wega_accessibility_owner();
        if self.accessibility.active
            && let Some(tree) = self.accessibility.wega.as_mut()
            && owner == Some(tree.owner)
            && tree.parent_ready
            && let Some(update) = tree.pending.take()
            && let Some(adapter) = gui.egui_winit.accesskit.as_mut()
        {
            adapter.update_if_active(|| {
                tree.published = true;
                update
            });
        }
        if let Some(tree) = self.accessibility.wega.as_mut() {
            tree.parent_ready = false;
        }
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
