//! Bounded RFC6265bis jar shared by HTTP resources, fetch and document.cookie.
//! SameSite is enforced from the request's site context; reqwest's cookie trait
//! cannot carry that context, so every request attaches cookies explicitly.
use cookie_store::{Cookie, CookieStore, RawCookie};
use publicsuffix::Psl;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Mutex};
use url::Url;

/// Where a request comes from, relative to the cookie's site.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SiteContext {
    /// Same scheme and registrable domain, or a navigation the user started.
    #[default]
    SameSite,
    /// A cross-site top-level navigation with a safe method: Lax cookies only.
    CrossSiteTopLevel,
    /// Any other cross-site request: only `SameSite=None; Secure` cookies.
    CrossSite,
}

struct State {
    store: CookieStore,
    directory: Option<PathBuf>,
}
pub struct Cookies {
    state: Mutex<State>,
}
fn suffixes() -> &'static publicsuffix::List {
    static LIST: std::sync::OnceLock<publicsuffix::List> = std::sync::OnceLock::new();
    LIST.get_or_init(suffix_list)
}
fn suffix_list() -> publicsuffix::List {
    publicsuffix::List::from_bytes(include_bytes!("public_suffix_list.dat"))
        .expect("pinned public suffix list")
}
impl Default for Cookies {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                store: CookieStore::default().with_suffix_list(suffix_list()),
                directory: None,
            }),
        }
    }
}
impl Cookies {
    /// The registrable domain ("site") of a host; IP addresses and single-label
    /// hosts are their own site.
    fn site(url: &Url) -> Option<(String, String)> {
        let host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
        let registrable = match url.host() {
            Some(url::Host::Domain(_)) => suffixes()
                .domain(host.as_bytes())
                .and_then(|d| std::str::from_utf8(d.as_bytes()).ok().map(str::to_owned))
                .unwrap_or(host),
            _ => host,
        };
        let scheme = match url.scheme() {
            "wss" => "https",
            "ws" => "http",
            other => other,
        };
        Some((scheme.to_owned(), registrable))
    }
    pub fn same_site(a: &Url, b: &Url) -> bool {
        matches!((Self::site(a), Self::site(b)), (Some(a), Some(b)) if a == b)
    }
    /// The context for a request to `url` made by a document at `initiator`.
    /// A missing initiator is a navigation the user started (address bar, reload).
    pub fn context(url: &Url, initiator: Option<&Url>, top_level_safe: bool) -> SiteContext {
        match initiator {
            None => SiteContext::SameSite,
            Some(initiator) if Self::same_site(url, initiator) => SiteContext::SameSite,
            Some(_) if top_level_safe => SiteContext::CrossSiteTopLevel,
            Some(_) => SiteContext::CrossSite,
        }
    }
    pub fn set_directory(&self, directory: PathBuf) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Ok(file) = std::fs::File::open(directory.join("cookies.json"))
            && file.metadata().is_ok_and(|m| m.len() <= 4 * 1024 * 1024)
            && let Ok(store) = cookie_store::serde::json::load(std::io::BufReader::new(file))
            && store.iter_unexpired().count() <= 512
        {
            state.store = store.with_suffix_list(suffix_list());
        }
        state.directory = Some(directory);
    }
    /// The cookie's SameSite attribute, lowercased ("strict", "lax", "none").
    fn same_site_of(cookie: &RawCookie<'_>) -> Option<String> {
        cookie
            .same_site()
            .map(|s| s.to_string().to_ascii_lowercase())
    }
    fn allowed(cookie: &RawCookie<'_>, context: SiteContext) -> bool {
        match (Self::same_site_of(cookie).as_deref(), context) {
            (_, SiteContext::SameSite) => true,
            (Some("strict"), _) => false,
            (Some("none"), _) => cookie.secure().unwrap_or(false),
            // Unset SameSite is treated as Lax.
            (_, SiteContext::CrossSiteTopLevel) => true,
            (_, SiteContext::CrossSite) => false,
        }
    }
    pub fn get(&self, url: &Url, script: bool, context: SiteContext) -> String {
        if !matches!(url.scheme(), "http" | "https" | "ws" | "wss") {
            return String::new();
        }
        let mut lookup = url.clone();
        match url.scheme() {
            "ws" => {
                let _ = lookup.set_scheme("http");
            }
            "wss" => {
                let _ = lookup.set_scheme("https");
            }
            _ => {}
        }
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut matching = state.store.matches(&lookup);
        matching.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));
        matching
            .into_iter()
            .filter(|cookie| !script || !cookie.http_only().unwrap_or(false))
            .filter(|cookie| Self::allowed(cookie, context))
            .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
            .collect::<Vec<_>>()
            .join("; ")
    }
    /// Cookies a top-level download of `url` would send, as Netscape cookie-file
    /// lines for the browser's download manager.
    pub fn netscape(&self, url: &Url) -> String {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let host = url.host_str().unwrap_or_default();
        state
            .store
            .matches(url)
            .into_iter()
            .filter(|cookie| {
                ![cookie.name(), cookie.value()]
                    .iter()
                    .any(|s| s.chars().any(|c| c.is_control() || c == '\t'))
            })
            .map(|cookie| {
                format!(
                    "{host}\tFALSE\t{}\t{}\t0\t{}\t{}\n",
                    String::from(&cookie.path),
                    if cookie.secure().unwrap_or(false) {
                        "TRUE"
                    } else {
                        "FALSE"
                    },
                    cookie.name(),
                    cookie.value()
                )
            })
            .collect()
    }
    pub fn set(&self, url: &Url, value: &str, script: bool, context: SiteContext) {
        if !matches!(url.scheme(), "http" | "https") || value.len() > 4096 {
            return;
        }
        let Ok(raw) = RawCookie::parse(value.to_owned()) else {
            return;
        };
        if script && raw.http_only().unwrap_or(false) {
            return;
        }
        // SameSite=None requires Secure; other cookies are not stored from
        // cross-site subresource responses.
        let same_site = Self::same_site_of(&raw);
        if same_site.as_deref() == Some("none") && !raw.secure().unwrap_or(false) {
            return;
        }
        if context == SiteContext::CrossSite && same_site.as_deref() != Some("none") {
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
            .state
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

#[cfg(test)]
mod tests {
    use super::*;
    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }
    #[test]
    fn same_site_uses_registrable_domain_and_scheme() {
        assert!(Cookies::same_site(
            &url("https://a.example.com/"),
            &url("https://b.example.com/")
        ));
        assert!(!Cookies::same_site(
            &url("https://example.com/"),
            &url("https://example.org/")
        ));
        assert!(!Cookies::same_site(
            &url("http://example.com/"),
            &url("https://example.com/")
        ));
        assert!(!Cookies::same_site(
            &url("https://a.github.io/"),
            &url("https://b.github.io/")
        ));
        assert!(Cookies::same_site(
            &url("http://127.0.0.1:1/"),
            &url("http://127.0.0.1:2/")
        ));
    }
    #[test]
    fn samesite_attributes_follow_the_request_context() {
        let jar = Cookies::default();
        let site = url("https://shop.example/");
        let user = SiteContext::SameSite;
        jar.set(&site, "lax=1; Path=/", false, user);
        jar.set(&site, "strict=1; Path=/; SameSite=Strict", false, user);
        jar.set(&site, "none=1; Path=/; SameSite=None; Secure", false, user);
        jar.set(&site, "insecure_none=1; Path=/; SameSite=None", false, user);
        let cookies = |context| {
            let mut names: Vec<String> = jar
                .get(&site, false, context)
                .split("; ")
                .filter(|c| !c.is_empty())
                .map(str::to_owned)
                .collect();
            names.sort();
            names.join("; ")
        };
        assert_eq!(cookies(SiteContext::SameSite), "lax=1; none=1; strict=1");
        assert_eq!(cookies(SiteContext::CrossSiteTopLevel), "lax=1; none=1");
        assert_eq!(cookies(SiteContext::CrossSite), "none=1");
        // A cross-site response may only store SameSite=None cookies.
        jar.set(&site, "tracker=1; Path=/", false, SiteContext::CrossSite);
        assert!(!cookies(SiteContext::SameSite).contains("tracker"));
    }
}
