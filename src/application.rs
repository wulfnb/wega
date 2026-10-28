//! Application operations. No engine, window, filesystem or process types enter this layer.
use crate::{
    model::{Dock, Policy, Theme},
    profile::{Bookmark, Profile, SavedFavicon, TabLayout},
    storage::ProfileStore,
};

pub enum Command {
    Layout(TabLayout),
    Theme(Theme),
    Dock(Dock),
    SidebarWidth(f32),
    Policy(Policy),
    ToggleBookmark { title: String, url: String },
    RemoveBookmark(usize),
    ClearHistory,
    Visit(Bookmark),
    Favicon { url: String, image: SavedFavicon },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Changed,
    PolicyChanged,
    Unchanged,
}

/// Owns persisted state and talks to storage only through its injected port.
/// Even a permissive adapter cannot overwrite a profile after a failed load.
pub struct Application {
    profile: Profile,
    store: Box<dyn ProfileStore>,
    writable: bool,
}
impl Application {
    pub fn load(mut store: Box<dyn ProfileStore>) -> (Self, Option<String>) {
        let (mut profile, warning, writable) = match store.load() {
            Ok(profile) => (profile, None, true),
            Err(error) => (
                Profile::default(),
                Some(format!(
                    "{error}. Existing profile will not be overwritten."
                )),
                false,
            ),
        };
        if profile.policy.performance_visibility_version == 0 {
            profile.policy.show_performance = true;
            profile.policy.performance_visibility_version = 1;
        }
        (
            Self {
                profile,
                store,
                writable,
            },
            warning,
        )
    }
    pub fn profile(&self) -> &Profile {
        &self.profile
    }
    pub fn apply(&mut self, command: Command) -> Event {
        match command {
            Command::Layout(value) => self.profile.layout = value,
            Command::Theme(value) => self.profile.theme = value,
            Command::Dock(value) => self.profile.devtools_dock = value,
            Command::SidebarWidth(value) => {
                if !value.is_finite() {
                    return Event::Unchanged;
                }
                self.profile.sidebar_width = value.clamp(150.0, 500.0);
            }
            Command::Policy(value) => {
                self.profile.policy = value;
                return Event::PolicyChanged;
            }
            Command::ToggleBookmark { title, url } => self.profile.toggle_bookmark(title, url),
            Command::RemoveBookmark(index) => {
                if index >= self.profile.bookmarks.len() {
                    return Event::Unchanged;
                }
                self.profile.bookmarks.remove(index);
            }
            Command::ClearHistory => self.profile.history.clear(),
            Command::Visit(mut visit) => {
                if !["http://", "https://", "file://"]
                    .iter()
                    .any(|scheme| visit.url.starts_with(scheme))
                {
                    return Event::Unchanged;
                }
                visit.visit_count = self
                    .profile
                    .history
                    .iter()
                    .find(|entry| entry.url == visit.url)
                    .map_or(1, |entry| entry.visit_count.max(1).saturating_add(1));
                self.profile.history.retain(|entry| entry.url != visit.url);
                self.profile.history.insert(0, visit);
                self.profile.history.truncate(1000);
            }
            Command::Favicon { url, image } => {
                if !image.valid(32) {
                    return Event::Unchanged;
                }
                if self.profile.favicons.get(&url) == Some(&image) {
                    return Event::Unchanged;
                }
                if self.profile.favicons.len() >= 128 && !self.profile.favicons.contains_key(&url) {
                    self.profile.favicons.pop_first();
                }
                self.profile.favicons.insert(url, image);
            }
        }
        Event::Changed
    }
    pub fn start_download(
        &self,
        port: &mut impl crate::downloads::DownloadPort,
        address: &str,
        filename: &str,
        connections: usize,
        cookies: String,
    ) -> Result<(), String> {
        let url = crate::downloads::validate_url(address)?;
        if self.profile.policy.offline || !self.profile.policy.permits(&url) {
            return Err("This download is blocked by your offline/host settings.".into());
        }
        port.start(url.as_str(), filename, connections, cookies)
    }
    /// Preserve selection by identity when non-persistable data tabs are removed.
    pub fn save_session(&mut self, urls: Vec<String>, active: usize) -> Result<(), String> {
        self.save_tabs(
            urls.into_iter().map(|url| (url, String::new())).collect(),
            active,
        )
    }
    /// Persist the open tabs as (url, title) pairs. Titles let unloaded tabs show
    /// their page name after a restart without loading the page.
    pub fn save_tabs(&mut self, tabs: Vec<(String, String)>, active: usize) -> Result<(), String> {
        self.save_engine_tabs(
            tabs.into_iter()
                .map(|(url, title)| (url, title, crate::engine_choice::EngineKind::Servo))
                .collect(),
            active,
        )
    }
    pub fn save_engine_tabs(
        &mut self,
        tabs: Vec<(String, String, crate::engine_choice::EngineKind)>,
        active: usize,
    ) -> Result<(), String> {
        if !self.writable {
            return Err(
                "Profile has not loaded successfully; existing data will not be overwritten".into(),
            );
        }
        let selected = tabs
            .iter()
            .take(active)
            .filter(|(url, _, _)| !url.starts_with("data:"))
            .count();
        let mut new_engines = Vec::new();
        let (new_urls, new_titles): (Vec<String>, Vec<String>) = tabs
            .into_iter()
            .filter(|(url, _, _)| !url.starts_with("data:"))
            .map(|(url, title, engine)| {
                new_engines.push(engine);
                (url, title)
            })
            .unzip();
        let new_active = selected.min(new_urls.len().saturating_sub(1));
        let previous_urls = std::mem::replace(&mut self.profile.urls, new_urls);
        let previous_titles = std::mem::replace(&mut self.profile.titles, new_titles);
        let previous_active = std::mem::replace(&mut self.profile.active, new_active);
        let previous_engines = std::mem::replace(&mut self.profile.engines, new_engines);
        if let Err(error) = self.store.save(&self.profile) {
            self.profile.urls = previous_urls;
            self.profile.titles = previous_titles;
            self.profile.active = previous_active;
            self.profile.engines = previous_engines;
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "application_tests.rs"]
mod tests;
