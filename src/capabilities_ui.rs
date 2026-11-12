//! Native settings and page actions for the standalone engine.
use crate::{browser::Browser, capabilities::Decision};
impl Browser {
    pub fn screenshot_page(&mut self) {
        let frame = self
            .tab()
            .wega
            .as_ref()
            .and_then(|page| page.state.lock().ok()?.last_frame.clone());
        let Some((width, height, pixels)) = frame else {
            self.notify(
                crate::ux::Severity::Warning,
                "No page frame is available yet.",
                None,
            );
            return;
        };
        if let Some(path) = rfd::FileDialog::new()
            .set_parent(self.window.as_ref())
            .set_file_name("wega-page.png")
            .add_filter("PNG", &["png"])
            .save_file()
        {
            let dark = self
                .profile()
                .theme
                .dark(self.window.theme() != Some(winit::window::Theme::Light));
            let result = crate::capture::page_image(width, height, &pixels, dark)
                .ok_or_else(|| "Invalid page frame".to_string())
                .and_then(|image| image.save(&path).map_err(|error| error.to_string()));
            match result {
                Ok(()) => self.notify(
                    crate::ux::Severity::Success,
                    "Screenshot saved.",
                    Some(path),
                ),
                Err(error) => self.notify(crate::ux::Severity::Error, error, None),
            }
        }
    }
    pub fn reload_without_cache(&mut self) {
        if let Err(error) = self.dispatch_navigation(wega::navigation::Command::ReloadBypassCache) {
            self.notify(crate::ux::Severity::Error, error, None);
        }
    }
    pub fn refresh_site_data(&mut self) {
        self.features.cookies.clear();
        if let Some(page) = &self.tab().wega {
            let cookies = page.cookies();
            self.features.cookies = cookies
                .into_iter()
                .map(|cookie| {
                    format!(
                        "{}={} ({}, {}{})",
                        cookie.name,
                        if self.features.show_cookie_values {
                            cookie.value
                        } else {
                            "••••".into()
                        },
                        cookie.domain,
                        cookie.path,
                        if cookie.http_only { ", HttpOnly" } else { "" }
                    )
                })
                .collect();
        }
    }
    fn settings_sheet(&mut self, ctx: &egui::Context) {
        if self.features.settings {
            let mut open = true;
            let fresh = self.prepare_dialog(ctx, "settings");
            let response = egui::Modal::new(egui::Id::unique("browser-settings")).show(ctx,|ui|{
                ui.set_width(620.0_f32.min((ctx.content_rect().width()-64.0).max(240.0)));
                ui.horizontal(|ui| {
                    ui.heading("Settings");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let close = ui.button("Close");
                        if fresh { close.request_focus(); }
                        if close.clicked() { open = false; }
                    });
                });
                crate::components::tabs(ui, &mut self.features.settings_section, &[(0,"Appearance"),(1,"Browsing"),(2,"Privacy"),(3,"Downloads"),(4,"Advanced")]);
                ui.separator();
                egui::ScrollArea::vertical().max_height((ctx.content_rect().height()-210.0).max(180.0)).show(ui,|ui|{
                    match self.features.settings_section {
                        0 => {
                            ui.heading("Appearance");
                            for theme in crate::theme::Theme::ALL {
                                if !self.native_theme.as_ref().is_some_and(|native| native.available()) && matches!(theme,crate::theme::Theme::GlassLight | crate::theme::Theme::GlassDark) { continue; }
                                ui.horizontal(|ui| {
                                    let palette=theme.palette(self.window.theme()!=Some(winit::window::Theme::Light), false);
                                    for color in [palette.chrome,palette.selected,palette.accent] {
                                        let (rect,_) = ui.allocate_exact_size(egui::vec2(18.0,18.0),egui::Sense::hover());
                                        ui.painter().rect_filled(rect,4,color);
                                    }
                                    if ui.selectable_label(self.profile().theme==theme,theme.name()).clicked(){self.set_theme(ctx,theme);}
                                });
                            }
                            ui.weak("Auto follows your system appearance. Websites can follow the chosen appearance.");
                            ui.horizontal(|ui|{
                                if ui.selectable_label(self.profile().layout == crate::profile::TabLayout::Top,"Tabs on top").clicked(){self.apply(wega::application::Command::Layout(crate::profile::TabLayout::Top));}
                                if ui.selectable_label(self.profile().layout == crate::profile::TabLayout::Side,"Tabs in sidebar").clicked(){self.apply(wega::application::Command::Layout(crate::profile::TabLayout::Side));}
                            });
                        }
                        1 => {
                    #[cfg(any(target_os="windows",target_os="macos"))]
                    ui.heading("Default browser");
                    #[cfg(target_os="windows")]
                    if ui.button("Open default browser settings").clicked() {
                        if let Err(error)=crate::ux::open_external(&url::Url::parse("ms-settings:defaultapps").unwrap()) {self.notify(crate::ux::Severity::Error,error,None);}
                    }
                    #[cfg(target_os="macos")]
                    {
                        if ui.button("Open System Settings").clicked() {
                            let result=std::process::Command::new("/usr/bin/open").args(["-b","com.apple.systempreferences"]).spawn();
                            if let Err(error)=result {self.notify(crate::ux::Severity::Error,format!("Could not open System Settings: {error}"),None);}
                        }
                        ui.label("Install Wega in Applications, then choose it under Default web browser in System Settings. Look in Desktop & Dock, or General on macOS 12.");
                    }
                    ui.heading("Navigation and requests");
                    ui.label("Host rules match the exact host and its subdomains. Separate hosts with spaces or newlines.");
                    ui.label("Blocked hosts (also blocks subresources)");
                    let mut policy = self.profile().policy.clone();
                    let mut changed=ui.add(egui::TextEdit::multiline(&mut policy.blocked_hosts).desired_rows(2).desired_width(f32::INFINITY)).changed();
                    ui.label("Allowed hosts (empty allows all; restricts navigation and subresources)");
                    changed|=ui.add(egui::TextEdit::multiline(&mut policy.allowed_hosts).desired_rows(2).desired_width(f32::INFINITY)).changed();
                    changed|=ui.checkbox(&mut policy.offline,"Offline mode — block HTTP/HTTPS requests").changed();
                    changed|=ui.checkbox(&mut policy.allow_window_changes,"Allow sites to move and resize this window").changed();
                    ui.separator();
                    ui.heading("Search");
                    egui::ComboBox::from_label("Search engine").selected_text(policy.search_engine.name()).show_ui(ui,|ui|{
                        for engine in wega::model::SearchEngine::ALL {
                            changed|=ui.selectable_value(&mut policy.search_engine,engine,engine.name()).changed();
                        }
                    });
                    if policy.search_engine==wega::model::SearchEngine::Custom {
                        ui.label("Search URL, with %s where the search terms go");
                        changed|=ui.add(egui::TextEdit::singleline(&mut policy.search_template).hint_text("https://search.example/?q=%s").desired_width(f32::INFINITY)).changed();
                        if !wega::model::valid_search_template(&policy.search_template) {
                            ui.colored_label(crate::theme::palette(ui.ctx()).amber,"Enter an http(s) URL containing %s once. Until then, Google is used.");
                        }
                    }
                    ui.separator();
                    ui.heading("Memory");
                    ui.horizontal_wrapped(|ui|{
                        ui.label("Unload background tabs after");
                        changed|=ui.add(egui::DragValue::new(&mut policy.discard_after_minutes).range(0..=wega::lifecycle::MAX_DISCARD_MINUTES).suffix(" min")).changed();
                    });
                    ui.weak("Automatic unloading applies only to browser-owned internal pages. Website and local-file tabs stay loaded to protect unsaved work. Manual unloading asks first and restores only the address, title and icon. 0 disables automatic unloading.");
                    if changed { self.apply(wega::application::Command::Policy(policy)); }

                        }
                        2 => {
                    ui.heading("Browsing data");
                    if ui.button("Review cookies and stored sites").clicked(){self.refresh_site_data();self.features.site_info=true;open=false;}
                    if ui.button("Clear browsing data…").clicked(){self.features.clear_data=true;open=false;}

                        }
                        3 => {
                            ui.heading("Downloads");
                            ui.label(format!("Save folder: {}",self.downloads.directory.display()));
                            ui.horizontal(|ui|{
                                if ui.button("Open download manager").clicked(){self.download_ui.open=true;open=false;}
                                if ui.button("Show folder").clicked() && let Err(error)=crate::ux::open_path(&self.downloads.directory,false){self.notify(crate::ux::Severity::Error,error,None);}
                            });
                            ui.weak("Open completed files or show them in their folder from Downloads. In Docker, use the viewer's Downloads button to save to your computer.");
                        }
                        _ => {
                    ui.heading("User script");
                    ui.label("Runs in every subsequently loaded page. Changes apply after reload.");
                    ui.add(egui::TextEdit::multiline(&mut self.features.script_source).code_editor().desired_rows(7).desired_width(f32::INFINITY));
                    if ui.button("Apply script").clicked(){let mut policy=self.profile().policy.clone();policy.user_script=self.features.script_source.clone();self.apply(wega::application::Command::Policy(policy));
                        self.changed();self.notify(crate::ux::Severity::Success,"User script saved. Reload a page to apply.",None);
                    }

                            let mut policy=self.profile().policy.clone();
                            if ui.checkbox(&mut policy.show_performance,"Show performance in status bar").changed(){self.apply(wega::application::Command::Policy(policy));}
                            if ui.button("Open developer diagnostics").clicked(){self.features.diagnostics=true;open=false;}
                        }
                    }
                });
            });
            self.features.settings = open && !response.should_close();
            if !self.features.settings {
                self.finish_dialog(ctx);
            }
        }
    }
    pub fn feature_ui(&mut self, ctx: &egui::Context) {
        self.settings_sheet(ctx);
        if let Some(prompt) = self.dialogs.prompts.front() {
            let title = prompt.title.clone();
            let detail = prompt.detail.clone();
            let mut answer = crate::ux::escape(ctx).then_some(false);
            egui::Modal::new(egui::Id::unique("external-application")).show(ctx, |ui| {
                ui.heading(title);
                ui.label(detail);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                    if ui.button("Continue").clicked() {
                        answer = Some(true);
                    }
                });
            });
            if let Some(allow) = answer {
                let prompt = self.dialogs.prompts.pop_front().unwrap();
                let Decision::External(url) = prompt.decision;
                if allow && let Err(error) = crate::ux::open_external(&url) {
                    self.notify(crate::ux::Severity::Error, error, None);
                }
            }
            return;
        }
        if self.features.site_info {
            let mut open = true;
            egui::Window::new("Site information")
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(self.tab().url());
                    if ui
                        .checkbox(&mut self.features.show_cookie_values, "Show cookie values")
                        .changed()
                        || ui.button("Refresh cookies").clicked()
                    {
                        self.refresh_site_data();
                    }
                    for cookie in &self.features.cookies {
                        ui.monospace(cookie);
                    }
                    if ui.button("Clear this site's data").clicked()
                        && let Some(url) = self.current_url()
                    {
                        let result = wega_network::Broker::shared().and_then(|broker| {
                            broker
                                .open_session(
                                    self.profile_dir.join("wega-storage"),
                                    wega_network::Policy::default(),
                                )?
                                .clear_site_data(&url.origin().ascii_serialization())
                        });
                        if let Err(error) = result {
                            self.notify(crate::ux::Severity::Error, error, None);
                        }
                        self.refresh_site_data();
                    }
                });
            self.features.site_info = open;
        }
        if self.features.clear_data {
            let mut answer = None;
            egui::Modal::new(egui::Id::unique("clear-browsing-data")).show(ctx, |ui| {
                ui.heading("Clear browsing data?"); ui.label("Removes cookies, website storage, cache, and history for this container. This signs you out of websites.");
                answer = crate::components::dialog_buttons(ui, Some("Cancel"), "Clear browsing data", true);
            });
            if let Some(clear) = answer {
                self.features.clear_data = false;
                if clear {
                    match wega_network::Broker::shared().and_then(|broker| {
                        broker
                            .open_session(
                                self.profile_dir.join("wega-storage"),
                                wega_network::Policy::default(),
                            )?
                            .clear_all_data()
                    }) {
                        Ok(()) => {
                            self.apply(wega::application::Command::ClearHistory);
                            self.refresh_site_data();
                            self.notify(
                                crate::ux::Severity::Success,
                                "Browsing data cleared.",
                                None,
                            );
                        }
                        Err(error) => self.notify(crate::ux::Severity::Error, error, None),
                    }
                }
            }
        }
        if self.features.diagnostics {
            let mut open = true;
            egui::Window::new("Engine diagnostics")
                .open(&mut open)
                .show(ctx, |ui| {
                    self.devtools_contents(ui);
                });
            self.features.diagnostics = open;
        }
        if self.features.find_open {
            let mut open = true;
            egui::Window::new("Find in page")
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let response = ui.text_edit_singleline(&mut self.features.find);
                        if !self.features.find_focused {
                            response.request_focus();
                            self.features.find_focused = true;
                            self.chrome.page_focused = false;
                        }
                        let previous = ui.button("Previous").clicked();
                        let next = ui.button("Next").clicked()
                            || (response.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter)));
                        if (previous || next)
                            && let Some(page) = &self.tab().wega
                        {
                            page.find_in_page(self.features.find.clone(), previous, false);
                        }
                        ui.label(self.features.find_status.borrow().as_str());
                    });
                });
            if !open {
                self.features.find_focused = false;
                if let Some(page) = &self.tab().wega {
                    page.find_in_page(String::new(), false, false);
                }
            }
            self.features.find_open = open;
        }
    }
}
/// Stored permission keys (the stored policy uses these exact strings) with a
/// short label for the site panel and a phrase for the permission prompt.
pub(crate) const PERMISSIONS: [(&str, &str, &str); 13] = [
    ("Notifications", "Notifications", "show notifications"),
    ("Geolocation", "Location", "know your location"),
    ("Camera", "Camera", "use your camera"),
    ("Microphone", "Microphone", "use your microphone"),
    ("Push", "Push messages", "send you push messages"),
    ("Midi", "MIDI devices", "use your MIDI devices"),
    ("Speaker", "Audio output", "choose your audio output"),
    (
        "DeviceInfo",
        "Device information",
        "see information about your devices",
    ),
    (
        "BackgroundSync",
        "Background sync",
        "sync in the background",
    ),
    ("Bluetooth", "Bluetooth", "use Bluetooth"),
    (
        "PersistentStorage",
        "Persistent storage",
        "store data persistently",
    ),
    ("Gamepad", "Gamepads", "use your gamepads"),
    (
        "ScreenWakeLock(Screen)",
        "Screen wake lock",
        "keep your screen on",
    ),
];

/// What a site is asking for, in words, for a stored permission key.
pub(crate) fn permission_request(feature: &str) -> String {
    PERMISSIONS
        .iter()
        .find(|(key, _, _)| *key == feature)
        .map_or_else(
            || format!("use {feature}"),
            |(_, _, phrase)| (*phrase).to_owned(),
        )
}
