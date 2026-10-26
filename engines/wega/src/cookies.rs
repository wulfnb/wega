//! Bounded RFC6265 jar shared by HTTP resources, fetch and document.cookie.
use cookie_store::{Cookie, CookieStore, RawCookie};
use std::{path::PathBuf, sync::Mutex};
use url::Url;
struct State {
    store: CookieStore,
    directory: Option<PathBuf>,
}
pub(crate) struct Cookies(Mutex<State>);
impl Default for Cookies {
    fn default() -> Self {
        let suffixes = publicsuffix::List::from_bytes(include_bytes!("public_suffix_list.dat"))
            .expect("pinned public suffix list");
        Self(Mutex::new(State {
            store: CookieStore::default().with_suffix_list(suffixes),
            directory: None,
        }))
    }
}
impl Cookies {
    pub fn set_directory(&self, directory: PathBuf) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Ok(file) = std::fs::File::open(directory.join("cookies.json"))
            && file.metadata().is_ok_and(|m| m.len() <= 4 * 1024 * 1024)
            && let Ok(store) = cookie_store::serde::json::load(std::io::BufReader::new(file))
            && store.iter_unexpired().count() <= 512
        {
            state.store = store.with_suffix_list(
                publicsuffix::List::from_bytes(include_bytes!("public_suffix_list.dat"))
                    .expect("pinned suffix list"),
            );
        }
        state.directory = Some(directory);
    }
    pub fn get(&self, url: &Url, script: bool) -> String {
        if !matches!(url.scheme(), "http" | "https") {
            return String::new();
        }
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut matching = state.store.matches(url);
        matching.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));
        matching
            .into_iter()
            .filter(|cookie| !script || !cookie.http_only().unwrap_or(false))
            .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
            .collect::<Vec<_>>()
            .join("; ")
    }
    pub fn set(&self, url: &Url, value: &str, script: bool) {
        if !matches!(url.scheme(), "http" | "https") || value.len() > 4096 {
            return;
        }
        let Ok(raw) = RawCookie::parse(value.to_owned()) else {
            return;
        };
        if script && raw.http_only().unwrap_or(false) {
            return;
        }
        if raw.name().starts_with("__Secure-")
            && (url.scheme() != "https" || !raw.secure().unwrap_or(false))
        {
            return;
        }
        if raw.name().starts_with("__Host-")
            && (url.scheme() != "https"
                || !raw.secure().unwrap_or(false)
                || raw.domain().is_some()
                || raw.path() != Some("/"))
        {
            return;
        }
        let Ok(cookie) = Cookie::try_from_raw_cookie(&raw, url).map(Cookie::into_owned) else {
            return;
        };
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let domain = cookie.domain.as_cow().unwrap_or_default();
        let old = state.store.get(&domain, &cookie.path, cookie.name());
        if script && old.is_some_and(|c| c.http_only().unwrap_or(false)) {
            return;
        }
        if old.is_none() && state.store.iter_unexpired().count() >= 512 {
            return;
        }
        if state.store.insert(cookie, url).is_err() {
            return;
        }
        if let Some(directory) = &state.directory
            && std::fs::create_dir_all(directory).is_ok()
        {
            let temporary = directory.join(format!("cookies-{}.tmp", std::process::id()));
            if let Ok(mut file) = std::fs::File::create(&temporary)
                && cookie_store::serde::json::save(&state.store, &mut file).is_ok()
            {
                let _ = std::fs::rename(&temporary, directory.join("cookies.json"));
            }
        }
    }
}
impl reqwest::cookie::CookieStore for Cookies {
    fn set_cookies(
        &self,
        headers: &mut dyn Iterator<Item = &reqwest::header::HeaderValue>,
        url: &Url,
    ) {
        for header in headers {
            if let Ok(value) = header.to_str() {
                self.set(url, value, false);
            }
        }
    }
    fn cookies(&self, url: &Url) -> Option<reqwest::header::HeaderValue> {
        let value = self.get(url, false);
        if value.is_empty() {
            None
        } else {
            reqwest::header::HeaderValue::from_str(&value).ok()
        }
    }
}
