//! Containers are isolated windows for both engines. A window's identity never
//! changes in place, so popups, navigation, downloads and engine fallback inherit
//! the same profile without copying cookies or losing the source page's state.
use crate::browser::Browser;
use egui::{Color32, RichText};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use wega::containers::{Color, Container, Registry};

pub(crate) struct State {
    pub root: PathBuf,
    pub current: Option<Container>,
    registry: Registry,
    open: bool,
    editing: Option<String>,
    name: String,
    color: Color,
    error: Option<String>,
    remove: Option<String>,
}
impl State {
    pub fn new(options: &wega::options::Options, profile: &Path) -> Result<Self, String> {
        let root = options
            .container_root
            .clone()
            .unwrap_or_else(|| profile.into());
        // A broken registry must not stop normal browsing or be replaced by
        // defaults. Its error disables launching/editing until it is repaired.
        let (registry, error) = match Registry::load(&root) {
            Ok(registry) => (registry, None),
            Err(error) if options.container.is_none() => (Registry::default(), Some(error)),
            Err(error) => return Err(error),
        };
        let current = options
            .container
            .as_deref()
            .map(|id| registry.find(id).cloned())
            .transpose()?;
        Ok(Self {
            root,
            current,
            registry,
            open: false,
            editing: None,
            name: String::new(),
            color: Color::Blue,
            error,
            remove: None,
        })
    }
    fn refresh(&mut self) {
        match Registry::load(&self.root) {
            Ok(registry) => {
                if let Some(current) = &mut self.current
                    && let Ok(updated) = registry.find(&current.id)
                {
                    *current = updated.clone();
                }
                self.registry = registry;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}
fn colored(container: &Container) -> (egui::Atom<'static>, RichText) {
    let [r, g, b] = container.color.rgb();
    let color = Color32::from_rgb(r, g, b);
    (
        crate::ui::icon_atom("container-dot", Some(color)),
        RichText::new(&container.name).color(color),
    )
}
impl Browser {
    pub(crate) fn containers_menu(&mut self, ui: &mut egui::Ui) {
        let label = self
            .containers
            .current
            .as_ref()
            .map(|container| {
                let mut label = container.clone();
                if label.name.chars().count() > 18 {
                    label.name = format!("{}…", label.name.chars().take(17).collect::<String>());
                }
                colored(&label)
            })
            .unwrap_or_else(|| {
                (
                    crate::ui::icon_atom("containers", None),
                    RichText::new("Containers"),
                )
            });
        ui.menu_button(label, |ui| {
            // Load only while this popup is open; other windows may have renamed
            // an identity. Never read JSON on every closed-menu redraw.
            self.containers.refresh();
            ui.label("Separate accounts in container windows");
            if let Some(error) = &self.containers.error { ui.colored_label(crate::theme::palette(ui.ctx()).amber, error); }
            let enabled = self.containers.error.is_none();
            let containers = self.containers.registry.containers.clone();
            ui.add_enabled_ui(enabled, |ui| {
                for container in &containers {
                    if ui.button(colored(container)).on_hover_text("Open or focus this container window").clicked() {
                        self.open_container(&container.id, None);
                        ui.close();
                    }
                }
                let address = self.current_url().filter(|url| matches!(url.scheme(), "http" | "https" | "file"));
                ui.add_enabled_ui(address.is_some(), |ui| {
                    ui.menu_button("Open copy of this page in…", |ui| {
                        for container in &containers {
                            if ui.button(colored(container)).clicked() {
                                self.open_container(&container.id, address.as_ref().map(ToString::to_string));
                                ui.close();
                            }
                        }
                    });
                });
            });
            ui.separator();
            if ui.button("Manage containers…").clicked() { self.containers.open = true; self.containers.refresh(); ui.close(); }
        }).response.on_hover_text("Containers keep accounts and website data separate. Every tab in a container window shares its identity.");
    }
    pub(crate) fn open_container(&mut self, id: &str, address: Option<String>) {
        let result = (|| -> Result<(), String> {
            let registry = Registry::load(&self.containers.root)?;
            let container = registry.find(id)?;
            let directory = wega::containers::profile_directory(&self.containers.root, id)?;
            if let Some(address) = &address
                && wega::options::external_launch_urls(std::slice::from_ref(address)).len() != 1
            {
                return Err("Only web or file pages can open in another container".into());
            }
            let mut log_options = std::fs::OpenOptions::new();
            log_options.create(true).append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                log_options.mode(0o600);
            }
            let log = log_options
                .open(directory.join("container-launch.log"))
                .map_err(|e| e.to_string())?;
            let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
            command
                .arg("--profile-dir")
                .arg(&self.containers.root)
                .arg("--container")
                .arg(id)
                .env_remove("GST_REGISTRY_1_0")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log);
            if let Some(address) = address {
                command.arg("--").arg(address);
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut child = command
                .spawn()
                .map_err(|e| format!("Cannot open container: {e}"))?;
            let name = container.name.clone();
            let proxy = self.event_proxy.clone();
            std::thread::spawn(move || match child.wait() {
                Ok(status) if status.success() => {}
                _ => {
                    let _ = proxy.send_event(crate::engine::Wake::ContainerError(format!(
                        "Could not open {name}. See {}. Existing accounts remain unchanged.",
                        directory.join("container-launch.log").display()
                    )));
                }
            });
            Ok(())
        })();
        if let Err(error) = result {
            self.notify(crate::ux::Severity::Error, error, None);
        }
    }
    pub(crate) fn containers_manager(&mut self, ctx: &egui::Context) {
        if !self.containers.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Containers").id(egui::Id::unique("containers-manager")).open(&mut open).collapsible(false).resizable(false).default_width(440.0).show(ctx, |ui| {
            ui.label("Keep separate sign-ins in Personal, Work, Banking or Shopping. Every tab in a container window uses the same account space.");
            ui.add_space(8.0);
            let rows = self.containers.registry.containers.clone();
            for container in rows {
                ui.horizontal(|ui| {
                    let label = egui::WidgetAtom::new(colored(&container)).show(ui).response;
                    label.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), &container.name));
                    if ui.small_button("Open").clicked() { self.open_container(&container.id, None); }
                    if ui.small_button("Edit").clicked() { self.containers.editing = Some(container.id.clone()); self.containers.name = container.name.clone(); self.containers.color = container.color; }
                    if ui.small_button("Remove…").clicked() { self.containers.remove = Some(container.id.clone()); }
                });
                if self.containers.remove.as_deref() == Some(&container.id) {
                    ui.label("Remove from the menu? Saved website data will be kept on disk.");
                    ui.horizontal(|ui| {
                        if ui.button("Remove from menu").clicked() {
                            match Registry::remove(&self.containers.root, &container.id) {
                                Ok(registry) => { self.containers.registry = registry; self.containers.remove = None; self.containers.error = None; },
                                Err(error) => self.containers.error = Some(error),
                            }
                        }
                        if ui.button("Cancel").clicked() { self.containers.remove = None; }
                    });
                }
            }
            ui.separator();
            ui.label(if self.containers.editing.is_some() { "Edit container" } else { "New container" });
            ui.horizontal(|ui| {
                ui.label("Name"); ui.text_edit_singleline(&mut self.containers.name);
            });
            egui::ComboBox::from_id_salt("container-color").selected_text(self.containers.color.name()).show_ui(ui, |ui| {
                for color in Color::ALL { ui.selectable_value(&mut self.containers.color, color, color.name()); }
            });
            ui.horizontal(|ui| {
                if ui.button(if self.containers.editing.is_some() { "Save changes" } else { "Add container" }).clicked() {
                    let result = (|| {
                        let id = match &self.containers.editing { Some(id) => id.clone(), None => format!("c-{}", wega::containers::random_id()?) };
                        Registry::update(&self.containers.root, Container { id, name: self.containers.name.trim().to_owned(), color: self.containers.color })
                    })();
                    match result {
                        Ok(registry) => { self.containers.registry = registry; self.containers.refresh(); self.containers.editing = None; self.containers.name.clear(); },
                        Err(error) => self.containers.error = Some(error),
                    }
                }
                if self.containers.editing.is_some() && ui.button("Cancel edit").clicked() { self.containers.editing = None; self.containers.name.clear(); }
                if ui.button("Refresh").clicked() { self.containers.refresh(); }
            });
            if let Some(error) = &self.containers.error { ui.colored_label(crate::theme::palette(ui.ctx()).amber, error); }
            ui.small("Containers also keep history, bookmarks, permissions and downloads separate. They share your network address. Mozilla VPN and Firefox Sync are not connected.");
        });
        self.containers.open = open;
    }

    #[cfg(feature = "test-harness")]
    pub(crate) fn check_containers_ui_contract(&mut self) {
        let view = self.view().id();
        let address = self.tab().url();
        self.coverage_click(&["Containers", "Manage containers…"], |browser, ui| {
            browser.ui(ui)
        });
        assert!(self.containers.open);
        self.containers.name = "Native test account".into();
        self.containers.color = Color::Purple;
        self.coverage_click(&["Add container"], |browser, ui| browser.ui(ui));
        let custom = Registry::load(&self.containers.root)
            .unwrap()
            .containers
            .into_iter()
            .find(|c| c.name == "Native test account")
            .expect("Native manager must save the identity");
        let directory =
            wega::containers::profile_directory(&self.containers.root, &custom.id).unwrap();
        std::fs::write(directory.join("retained-data"), b"account data").unwrap();
        self.containers.editing = Some(custom.id.clone());
        self.containers.name = "Renamed native account".into();
        self.containers.color = Color::Green;
        self.coverage_click(&["Save changes"], |browser, ui| browser.ui(ui));
        let registry = Registry::load(&self.containers.root).unwrap();
        assert_eq!(
            registry.find(&custom.id).unwrap().name,
            "Renamed native account"
        );
        assert_eq!(registry.find(&custom.id).unwrap().color, Color::Green);
        self.containers.remove = Some(custom.id.clone());
        self.coverage_click(&["Remove from menu"], |browser, ui| browser.ui(ui));
        assert!(
            Registry::load(&self.containers.root)
                .unwrap()
                .find(&custom.id)
                .is_err()
        );
        assert_eq!(
            std::fs::read(directory.join("retained-data")).unwrap(),
            b"account data"
        );
        self.containers.open = false;
        assert_eq!(
            self.view().id(),
            view,
            "Managing accounts must preserve the live source tab"
        );
        assert_eq!(self.tab().url(), address);
        println!(
            "CONTAINERS UI PASSED: default menu, create, rename/color, remove with retained data and source-tab preservation"
        );
    }
}
