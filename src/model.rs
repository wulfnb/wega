//! Engine-independent persisted settings.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Glass,
    GlassLight,
    GlassDark,
    Midnight,
    Paper,
    #[default]
    #[serde(alias = "Purple")]
    Emerald,
}
impl Theme {
    pub const ALL: [Self; 6] = [
        Self::Glass,
        Self::GlassLight,
        Self::GlassDark,
        Self::Midnight,
        Self::Paper,
        Self::Emerald,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Glass => "Auto",
            Self::GlassLight => "Glass Light",
            Self::GlassDark => "Glass Dark",
            Self::Midnight => "Midnight",
            Self::Paper => "Paper",
            Self::Emerald => "Emerald",
        }
    }
    pub fn glass(self) -> bool {
        matches!(self, Self::Glass | Self::GlassLight | Self::GlassDark)
    }
    pub fn dark(self, system_dark: bool) -> bool {
        match self {
            Self::Glass => system_dark,
            Self::GlassLight | Self::Paper => false,
            _ => true,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Dock {
    #[default]
    Bottom,
    Left,
    Right,
    Window,
}
impl Dock {
    pub const ALL: [(Self, &'static str); 4] = [
        (Self::Bottom, "Dock to bottom"),
        (Self::Right, "Dock to right"),
        (Self::Left, "Dock to left"),
        (Self::Window, "Separate window"),
    ];
}
/// Search provider used for address-bar queries and the new-tab search form.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchEngine {
    #[default]
    Google,
    DuckDuckGo,
    Bing,
    Startpage,
    Brave,
    /// Uses `Policy::search_template`, an http(s) URL containing `%s`.
    Custom,
}
impl SearchEngine {
    pub const ALL: [Self; 6] = [
        Self::Google,
        Self::DuckDuckGo,
        Self::Bing,
        Self::Startpage,
        Self::Brave,
        Self::Custom,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Bing => "Bing",
            Self::Startpage => "Startpage",
            Self::Brave => "Brave Search",
            Self::Custom => "Custom",
        }
    }
    /// Built-in query templates; `%s` is replaced by the form-encoded query.
    pub fn template(self) -> Option<&'static str> {
        match self {
            Self::Google => Some("https://www.google.com/search?q=%s"),
            Self::DuckDuckGo => Some("https://duckduckgo.com/?q=%s"),
            Self::Bing => Some("https://www.bing.com/search?q=%s"),
            Self::Startpage => Some("https://www.startpage.com/do/search?query=%s"),
            Self::Brave => Some("https://search.brave.com/search?q=%s"),
            Self::Custom => None,
        }
    }
}

/// A custom template must be an absolute http(s) URL with exactly one `%s`.
pub fn valid_search_template(template: &str) -> bool {
    let template = template.trim();
    template.matches("%s").count() == 1
        && url::Url::parse(&template.replace("%s", "query"))
            .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub blocked_hosts: String,
    pub allowed_hosts: String,
    pub offline: bool,
    pub allow_window_changes: bool,
    pub show_performance: bool,
    /// Exact canonical HTTP(S) origins; no wildcard or credential/path matching.
    pub site_engines: std::collections::BTreeMap<String, crate::engine_choice::EngineKind>,
    pub wega_rollout: bool,
    pub wega_fallback: bool,
    pub blocked_site_permissions:
        std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    pub user_script: String,
    pub search_engine: SearchEngine,
    pub search_template: String,
    /// Unload background tabs after this many idle minutes; 0 keeps them loaded.
    pub discard_after_minutes: u32,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            blocked_hosts: String::new(),
            allowed_hosts: String::new(),
            offline: false,
            allow_window_changes: false,
            show_performance: false,
            site_engines: Default::default(),
            wega_rollout: true,
            wega_fallback: true,
            blocked_site_permissions: Default::default(),
            user_script: String::new(),
            search_engine: SearchEngine::default(),
            search_template: String::new(),
            discard_after_minutes: crate::lifecycle::DEFAULT_DISCARD_MINUTES,
        }
    }
}
impl Policy {
    /// Blocks apply to one canonical top-level web origin, including its frames.
    pub fn permission_blocked(&self, origin: &str, feature: &str) -> bool {
        let Ok(url) = url::Url::parse(origin) else {
            return false;
        };
        matches!(url.scheme(), "http" | "https")
            && url.origin().ascii_serialization() == origin
            && self
                .blocked_site_permissions
                .get(origin)
                .is_some_and(|features| features.contains(feature))
    }
    pub fn set_permission_blocked(&mut self, origin: &str, feature: &str, blocked: bool) {
        let Ok(url) = url::Url::parse(origin) else {
            return;
        };
        if !matches!(url.scheme(), "http" | "https") || url.origin().ascii_serialization() != origin
        {
            return;
        }
        if blocked {
            if self.blocked_site_permissions.len() >= 512
                && !self.blocked_site_permissions.contains_key(origin)
            {
                return;
            }
            self.blocked_site_permissions
                .entry(origin.into())
                .or_default()
                .insert(feature.into());
        } else if let Some(features) = self.blocked_site_permissions.get_mut(origin) {
            features.remove(feature);
            if features.is_empty() {
                self.blocked_site_permissions.remove(origin);
            }
        }
    }
    /// The effective template. An invalid custom template falls back to the default engine.
    pub fn search_template(&self) -> &str {
        match self.search_engine.template() {
            Some(template) => template,
            None if valid_search_template(&self.search_template) => self.search_template.trim(),
            None => SearchEngine::default().template().unwrap_or_default(),
        }
    }
    pub fn search_url(&self, query: &str) -> url::Url {
        // `%20` instead of `+` keeps spaces correct when `%s` sits in a path segment.
        let encoded = url::form_urlencoded::byte_serialize(query.as_bytes())
            .collect::<String>()
            .replace('+', "%20");
        url::Url::parse(&self.search_template().replace("%s", &encoded))
            .expect("validated search template")
    }
    /// GET form target for internal search pages: action, query field and fixed fields.
    pub fn search_form(&self) -> (String, String, Vec<(String, String)>) {
        const MARKER: &str = "wegasearchquerymarker";
        let mut url = url::Url::parse(&self.search_template().replace("%s", MARKER))
            .expect("validated search template");
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        let field = pairs
            .iter()
            .find(|(_, value)| value == MARKER)
            .map(|(key, _)| key.clone());
        url.set_query(None);
        url.set_fragment(None);
        match field {
            Some(field) => (
                url.to_string(),
                field,
                pairs.into_iter().filter(|(_, v)| v != MARKER).collect(),
            ),
            // Path-based templates cannot be expressed as a form; use the default engine.
            None => Self::default().search_form(),
        }
    }
    pub fn permits(&self, url: &url::Url) -> bool {
        let host = url.host_str().unwrap_or_default().trim_end_matches('.');
        let matches = |list: &str| {
            list.split_whitespace().any(|rule| {
                let rule = rule.trim_start_matches("*.").trim_end_matches('.');
                if rule.is_empty() {
                    return false;
                }
                host.eq_ignore_ascii_case(rule)
                    || host
                        .to_ascii_lowercase()
                        .ends_with(&format!(".{}", rule.to_ascii_lowercase()))
            })
        };
        !matches(&self.blocked_hosts)
            && (self.allowed_hosts.trim().is_empty() || matches(&self.allowed_hosts))
    }
}

#[cfg(feature = "desktop")]
#[path = "theme_palette.rs"]
mod presentation;
#[cfg(feature = "desktop")]
pub use presentation::*;

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;

/// Byte range of the actual host in a canonical HTTP(S) URL. Credentials and paths
/// must never be mistaken for the host when painting the address field.
pub fn address_host_range(address: &str) -> Option<std::ops::Range<usize>> {
    let url = url::Url::parse(address).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.as_str() != address
        || url.host_str().is_none()
    {
        return None;
    }
    Some(url[..url::Position::BeforeHost].len()..url[..url::Position::AfterHost].len())
}
