use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use url::Url;

pub const HOME: &str = "https://www.google.com/";

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum TabLayout {
    Top,
    #[default]
    Side,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub visited_at: u64,
    /// Visits observed for this history entry; zero means an older unknown count.
    #[serde(default)]
    pub visit_count: u64,
}

/// Largest accepted encoded favicon. 64×64 RGBA is 16 KiB uncompressed.
pub const MAX_FAVICON_BYTES: usize = 32 * 1024;

/// A persisted favicon. New profiles store PNG bytes (base64 in JSON) produced by the
/// desktop layer. Profiles written before that stored raw RGBA number arrays; those are
/// still read and are converted to PNG by the desktop layer after startup.
#[derive(Clone, Debug, PartialEq)]
pub enum SavedFavicon {
    Png(Vec<u8>),
    Rgba {
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
}
impl SavedFavicon {
    /// Pixel size from the PNG header or the legacy fields, without decoding pixels.
    pub fn dimensions(&self) -> Option<(usize, usize)> {
        match self {
            Self::Png(bytes) => png_dimensions(bytes),
            Self::Rgba { width, height, .. } => Some((*width, *height)),
        }
    }
    /// Structurally valid and no larger than `max` pixels in each direction.
    pub fn valid(&self, max: usize) -> bool {
        let Some((width, height)) = self.dimensions() else {
            return false;
        };
        let sized = width > 0 && height > 0 && width <= max && height <= max;
        sized
            && match self {
                Self::Png(bytes) => bytes.len() <= MAX_FAVICON_BYTES,
                Self::Rgba { rgba, .. } => rgba.len() == width * height * 4,
            }
    }
}
fn png_dimensions(bytes: &[u8]) -> Option<(usize, usize)> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let field = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    Some((field(16), field(20)))
}
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum FaviconRepr {
    Png {
        png: String,
    },
    Rgba {
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
}
impl Serialize for SavedFavicon {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Png(bytes) => FaviconRepr::Png {
                png: crate::base64_text::encode(bytes),
            },
            Self::Rgba {
                width,
                height,
                rgba,
            } => FaviconRepr::Rgba {
                width: *width,
                height: *height,
                rgba: rgba.clone(),
            },
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for SavedFavicon {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // A corrupt icon must not make the whole profile unreadable: invalid base64
        // becomes an empty image that `normalize` discards.
        Ok(match FaviconRepr::deserialize(deserializer)? {
            FaviconRepr::Png { png } => {
                Self::Png(crate::base64_text::decode(&png).unwrap_or_default())
            }
            FaviconRepr::Rgba {
                width,
                height,
                rgba,
            } => Self::Rgba {
                width,
                height,
                rgba,
            },
        })
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub policy: crate::model::Policy,
    pub favicons: std::collections::BTreeMap<String, SavedFavicon>,
    pub layout: TabLayout,
    pub devtools_dock: crate::model::Dock,
    pub theme: crate::model::Theme,
    pub sidebar_width: f32,
    pub urls: Vec<String>,
    /// Last known page titles for `urls`, shown for tabs that are not loaded yet.
    pub titles: Vec<String>,
    /// Matches saved URL order; migrated to Wega for standalone browser operation.
    pub engines: Vec<crate::engine_choice::EngineKind>,
    pub active: usize,
    pub bookmarks: Vec<Bookmark>,
    pub history: Vec<Bookmark>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            policy: Default::default(),
            favicons: Default::default(),
            layout: TabLayout::Side,
            devtools_dock: Default::default(),
            theme: crate::model::Theme::default(),
            sidebar_width: 240.0,
            urls: vec![HOME.into()],
            titles: Vec::new(),
            engines: Vec::new(),
            active: 0,
            bookmarks: vec![
                Bookmark {
                    title: "Servo".into(),
                    url: "https://servo.org/".into(),
                    visited_at: 0,
                    visit_count: 0,
                },
                Bookmark {
                    title: "Rust".into(),
                    url: "https://www.rust-lang.org/".into(),
                    visited_at: 0,
                    visit_count: 0,
                },
            ],
            history: Vec::new(),
        }
    }
}

impl Profile {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("browser.json");
        match fs::read(&path) {
            Ok(bytes) => {
                let mut profile: Self = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
                profile.normalize();
                Ok(profile)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Cannot read {}: {e}", path.display())),
        }
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        crate::storage::atomic_write(&dir.join("browser.json"), &bytes).map_err(|e| e.to_string())
    }

    /// Normalize legacy or manually edited settings before they reach the UI.
    pub fn normalize(&mut self) {
        if !self.sidebar_width.is_finite() {
            self.sidebar_width = 240.0;
        }
        self.sidebar_width = self.sidebar_width.clamp(150.0, 500.0);
        let mut titles = std::mem::take(&mut self.titles).into_iter();
        let tabs: Vec<(String, String)> = std::mem::take(&mut self.urls)
            .into_iter()
            .map(|url| (url, titles.next().unwrap_or_default()))
            .filter(|(url, _)| resolve_address(url).is_ok())
            .take(100)
            .collect();
        self.engines = tabs
            .iter()
            .map(|_| crate::engine_choice::EngineKind::Wega)
            .collect();
        (self.urls, self.titles) = tabs.into_iter().unzip();
        for title in &mut self.titles {
            if title.len() > 512 {
                *title = title.chars().take(256).collect();
            }
        }
        if self.urls.is_empty() {
            self.urls.push(HOME.into());
            self.titles.push(String::new());
            self.engines.push(crate::engine_choice::EngineKind::Wega);
        }
        self.active = self.active.min(self.urls.len() - 1);
        self.favicons.retain(|_, image| image.valid(64));
        while self.favicons.len() > 128 {
            self.favicons.pop_first();
        }
    }

    /// Keep saved pages first, then fill with frequent pages. Stable sorting keeps
    /// recent history order when counts tie. History deletion removes counts too.
    pub fn new_tab_shortcuts(&self) -> Vec<&Bookmark> {
        let mut frequent: Vec<_> = self.history.iter().collect();
        frequent.sort_by_key(|entry| std::cmp::Reverse(entry.visit_count.max(1)));
        let mut seen = std::collections::HashSet::new();
        self.bookmarks
            .iter()
            .chain(frequent)
            .filter(|entry| seen.insert(entry.url.as_str()))
            .take(8)
            .collect()
    }

    pub fn toggle_bookmark(&mut self, title: String, url: String) {
        if let Some(index) = self
            .bookmarks
            .iter()
            .position(|bookmark| bookmark.url == url)
        {
            self.bookmarks.remove(index);
        } else {
            self.bookmarks.push(Bookmark {
                title,
                url,
                visited_at: 0,
                visit_count: 0,
            });
        }
    }

    pub fn default_dir() -> PathBuf {
        directories::ProjectDirs::from("io.github", "wulfnb", "Wega")
            .map(|p| p.data_local_dir().to_owned())
            .unwrap_or_else(|| PathBuf::from(".wega"))
    }
}

pub fn visit_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn history_date(visited_at: u64, now: chrono::DateTime<chrono::Local>) -> (String, String) {
    if visited_at == 0 {
        return ("Earlier visits".into(), String::new());
    }
    let Some(date) = i64::try_from(visited_at)
        .ok()
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .map(|t| t.with_timezone(&chrono::Local))
    else {
        return ("Earlier visits".into(), String::new());
    };
    let day = date.date_naive();
    let today = now.date_naive();
    let group = if day == today {
        "Today".into()
    } else if today.pred_opt() == Some(day) {
        "Yesterday".into()
    } else {
        date.format("%d %b %Y").to_string()
    };
    (group, date.format("%H:%M").to_string())
}

/// Resolve an address with the default search engine.
pub fn resolve_address(input: &str) -> Result<Url, String> {
    resolve_address_with(input, &crate::model::Policy::default())
}

/// Resolve typed input to a URL; text that is not an address searches with the policy's engine.
pub fn resolve_address_with(input: &str, policy: &crate::model::Policy) -> Result<Url, String> {
    let input = input.trim();
    if input.is_empty() {
        return Url::parse(HOME).map_err(|e| e.to_string());
    }
    if Path::new(input).is_absolute() {
        return Url::from_file_path(input).map_err(|_| "Invalid local file path".into());
    }
    if input.contains("://") || input.starts_with("about:") || input.starts_with("data:") {
        let url = Url::parse(input).map_err(|e| e.to_string())?;
        return match url.scheme() {
            "http" | "https" | "file" | "data" | "wega" => Ok(url),
            "about" if url.as_str() == "about:blank" => Ok(url),
            _ => Err(format!("Unsupported address scheme: {}", url.scheme())),
        };
    }
    if !input.chars().any(char::is_whitespace) {
        let local = input.starts_with("localhost")
            || input.starts_with("127.")
            || input.starts_with("[::1]");
        if local || input.contains('.') {
            return Url::parse(&format!(
                "{}://{input}",
                if local { "http" } else { "https" }
            ))
            .map_err(|e| e.to_string());
        }
    }
    Ok(policy.search_url(input))
}

/// Move a tab while preserving the identity of the selected tab.
pub fn move_tab<T>(tabs: &mut Vec<T>, active: &mut usize, from: usize, to: usize) {
    if from >= tabs.len() || to >= tabs.len() || from == to {
        return;
    }
    let item = tabs.remove(from);
    tabs.insert(to, item);
    *active = if *active == from {
        to
    } else if from < *active && to >= *active {
        *active - 1
    } else if from > *active && to <= *active {
        *active + 1
    } else {
        *active
    };
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "profile_failure_tests.rs"]
mod failure_tests;
