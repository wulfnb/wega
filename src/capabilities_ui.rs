use crate::{
    browser::Browser,
    capabilities::{self, Decision},
};
impl Browser {
    pub fn update_documents(&self) {
        let mut docs = self.documents.write().unwrap();
        let palette = self.profile().theme.palette(
            self.window.theme() != Some(winit::window::Theme::Light),
            false,
        );
        let document = |title: &str, body: &str| capabilities::document(title, body, palette);
        let icon = |url: &str| match self.profile().favicons.get(url) {
            Some(crate::profile::SavedFavicon::Png(bytes)) => {
                use base64::Engine;
                format!(
                    "<img alt='' width='24' height='24' src='data:image/png;base64,{}'>",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                )
            }
            _ => "<span class='page-icon' aria-hidden='true'>▤</span>".into(),
        };
        let shortcuts = self
            .profile()
            .new_tab_shortcuts()
            .into_iter()
            .map(|entry| {
                format!(
                    "<a class='tile' href='{}'>{}<span>{}</span></a>",
                    capabilities::html(&entry.url),
                    icon(&entry.url),
                    capabilities::html(&entry.title)
                )
            })
            .collect::<String>();
        let recent = self
            .profile()
            .history
            .iter()
            .take(5)
            .map(|entry| {
                format!(
                    "<li>{}<a href='{}'>{}</a></li>",
                    icon(&entry.url),
                    capabilities::html(&entry.url),
                    capabilities::html(&entry.title)
                )
            })
            .collect::<String>();
        let policy = &self.profile().policy;
        let (action, field, fixed) = policy.search_form();
        let engine = match policy.search_engine {
            wega::model::SearchEngine::Custom => "the web".to_owned(),
            engine => engine.name().to_owned(),
        };
        let hidden = fixed
            .iter()
            .map(|(name, value)| {
                format!(
                    "<input type='hidden' name='{}' value='{}'>",
                    capabilities::html(name),
                    capabilities::html(value)
                )
            })
            .collect::<String>();
        docs.insert("newtab".into(), document("Welcome to Wega", &format!("<p>Search here or press <b>Cmd/Ctrl+L</b> to enter an address.</p><form action='{}' method='get'><label for='search'>Search the web</label><p class='search-controls'>{hidden}<input id='search' name='{}' type='search' placeholder='Search {}' required autofocus><button type='submit'>Search</button></p></form><h2>Your pages</h2><div class='tiles'>{shortcuts}</div><h2>Recent</h2><ul class='recent'>{recent}</ul>", capabilities::html(&action), capabilities::html(&field), capabilities::html(&engine))));
        if let Some(page) = docs.get_mut("newtab")
            && let (Some(start), Some(end)) = (page.find("<nav "), page.find("</nav>"))
        {
            page.replace_range(start..end + 6, "");
        }
        for (name, title, items) in [
            ("history", "History", &self.profile().history),
            ("bookmarks", "Bookmarks", &self.profile().bookmarks),
        ] {
            let list = items
                .iter()
                .map(|b| {
                    format!(
                        "<li><a href='{}'>{}</a></li>",
                        capabilities::html(&b.url),
                        capabilities::html(&b.title)
                    )
                })
                .collect::<String>();
            docs.insert(name.into(), document(title, &format!("<ul>{list}</ul>")));
        }
        docs.insert("about".into(), document("About Wega", "<p>Wega 0.1.0-alpha.1</p><p>Built with Rust, Servo 0.7 and egui. This is an alpha preview.</p>"));
        docs.insert("settings".into(),document("Browser settings","<p>Use the Browser settings window to manage appearance, browsing, privacy, downloads and advanced options. If it is closed, reopen it from <b>… → Browser settings</b>.</p><p>Internal documents have no access to native browser controls.</p>"));
    }
    pub fn screenshot_page(&mut self) {
        if self.is_wega() {
            self.notify(
                crate::ux::Severity::Info,
                "This page action requires Servo in this version. Use the engine selector below.",
                None,
            );
            return;
        }
        let folder = self.profile_dir.join("screenshots");
        if let Err(e) = std::fs::create_dir_all(&folder) {
            self.status = Some(e.to_string());
            return;
        }
        let path = folder.join(format!(
            "page-{}.png",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let events = self.delegate.clone();
        self.view()
            .take_screenshot(None, move |result| match result {
                Ok(image) => match image.save(&path) {
                    Ok(()) => events.notice(
                        crate::ux::Severity::Success,
                        format!("Saved screenshot: {}", path.display()),
                        Some(path),
                    ),
                    Err(error) => {
                        events.notice(crate::ux::Severity::Error, error.to_string(), None)
                    }
                },
                Err(error) => events.notice(
                    crate::ux::Severity::Error,
                    format!("Screenshot failed: {error:?}"),
                    None,
                ),
            });
        self.notify(
            crate::ux::Severity::Info,
            "Waiting for the page screenshot…",
            None,
        );
    }
    pub fn reload_without_cache(&mut self) {
        if self.is_wega() {
            self.notify(
                crate::ux::Severity::Info,
                "This page action requires Servo in this version. Use the engine selector below.",
                None,
            );
            return;
        }
        if let Some(url) = self.view().url() {
            let mut headers = http::HeaderMap::new();
            headers.insert(
                http::header::CACHE_CONTROL,
                http::HeaderValue::from_static("no-cache"),
            );
            headers.insert(
                http::header::PRAGMA,
                http::HeaderValue::from_static("no-cache"),
            );
            self.view()
                .load_request(servo::UrlRequest::new(url).headers(headers));
        }
    }
    pub fn refresh_site_data(&mut self) {
        if self.is_wega() {
            self.notify(
                crate::ux::Severity::Info,
                "This page action requires Servo in this version. Use the engine selector below.",
                None,
            );
            return;
        }
        self.features.sites = self
            .servo
            .site_data_manager()
            .site_data(servo::StorageType::all())
            .into_iter()
            .map(|s| {
                let kinds = [
                    (servo::StorageType::Cookies, "Cookies"),
                    (servo::StorageType::Local, "Local storage"),
                    (servo::StorageType::Session, "Session storage"),
                ]
                .into_iter()
                .filter_map(|(kind, label)| s.storage_types().contains(kind).then_some(label))
                .collect::<Vec<_>>()
                .join(", ");
                (s.name(), kinds)
            })
            .collect();
        self.features.sites.sort_by(|a, b| a.0.cmp(&b.0));
        self.features.cookies = self
            .view()
            .url()
            .map(|url| {
                self.servo
                    .site_data_manager()
                    .cookies_for_url(url, servo::CookieSource::HTTP)
                    .iter()
                    .map(|c| {
                        format!(
                            "{}={} · domain={} · path={}",
                            c.name(),
                            c.value(),
                            c.domain().unwrap_or(""),
                            c.path().unwrap_or("/")
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
    pub fn feature_ui(&mut self, ctx: &egui::Context) {
        if self.is_wega() {
            if self.features.settings || self.features.site_info || self.features.find_open {
                let mut open = true;
                let mut servo = false;
                egui::Window::new("Wega experimental engine").open(&mut open).show(ctx,|ui|{
                    ui.label("This engine currently supports article rendering and basic JavaScript. Site-data inspection, page find, forms, media, accessibility content and page tools require Servo.");
                    servo=ui.button("Reload this page in Servo").clicked();
                    ui.label("Website cookies and sign-in state are separate. Engine selection is remembered per open tab.");
                });
                if !open {
                    self.features.settings = false;
                    self.features.site_info = false;
                    self.features.find_open = false;
                }
                if servo {
                    self.switch_engine(wega::engine_choice::EngineKind::Servo);
                }
            }
            return;
        }

        if !self.dialogs.prompts.is_empty() {
            let fresh = self.prepare_dialog(ctx, "engine-confirmation");
            let mut answer = if crate::ux::escape(ctx) {
                Some(false)
            } else {
                None
            };
            let prompt = self.dialogs.prompts.front().unwrap();
            egui::Modal::new(egui::Id::unique("engine-confirmation")).show(ctx, |ui| {
                ui.set_max_width(480.0);
                ui.heading(&prompt.title);
                ui.label(&prompt.detail);
                ui.horizontal(|ui| {
                    let cancel_button = ui.button("Cancel");
                    if fresh {
                        cancel_button.request_focus();
                    }
                    if cancel_button.clicked() {
                        answer = Some(false);
                    }
                    if ui.button("Continue").clicked() {
                        answer = Some(true);
                    }
                });
            });
            if let Some(allow) = answer {
                let prompt = self.dialogs.prompts.pop_front().unwrap();
                self.finish_dialog(ctx);
                match prompt.decision {
                    Decision::Servo(request) => {
                        if allow {
                            request.allow()
                        } else {
                            request.deny()
                        }
                    }
                    Decision::Protocol(request, registration) => {
                        if allow {
                            match registration.register_or_unregister {
                                servo::RegisterOrUnregister::Register => {
                                    self.delegate
                                        .handlers
                                        .borrow_mut()
                                        .insert(registration.scheme, registration.url);
                                }
                                servo::RegisterOrUnregister::Unregister => {
                                    self.delegate
                                        .handlers
                                        .borrow_mut()
                                        .remove(&registration.scheme);
                                }
                            }
                            request.allow();
                        } else {
                            request.deny();
                        }
                    }
                    Decision::External(url) if allow => {
                        if let Err(error) = crate::ux::open_external(&url) {
                            self.notify(crate::ux::Severity::Error, error, None);
                        }
                    }
                    _ => {}
                }
            }
            return;
        }
        if !self.features.bluetooth.is_empty() {
            let fresh = self.prepare_dialog(ctx, "bluetooth");
            let devices = self.features.bluetooth.front().unwrap().devices().clone();
            let mut selected = None;
            let mut cancel = crate::ux::escape(ctx);
            egui::Modal::new(egui::Id::unique("bluetooth-device")).show(ctx, |ui| {
                ui.heading("Choose a Bluetooth device");
                if devices.is_empty() {
                    ui.label("No matching devices found.");
                }
                for (i, device) in devices.iter().enumerate() {
                    if ui
                        .button(format!("{} ({})", device.name, device.address))
                        .clicked()
                    {
                        selected = Some(i);
                    }
                }
                let button = ui.button("Cancel");
                if fresh {
                    button.request_focus();
                }
                cancel |= button.clicked();
            });
            if let Some(i) = selected {
                let _ = self
                    .features
                    .bluetooth
                    .pop_front()
                    .unwrap()
                    .pick_device(&devices[i]);
            } else if cancel {
                let _ = self.features.bluetooth.pop_front().unwrap().cancel();
            }
            return;
        }
        if self.features.settings {
            let mut open = true;
            let fresh = self.prepare_dialog(ctx, "settings");
            let response = egui::Modal::new(egui::Id::unique("browser-settings")).show(ctx,|ui|{
                ui.set_width(620.0_f32.min((ctx.content_rect().width()-64.0).max(240.0)));
                ui.horizontal(|ui| {
                    ui.heading("Settings");
                    let close = ui.button("Close");
                    if fresh { close.request_focus(); }
                    if close.clicked() { open = false; }
                });
                ui.horizontal_wrapped(|ui|{
                    for (i,label) in ["Appearance","Browsing","Privacy","Downloads","Advanced"].iter().enumerate() {
                        ui.selectable_value(&mut self.features.settings_section,i,*label);
                    }
                });
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
                    ui.separator();ui.heading("Browsing data");
                    if ui.button("Review cookies and stored sites").clicked(){self.refresh_site_data();self.features.site_info=true;open=false;}
                    if ui.button("Clear browsing data…").clicked(){self.features.clear_data=true;open=false;}
                    if ui.button("Clear session cookies").clicked(){self.servo.site_data_manager().clear_session_cookies(None);self.notify(crate::ux::Severity::Success,"Session cookies cleared.",None);}

                        }
                        3 => {
                            ui.heading("Downloads");
                            ui.label(format!("Save folder: {}",self.downloads.directory.display()));
                            if ui.button("Open download manager").clicked(){self.download_ui.open=true;open=false;}
                            ui.weak("Open completed files or show them in their folder from Downloads. In Docker, use the viewer's Downloads button to save to your computer.");
                        }
                        _ => {
                    ui.separator(); ui.heading("User script");
                    ui.label("Runs in every subsequently loaded page. Changes apply after reload.");
                    ui.add(egui::TextEdit::multiline(&mut self.features.script_source).code_editor().desired_rows(7).desired_width(f32::INFINITY));
                    if ui.button("Apply script").clicked(){let mut policy=self.profile().policy.clone();policy.user_script=self.features.script_source.clone();self.apply(wega::application::Command::Policy(policy));
                        if let Some(script)=self.features.script.take(){self.content_manager.get().unwrap().remove_script(script);}
                        if !self.profile().policy.user_script.trim().is_empty(){let script=std::rc::Rc::new(servo::UserScript::new(self.profile().policy.user_script.clone(),None));self.content_manager.get().unwrap().add_script(script.clone());self.features.script=Some(script);}
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
        if self.features.diagnostics {
            let mut open = true;
            egui::Window::new("Developer rendering diagnostics")
                .open(&mut open)
                .default_width(520.0)
                .show(ctx, |ui| {
                    ui.separator();
                    ui.heading("Rendering diagnostics");
                    ui.horizontal(|ui| {
                        for (name, option) in [
                            ("Profiler", servo::WebRenderDebugOption::Profiler),
                            (
                                "Texture cache",
                                servo::WebRenderDebugOption::TextureCacheDebug,
                            ),
                            (
                                "Render targets",
                                servo::WebRenderDebugOption::RenderTargetDebug,
                            ),
                        ] {
                            if ui.button(name).clicked() {
                                self.view().toggle_webrender_debugging(option);
                            }
                        }
                    });
                    if ui.button("Capture WebRender state").clicked() {
                        self.view().capture_webrender();
                        self.notify(
                            crate::ux::Severity::Info,
                            "WebRender capture requested; see engine log for output location.",
                            None,
                        );
                    }
                    if ui.button("Clear this tab’s back/forward history").clicked() {
                        self.view().clear_session_history();
                    }
                });
            self.features.diagnostics = open;
        }
        if self.features.site_info {
            let mut open = true;
            let anchor = ctx
                .data(|data| data.get_temp::<egui::Pos2>(egui::Id::unique("site-panel-anchor")))
                .unwrap_or(egui::pos2(12.0, 60.0));
            let width = 620.0_f32.min((ctx.content_rect().width() - 32.0).max(240.0));
            egui::Window::new("Site data")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .fixed_pos(egui::pos2(
                    anchor
                        .x
                        .min(ctx.content_rect().right() - width - 16.0)
                        .max(8.0),
                    anchor.y + 6.0,
                ))
                .default_width(width)
                .frame(
                    egui::Frame::window(&ctx.style_of(if crate::theme::palette(ctx).dark {
                        egui::Theme::Dark
                    } else {
                        egui::Theme::Light
                    }))
                    .inner_margin(16),
                )
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().id_salt("site-panel-content").max_height((ctx.content_rect().height()-anchor.y-100.0).max(120.0)).show(ui,|ui| {
                    ui.label(self.tab().url());
                    let current = self.view().url();
                    ui.label(match current.as_ref().map(|url| url.scheme()) {
                        Some("https") => {
                            "Encrypted connection. The certificate is validated by the browser."
                        }
                        Some("http") => "Not secure: this connection is not encrypted.",
                        _ => "Local or internal document.",
                    });
                    if let Some(origin) = current.as_ref().filter(|url| matches!(url.scheme(),"http"|"https")).map(|url| url.origin().ascii_serialization()) {
                        ui.separator();
                        ui.collapsing("Site permissions",|ui| {
                        ui.weak("Allow lasts for the requesting page. Blocking denies queued and new permission prompts; it does not revoke access already granted. Restart Wega to end active device access. Remembered access is unavailable in this version.");
                        for (feature,label) in [("Notifications","Notifications"),("Geolocation","Location"),("Camera","Camera"),("Microphone","Microphone"),("Push","Push messages"),("Midi","MIDI devices"),("Speaker","Audio output"),("DeviceInfo","Device information"),("BackgroundSync","Background sync"),("Bluetooth","Bluetooth"),("PersistentStorage","Persistent storage"),("Gamepad","Gamepads"),("ScreenWakeLock(Screen)","Screen wake lock")] {
                            let mut blocked = self.profile().policy.permission_blocked(&origin,feature);
                            if ui.checkbox(&mut blocked,format!("Block {label}")).changed() {
                                let mut policy=self.profile().policy.clone();policy.set_permission_blocked(&origin,feature,blocked);
                                self.apply(wega::application::Command::Policy(policy));
                                if blocked {self.dialogs.permissions.resolve(&origin,feature,false);}
                            }
                        }
                        });
                        ui.separator();
                    }
                    // Servo groups storage by registered site, not URL origin.
                    let site = current
                        .as_ref()
                        .filter(|url| matches!(url.scheme(), "http" | "https"))
                        .and_then(|url| url.host_str())
                        .and_then(|host| {
                            self.features
                                .sites
                                .iter()
                                .find(|(site, _)| {
                                    host == site || host.ends_with(&format!(".{site}"))
                                })
                                .map(|(site, _)| site.clone())
                        });
                    if let Some(site) = site
                        && ui
                            .button(
                                egui::RichText::new("Clear data for this site…")
                                    .color(crate::theme::palette(ctx).danger),
                            )
                            .clicked()
                    {
                        self.features.delete_site = Some(site);
                    }
                    if ui.button("Refresh").clicked() {
                        self.refresh_site_data();
                    }
                    egui::ScrollArea::vertical()
                        .max_height(430.0)
                        .show(ui, |ui| {
                            ui.heading("Cookies on this site");
                            for cookie in &self.features.cookies {
                                ui.add(egui::Label::new(cookie).wrap());
                            }
                            if self.features.cookies.is_empty() {
                                ui.weak("No cookies for this URL.");
                            }
                            ui.separator();
                            ui.heading("Stored sites");
                            let mut remove = None;
                            for (site, kinds) in &self.features.sites {
                                ui.horizontal(|ui| {
                                    ui.label(site);
                                    ui.weak(kinds);
                                    if ui
                                        .button(
                                            egui::RichText::new("Remove")
                                                .color(crate::theme::palette(ctx).danger),
                                        )
                                        .clicked()
                                    {
                                        remove = Some(site.clone());
                                    }
                                });
                            }
                            if let Some(site) = remove {
                                self.features.delete_site = Some(site);
                            }
                        });
                    });
                });
            self.features.site_info = open && !crate::ux::escape(ctx);
        }
        if let Some(site) = self.features.delete_site.clone() {
            let fresh = self.prepare_dialog(ctx, "delete-site-data");
            let cancel = crate::ux::escape(ctx);
            egui::Modal::new(egui::Id::unique("delete-site-data")).show(ctx,|ui|{
                ui.heading("Delete this site's data?");
                ui.label(&site);
                ui.label("Cookies and offline storage for this site will be removed. You may be signed out. This cannot be undone.");
                ui.horizontal(|ui|{
                    let button=ui.button("Cancel");if fresh {button.request_focus();}
                    if button.clicked() || cancel {self.features.delete_site=None;}
                    if ui.button(egui::RichText::new("Delete site data").color(crate::theme::palette(ctx).danger)).clicked(){
                        self.servo.site_data_manager().clear_site_data(&[&site],servo::StorageType::all());
                        self.features.delete_site=None;self.refresh_site_data();self.notify(crate::ux::Severity::Success,"Site data deleted.",None);
                    }
                });
            });
            return;
        }
        if self.features.clear_data {
            let fresh = self.prepare_dialog(ctx, "clear-data");
            let cancel = crate::ux::escape(ctx);
            egui::Modal::new(egui::Id::unique("clear-data")).show(ctx,|ui|{
                ui.heading("Clear browsing data?");ui.label("Deletes cookies, local/session storage, HTTP cache and browsing history. Open sites may sign you out. Bookmarks remain.");
                ui.horizontal(|ui|{let button=ui.button("Cancel");if fresh {button.request_focus();}
if button.clicked() || cancel {self.features.clear_data=false;}
                if ui.button("Clear data").clicked(){
                    let sites=self.servo.site_data_manager().site_data(servo::StorageType::all());let names:Vec<_>=sites.iter().map(|s|s.name()).collect();let names:Vec<_>=names.iter().map(String::as_str).collect();
                    self.servo.site_data_manager().clear_site_data(&names,servo::StorageType::all());self.servo.site_data_manager().clear_cookies(None);self.servo.network_manager().clear_cache();self.apply(wega::application::Command::ClearHistory);self.features.cookies.clear();self.features.sites.clear();self.features.clear_data=false;self.changed();self.notify(crate::ux::Severity::Success,"Browsing data cleared.",None);
                }});
            });
        }
        if self.features.find_open {
            let mut open = true;
            egui::Window::new("Find in page")
                .open(&mut open)
                .anchor(egui::Align2::RIGHT_TOP, [-24.0, 120.0])
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let response = ui.text_edit_singleline(&mut self.features.find);
                        let enter =
                            response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        for (label, backward) in [("Previous", true), ("Next", false)] {
                            if ui.button(label).clicked() || (enter && !backward) {
                                let text = serde_json::to_string(&self.features.find).unwrap();
                                let source = include_str!("page_actions.js")
                                    .replace("BACKWARDS", if backward { "true" } else { "false" })
                                    .replace("QUERY", &text);
                                let delegate = self.delegate.clone();
                                self.view().evaluate_javascript(source, move |result| {
                                    if let Ok(servo::JSValue::String(text)) = result {
                                        delegate.notice(crate::ux::Severity::Info, text, None);
                                    }
                                });
                            }
                        }
                    });
                });
            self.features.find_open = open;
        }
    }
}
