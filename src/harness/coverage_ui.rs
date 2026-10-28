//! Native contracts use a real Browser/Servo and drive widgets through AccessKit.
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
    pub(crate) fn check_native_ui_contracts(&mut self, fixture: &str) {
        let feature = |b: &mut Self, ui: &mut egui::Ui| b.ui(ui);
        self.features.settings = true;
        self.features.settings_section = 0;
        self.coverage_click(&["Tabs on top"], feature);
        assert_eq!(self.profile().layout, crate::profile::TabLayout::Top);
        self.coverage_click(&["Tabs in sidebar"], feature);
        assert_eq!(self.profile().layout, crate::profile::TabLayout::Side);
        self.features.settings_section = 1;
        self.coverage_click(&["Offline mode — block HTTP/HTTPS requests"], feature);
        assert!(self.profile().policy.offline);
        self.coverage_click(&["Offline mode — block HTTP/HTTPS requests"], feature);
        assert!(!self.profile().policy.offline);
        self.features.settings_section = 4;
        self.features.script_source = "window.__coverageScript = 91".into();
        self.coverage_click(&["Apply script"], feature);
        assert!(self.features.script.is_some());
        assert_eq!(
            self.profile().policy.user_script,
            "window.__coverageScript = 91"
        );
        self.features.script_source.clear();
        self.coverage_click(&["Apply script"], feature);
        assert!(self.features.script.is_none());
        self.coverage_click(&["Open developer diagnostics"], feature);
        assert!(self.features.diagnostics);
        assert!(
            !self.features.settings,
            "Diagnostics replaces the settings sheet"
        );
        self.features.diagnostics = false;
        self.features.settings = true;
        self.features.settings_section = 2;
        self.coverage_click(&["Review cookies and stored sites"], feature);
        assert!(self.features.site_info);
        assert!(
            !self.features.settings,
            "Site data replaces the settings sheet"
        );
        self.features.site_info = false;
        self.features.settings = true;
        self.coverage_click(&["Clear browsing data…", "Cancel"], feature);
        assert!(!self.features.clear_data);
        assert!(
            !self.features.settings,
            "Data confirmation replaces the settings sheet"
        );
        self.features.settings = true;
        self.coverage_click(&["Clear browsing data…", "Clear data"], feature);
        assert!(!self.features.clear_data);
        assert!(self.profile().history.is_empty());
        self.features.settings = false;
        let origin = url::Url::parse(fixture)
            .unwrap()
            .origin()
            .ascii_serialization();
        self.features.delete_site = Some(origin.clone());
        self.coverage_click(&["Cancel"], feature);
        assert!(self.features.delete_site.is_none());
        self.features.delete_site = Some(origin);
        self.coverage_click(&["Delete site data"], feature);
        assert!(self.features.delete_site.is_none());
        let owner = crate::network::owner(self.view().id());
        self.coverage_click(&["Options", "Import HAR file…"], |b, ui| {
            b.network_options(ui, &owner)
        });
        assert!(self.tools.network_ui.import_open);
        self.tools.network_ui.import_path =
            self.profile_dir.join("missing.har").display().to_string();
        self.coverage_click(&["Import"], |b, ui| b.network_dialogs(ui.ctx(), &owner));
        assert!(self.tools.network_ui.import_open);
        assert!(self.tools.notice.is_some());
        let file = self.profile_dir.join("import-ui.har");
        std::fs::write(
            &file,
            serde_json::to_vec(&self.tools.network.har(&owner)).unwrap(),
        )
        .unwrap();
        self.tools.network_ui.import_path = file.display().to_string();
        self.coverage_click(&["Import"], |b, ui| b.network_dialogs(ui.ctx(), &owner));
        assert!(!self.tools.network_ui.import_open);
        assert!(self.tools.notice.as_ref().unwrap().starts_with("Imported "));
        self.tools.network_ui.composer = Some(crate::network::Composer::default());
        self.coverage_click(&["Send request"], |b, ui| {
            b.network_dialogs(ui.ctx(), &owner)
        });
        assert!(
            !self
                .tools
                .notice
                .as_ref()
                .unwrap()
                .starts_with("Request sent")
        );
        self.tools.network_ui.composer.as_mut().unwrap().url = format!(
            "{}/api/data",
            fixture.trim_end_matches("/capabilities.html")
        );
        self.coverage_click(&["Send request"], |b, ui| {
            b.network_dialogs(ui.ctx(), &owner)
        });
        assert!(
            self.tools
                .notice
                .as_ref()
                .unwrap()
                .starts_with("Request sent")
        );
        self.tools.network_ui.composer = None;
        self.coverage_click(&["Options", "Save all as HAR…"], |b, ui| {
            b.network_options(ui, &owner)
        });
        assert!(self.tools.export_open);
        self.tools.export_open = false;
        self.coverage_click(&["Options", "Copy all as HAR"], |b, ui| {
            b.network_options(ui, &owner)
        });
        self.coverage_click(
            &["Options", "Host blocking / offline settings…"],
            |b, ui| b.network_options(ui, &owner),
        );
        assert!(self.features.settings);
        self.features.settings = false;
        self.download_ui.open = true;
        self.download_ui.url = "file:///not-a-network-download".into();
        self.coverage_click(&["Download"], |b, ui| b.downloads_window(ui.ctx()));
        assert!(self.download_ui.notice.is_some());
        assert!(self.downloads.jobs().is_empty());
        self.download_ui.open = false;
        self.check_chrome_command_contracts();
        println!(
            "NATIVE UI CONTRACTS PASSED: settings commands, script lifecycle, destructive confirmations, HAR dialogs, Network menus and download validation"
        );
    }
    fn check_chrome_command_contracts(&mut self) {
        use winit::{
            event::ElementState,
            keyboard::{Key, ModifiersState, NamedKey},
        };
        let command = if cfg!(target_os = "macos") {
            ModifiersState::SUPER
        } else {
            ModifiersState::CONTROL
        };
        let key = |b: &mut Self, text: &str, mods: ModifiersState| {
            b.chrome.modifiers = mods.into();
            assert!(
                b.shortcut_key(&Key::Character(text.into()), ElementState::Pressed),
                "Shortcut {text}"
            );
        };
        let original = self.view().id();
        let tabs = self.session().tabs().len();
        key(self, "t", command);
        assert_eq!(self.session().tabs().len(), tabs + 1);
        assert_eq!(self.tab().url(), "wega://newtab");
        assert!(self.chrome.page_focused && !self.chrome.focus_address);
        key(self, "1", command);
        assert_eq!(self.view().id(), original);
        key(self, "9", command);
        key(self, "w", command);
        assert_eq!(self.session().tabs().len(), tabs);
        key(self, "t", command | ModifiersState::SHIFT);
        assert_eq!(self.session().tabs().len(), tabs + 1);
        key(self, "w", command);
        key(self, "l", command);
        assert!(self.chrome.focus_address && !self.chrome.page_focused);
        let layout = self.profile().layout;
        key(self, "l", command | ModifiersState::SHIFT);
        assert_ne!(self.profile().layout, layout);
        key(self, "l", command | ModifiersState::SHIFT);
        for text in ["+", "=", "-", "0"] {
            key(self, text, command);
        }
        for (text, tool) in [
            ("i", crate::toolbox::Tool::Inspector),
            ("e", crate::toolbox::Tool::Network),
            ("k", crate::toolbox::Tool::Console),
        ] {
            key(self, text, command | ModifiersState::SHIFT);
            assert_eq!(self.tools.active, tool);
        }
        key(self, "h", command);
        assert!(self.chrome.show_history && !self.chrome.show_bookmarks);
        key(self, "b", command | ModifiersState::SHIFT);
        assert!(self.chrome.show_bookmarks && !self.chrome.show_history);
        self.chrome.modifiers = ModifiersState::empty().into();
        assert!(self.shortcut_key(&Key::Named(NamedKey::Escape), ElementState::Pressed));
        assert!(!self.chrome.show_bookmarks && !self.chrome.show_history);
        key(self, "f", command);
        assert!(self.features.find_open);
        self.features.find_open = false;
        key(self, "d", command);
        assert!(
            self.profile()
                .bookmarks
                .iter()
                .any(|b| b.url == self.tab().url())
        );
        self.chrome.modifiers = ModifiersState::empty().into();
        for _ in 0..2 {
            assert!(self.shortcut_key(&Key::Named(NamedKey::F6), ElementState::Pressed));
        }
        self.chrome.page_focused = false;
        let split = self.tools.split_console;
        assert!(self.shortcut_key(&Key::Named(NamedKey::Escape), ElementState::Pressed));
        assert_eq!(self.tools.split_console, !split);
        for _ in 0..2 {
            assert!(self.shortcut_key(&Key::Named(NamedKey::F12), ElementState::Pressed));
        }
        for mods in [
            ModifiersState::CONTROL,
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        ] {
            self.chrome.modifiers = mods.into();
            assert!(self.shortcut_key(&Key::Named(NamedKey::Tab), ElementState::Pressed));
        }
        self.chrome.modifiers = ModifiersState::empty().into();
        assert!(!self.shortcut_key(&Key::Named(NamedKey::F6), ElementState::Released));
        assert!(!self.shortcut_key(&Key::Character("z".into()), ElementState::Pressed));
        self.chrome.modifiers = command.into();
        assert!(!self.shortcut_key(&Key::Character("z".into()), ElementState::Pressed));
        key(self, "q", command);
        self.settle_page_check();
        if self.destructive_action.is_some() {
            assert!(!self.quit_requested);
            self.coverage_click(&["Keep working"], |b, ui| b.ui(ui));
            key(self, "q", command);
            self.coverage_click(&["Quit browser"], |b, ui| b.ui(ui));
        }
        assert!(self.quit_requested);
        self.quit_requested = false;
        self.chrome.modifiers = ModifiersState::empty().into();
        self.chrome.show_console = false;
        self.tools.split_console = false;
        self.select_tab(0);
        self.select_tab(usize::MAX);
        self.close_tab(usize::MAX);
        assert_eq!(self.view().id(), original);
        self.add_tab("http://[");
        assert!(self.status.is_some());
        self.navigate("http://[");
        assert!(self.status.is_some());
        self.status = None;
        // IME state transitions go through the same window event handler as real input.
        self.chrome.page_focused = true;
        self.handle_event(winit::event::WindowEvent::Ime(winit::event::Ime::Preedit(
            "試".into(),
            Some((0, 3)),
        )));
        assert!(self.chrome.ime_composing);
        self.handle_event(winit::event::WindowEvent::Ime(winit::event::Ime::Preedit(
            "試験".into(),
            None,
        )));
        self.handle_event(winit::event::WindowEvent::Ime(winit::event::Ime::Commit(
            "試験".into(),
        )));
        self.handle_event(winit::event::WindowEvent::Ime(winit::event::Ime::Disabled));
        assert!(!self.chrome.ime_composing);
        self.handle_event(winit::event::WindowEvent::Focused(false));
        assert!(!self.chrome.page_focused);
        self.check_library_contracts();
        println!(
            "CHROME COMMAND CONTRACTS PASSED: tabs, focus, shortcuts, zoom, libraries and IME transitions"
        );
    }
    fn check_library_contracts(&mut self) {
        use wega::application::Command;
        let render = |b: &mut Self, ui: &mut egui::Ui| b.ui(ui);
        self.apply(Command::Visit(crate::profile::Bookmark {
            title: "Coverage library fixture".into(),
            url: "http://127.0.0.1/coverage-history".into(),
            visited_at: chrono::Utc::now().timestamp() as u64,
            visit_count: 0,
        }));
        self.chrome.show_history = true;
        self.chrome.show_bookmarks = false;
        self.chrome.library_query = "no matching history fixture".into();
        self.coverage_click(&[], render);
        self.chrome.library_query.clear();
        self.coverage_click(&["Clear history…", "Cancel"], render);
        assert!(!self.chrome.confirm_clear_history && !self.profile().history.is_empty());
        self.coverage_click(&["Clear history…", "Clear history"], render);
        assert!(self.profile().history.is_empty());
        assert!(!self.chrome.confirm_clear_history);
        self.coverage_click(&["Bookmarks"], render);
        assert!(self.chrome.show_bookmarks && !self.chrome.show_history);
        // Add a named disposable bookmark through the application command, then remove it
        // with the row's accessible control and verify the persisted model changed.
        let bookmark_url = "http://127.0.0.1/coverage-saved-page";
        self.apply(Command::ToggleBookmark {
            title: "Disposable coverage bookmark".into(),
            url: bookmark_url.into(),
        });
        self.coverage_actions(
            &[
                (
                    accesskit::Action::Focus,
                    "Disposable coverage bookmark http://127.0.0.1/coverage-saved-page",
                ),
                (
                    accesskit::Action::Click,
                    "Remove bookmark: Disposable coverage bookmark",
                ),
            ],
            render,
        );
        assert!(
            !self
                .profile()
                .bookmarks
                .iter()
                .any(|b| b.url == bookmark_url)
        );
        self.save();
        let saved = crate::profile::Profile::load(&self.profile_dir).unwrap();
        assert!(!saved.bookmarks.iter().any(|b| b.url == bookmark_url));
        assert!(saved.history.is_empty());
        self.coverage_click(&["History", "Close library"], render);
        assert!(!self.chrome.show_history && !self.chrome.show_bookmarks);
        self.features.settings = true;
        self.features.settings_section = 1;
        let changes = self.profile().policy.allow_window_changes;
        self.coverage_click(&["Allow sites to move and resize this window"], render);
        assert_eq!(self.profile().policy.allow_window_changes, !changes);
        self.coverage_click(&["Allow sites to move and resize this window"], render);
        self.features.settings_section = 2;
        self.coverage_click(&["Clear session cookies"], render);
        assert_eq!(
            self.chrome.notice.as_ref().unwrap().message,
            "Session cookies cleared."
        );
        self.features.settings_section = 3;
        self.coverage_click(&["Open download manager"], render);
        assert!(self.download_ui.open);
        assert!(
            !self.features.settings,
            "Downloads replaces the settings sheet"
        );
        self.download_ui.open = false;
        self.features.settings = false;
        self.features.find_open = true;
        self.features.find = "Capability".into();
        self.coverage_click(&["Next", "Previous"], render);
        self.features.find_open = false;
        self.chrome.focus_address = false;
        self.chrome.page_focused = true;
        self.view().set_focused(true);
        let ctx = egui::Context::default();
        let previous = egui::Id::unique("coverage-dialog-previous-focus");
        ctx.memory_mut(|m| m.request_focus(previous));
        assert!(self.prepare_dialog(&ctx, "coverage-dialog"));
        assert!(!self.prepare_dialog(&ctx, "coverage-dialog"));
        self.finish_dialog(&ctx);
        assert_eq!(ctx.memory(|m| m.focused()), Some(previous));
        assert!(self.chrome.dialog_key.is_none());
        println!(
            "LIBRARY CONTRACTS PASSED: filter, tabs, history confirmations and bookmark removal"
        );
    }
    pub(crate) fn check_tool_ui_contracts(&mut self) {
        use crate::toolbox::Tool;
        self.select_tab(0);
        let view = self.view().clone();
        let owner = crate::network::owner(view.id());
        self.tools.page(&view);
        self.tools.split_console = false;
        let render = |b: &mut Self, ui: &mut egui::Ui| b.devtools_contents(ui);
        self.tools.active = Tool::Console;
        let timestamps = self.tools.pages[&owner].console_state.timestamps;
        self.coverage_click(&["Options", "Timestamps"], render);
        assert_eq!(
            self.tools.pages[&owner].console_state.timestamps,
            !timestamps
        );
        self.coverage_click(&["Options", "Copy visible output"], render);
        self.coverage_click(&["Options", "Save visible output…", "Save"], render);
        assert!(self.profile_dir.join("console.log").exists());
        self.tools.page(&view).console_input = "40 + 2".into();
        self.coverage_click(&["Run"], render);
        assert!(
            self.tools
                .page(&view)
                .console_state
                .history
                .iter()
                .any(|s| s == "40 + 2")
        );
        self.coverage_click(&["Options", "Multiline editor"], render);
        assert!(self.tools.pages[&owner].console_state.multiline);
        self.coverage_click(&["Options", "JS"], render);
        assert!(
            !self.tools.pages[&owner]
                .console_state
                .categories
                .contains("JS")
        );
        self.coverage_click(&["Options", "JS"], render);
        assert!(
            self.tools.pages[&owner]
                .console_state
                .categories
                .contains("JS")
        );
        self.tools.active = Tool::Network;
        self.tools.export_open = true;
        self.tools.export_path = self
            .profile_dir
            .join("ui-network.har")
            .display()
            .to_string();
        self.coverage_click(&["Save HAR"], render);
        assert!(self.profile_dir.join("ui-network.har").exists());
        assert!(!self.tools.export_open);
        self.tools.export_open = true;
        self.coverage_click(&["Cancel"], render);
        assert!(!self.tools.export_open);
        self.tools.active = Tool::Debugger;
        self.tools
            .debug
            .get_mut(&owner)
            .expect("Original page debugger")
            .advanced
            .search = "advanced".into();
        self.tools
            .debug
            .get_mut(&owner)
            .unwrap()
            .advanced
            .search_all = true;
        self.coverage_click(&["Search"], render);
        assert!(!self.tools.debug[&owner].advanced.hits.is_empty());
        self.coverage_click(&["Clear search"], render);
        assert!(self.tools.debug[&owner].advanced.hits.is_empty());
        self.tools
            .debug
            .get_mut(&owner)
            .unwrap()
            .advanced
            .watch_input = "21 * 2".into();
        let count = self.tools.debug[&owner].advanced.watches.len();
        self.coverage_click(&["Add"], render);
        assert_eq!(self.tools.debug[&owner].advanced.watches.len(), count + 1);
        self.coverage_click(&["Remove all breakpoints"], render);
        assert!(self.tools.debug[&owner].breakpoints.is_empty());
        self.coverage_click(&["…", "Rendering diagnostics…"], render);
        assert!(self.features.diagnostics);
        self.features.diagnostics = false;
        self.tools.active = Tool::Console;
        self.coverage_click(&["Clear"], render);
        let left: Vec<_> = self
            .tools
            .log
            .entries
            .iter()
            .filter(|e| e.owner == owner)
            .map(|e| format!("{} / {}: {}", e.level, e.category, e.text))
            .collect();
        assert!(left.is_empty(), "Console Clear left entries: {left:?}");
        println!(
            "TOOL UI CONTRACTS PASSED: console filters/export/evaluation, HAR export, debugger search/watches/breakpoints and diagnostics"
        );
    }
}
