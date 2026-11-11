//! Navigation commands shared by desktop UI, shortcuts and automation.
use crate::profile::resolve_address;

#[derive(Clone)]
pub enum Command {
    Load(String),
    Reload,
    ReloadBypassCache,
    Traverse { forward: bool },
    Zoom { factor: f32 },
    ResetZoom,
}
/// The application knows navigation intent; the adapter owns engine handles.
pub trait NavigationPort {
    fn load(&mut self, url: url::Url);
    fn reload(&mut self);
    fn traverse(&mut self, forward: bool) -> bool;
    fn zoom(&self) -> f32;
    fn set_zoom(&mut self, value: f32);
    fn reload_bypass_cache(&mut self) {
        self.reload();
    }
    fn clear_history(&mut self) {}
}
/// False means no operation was dispatched (e.g. no forward history).
pub fn dispatch(command: Command, port: &mut impl NavigationPort) -> Result<bool, String> {
    match command {
        Command::Load(address) => port.load(resolve_address(&address)?),
        Command::Reload => port.reload(),
        Command::ReloadBypassCache => port.reload_bypass_cache(),
        Command::Traverse { forward } => return Ok(port.traverse(forward)),
        Command::Zoom { factor } => {
            if !factor.is_finite() || factor <= 0.0 {
                return Err("Invalid zoom factor".into());
            }
            port.set_zoom((port.zoom() * factor).clamp(0.3, 5.0));
        }
        Command::ResetZoom => port.set_zoom(1.0),
    }
    Ok(true)
}

/// Decision for a navigation requested by page content (links, scripts, forms).
/// Embedder loads — address bar, bookmarks, history, command line, session restore —
/// never ask, so local files and data documents stay reachable for the user.
/// Like Chromium and Firefox, web content may not open local files or top-level
/// `data:` documents; local and internal pages may still link to local files.
/// Untrusted pages cannot navigate to internal `wega://` chrome pages.
pub fn content_navigation_allowed(target: &url::Url, current: Option<&url::Url>) -> bool {
    let from = current.map(url::Url::scheme).unwrap_or_default();
    match target.scheme() {
        "file" => matches!(from, "file" | "wega"),
        "data" => from == "wega",
        "wega" => from == "wega",
        _ => true,
    }
}

/// Completion is evaluated against the engine snapshot after its event batch.
/// An old completion or unrelated history callback cannot finish a new load.
pub struct Completion {
    expected: Option<String>,
    initial: String,
    started: bool,
    traversed: bool,
}
impl Completion {
    pub fn new(expected: Option<String>, initial: String) -> Self {
        Self {
            expected,
            initial,
            started: false,
            traversed: false,
        }
    }
    pub fn started(&mut self) {
        self.started = true;
    }
    pub fn traversed(&mut self) {
        self.traversed = true;
    }
    pub fn ready(&self, current: &str, complete: bool) -> bool {
        if !complete || !(self.started || self.traversed) {
            return false;
        }
        match &self.expected {
            Some(expected) => current == expected || (self.started && current != self.initial),
            None => true,
        }
    }
    pub fn update(&mut self, loading: bool, current: &str) -> bool {
        if loading {
            self.started = true;
            return false;
        }
        self.ready(current, true)
    }
}

/// Snapshot of the engine's history, used to correlate traversal destinations.
#[derive(Clone, Debug, Default)]
pub struct History {
    urls: Vec<String>,
    active: usize,
}
impl History {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn update(&mut self, urls: Vec<String>, active: usize) {
        self.active = active.min(urls.len().saturating_sub(1));
        self.urls = urls;
    }
    pub fn push(&mut self, url: String) {
        if self.urls.get(self.active).map(String::as_str) == Some(&url) {
            return;
        }
        if !self.urls.is_empty() {
            self.urls.truncate(self.active + 1);
        }
        self.urls.push(url);
        self.active = self.urls.len() - 1;
    }
    pub fn can_back(&self) -> bool {
        self.active > 0
    }
    pub fn can_forward(&self) -> bool {
        self.active + 1 < self.urls.len()
    }
    pub fn traverse(&mut self, forward: bool) -> Option<&str> {
        let index = if forward {
            self.active.checked_add(1)?
        } else {
            self.active.checked_sub(1)?
        };
        if index < self.urls.len() {
            self.active = index;
            Some(&self.urls[self.active])
        } else {
            None
        }
    }
    pub fn target(&self, forward: bool) -> Option<&str> {
        let index = if forward {
            self.active.checked_add(1)?
        } else {
            self.active.checked_sub(1)?
        };
        self.urls.get(index).map(String::as_str)
    }
    pub fn clear(&mut self) {
        let current = self.urls.get(self.active).cloned();
        self.urls.clear();
        self.active = 0;
        if let Some(url) = current {
            self.urls.push(url);
        }
    }
}

#[cfg(test)]
#[path = "navigation_tests.rs"]
mod tests;
