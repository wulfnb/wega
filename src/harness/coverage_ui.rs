//! Native contracts use the native browser and drive widgets through AccessKit.
use crate::browser::Browser;
impl Browser {
    pub(crate) fn settle_page_check(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while self.page_check.is_some() && std::time::Instant::now() < deadline {
            self.pump();
            self.expire_page_check();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(
            self.page_check.is_none(),
            "page-state check did not complete"
        );
    }
    pub(crate) fn coverage_click(
        &mut self,
        labels: &[&str],
        render: impl FnMut(&mut Self, &mut egui::Ui),
    ) {
        self.settle_page_check();
        let actions: Vec<_> = labels
            .iter()
            .map(|label| (accesskit::Action::Click, *label))
            .collect();
        self.coverage_actions(&actions, render);
    }
    fn coverage_actions(
        &mut self,
        actions: &[(accesskit::Action, &str)],
        mut render: impl FnMut(&mut Self, &mut egui::Ui),
    ) {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let mut events = Vec::new();
        for (action, label) in actions
            .iter()
            .copied()
            .chain(std::iter::once((accesskit::Action::Click, "")))
        {
            let mut clicked = false;
            let mut available = Vec::new();
            // Automatic egui ids shift when time-dependent UI (e.g. an expiring toast) changes
            // between frames, so a request is only delivered once the frame that consumed it
            // still contains the same labelled node; otherwise it is requested again.
            let mut pending = None;
            for _ in 0..8 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1400.0, 1000.0),
                        )),
                        events: std::mem::take(&mut events),
                        ..Default::default()
                    },
                    |ui| render(self, ui),
                );
                // Window sizing can require another frame before its contents are visible.
                // Clear GPU deltas before any assertion so a failed test has one diagnostic.
                output.textures_delta.clear();
                if label.is_empty() {
                    clicked = true;
                    break;
                }
                let nodes = &output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes;
                available = nodes
                    .iter()
                    .filter_map(|(_, n)| n.label().map(str::to_owned))
                    .collect();
                if let Some(target) = pending.take()
                    && nodes
                        .iter()
                        .any(|(id, n)| *id == target && n.label() == Some(label))
                {
                    clicked = true;
                    break;
                }
                if let Some((id, _)) = nodes
                    .iter()
                    .find(|(_, n)| n.label() == Some(label) && n.supports_action(action))
                {
                    events.push(egui::Event::AccessKitActionRequest(
                        accesskit::ActionRequest {
                            action,
                            target_tree: accesskit::TreeId::ROOT,
                            target_node: *id,
                            data: None,
                        },
                    ));
                    pending = Some(*id);
                }
            }
            assert!(
                clicked,
                "Missing clickable control {label}; available: {available:?}"
            );
        }
        self.window.request_redraw();
    }
}
