//! ServiceWorker registration, scope matching, and offline fetch interception.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use url::Url;
use wega_network::{CacheEntry, CacheStorage};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceWorkerState {
    Installing,
    Installed,
    Activating,
    Activated,
    Redundant,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceWorkerInfo {
    pub script_url: String,
    pub scope: String,
    pub state: ServiceWorkerState,
}

pub struct ServiceWorkerRegistration {
    pub script_url: String,
    pub scope: String,
    pub state: ServiceWorkerState,
    pub storage_dir: Option<PathBuf>,
}

impl ServiceWorkerRegistration {
    pub fn new(script_url: String, scope: String, storage_dir: Option<PathBuf>) -> Self {
        Self {
            script_url,
            scope,
            state: ServiceWorkerState::Activated,
            storage_dir,
        }
    }

    /// Checks if a request URL falls within this registration's scope.
    pub fn matches_scope(&self, url: &Url) -> bool {
        let Ok(scope_url) = Url::parse(&self.scope) else {
            return false;
        };
        if url.origin() != scope_url.origin() {
            return false;
        }
        url.path().starts_with(scope_url.path())
    }

    /// Attempts to intercept a fetch request, serving cached response if available.
    pub fn intercept_fetch(&self, url: &Url) -> Option<CacheEntry> {
        let storage_dir = self.storage_dir.as_ref()?;
        let mut store = CacheStorage::open(storage_dir).ok()?;
        let cache_names = store.cache_names().ok()?;
        for name in cache_names {
            if let Ok(cache_id) = store.open_cache(&name)
                && let Ok(Some(entry)) = store.match_entry(cache_id, url.as_str())
            {
                return Some(entry);
            }
        }
        None
    }
}

#[derive(Default)]
pub struct ServiceWorkerManager {
    registrations: HashMap<String, ServiceWorkerRegistration>,
}

impl ServiceWorkerManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        document_url: &Url,
        script_url: &str,
        scope: Option<&str>,
        storage_dir: Option<PathBuf>,
    ) -> Result<ServiceWorkerInfo, String> {
        let script = document_url
            .join(script_url)
            .map_err(|e| format!("Invalid script URL: {e}"))?;

        if script.origin() != document_url.origin() {
            return Err("SecurityError: Script must be same-origin with the document".into());
        }

        let scope_url = if let Some(s) = scope {
            document_url
                .join(s)
                .map_err(|e| format!("Invalid scope: {e}"))?
        } else {
            let mut s = script.clone();
            let path = s.path().to_string();
            let dir = path
                .rsplit_once('/')
                .map(|(d, _)| format!("{d}/"))
                .unwrap_or_else(|| "/".into());
            s.set_path(&dir);
            s
        };

        if scope_url.origin() != document_url.origin() {
            return Err("SecurityError: Scope must be same-origin with the document".into());
        }

        let scope_str = scope_url.to_string();
        let registration =
            ServiceWorkerRegistration::new(script.to_string(), scope_str.clone(), storage_dir);

        let info = ServiceWorkerInfo {
            script_url: registration.script_url.clone(),
            scope: scope_str.clone(),
            state: registration.state,
        };

        self.registrations.insert(scope_str, registration);
        Ok(info)
    }

    pub fn get_registration(&self, url: &Url) -> Option<ServiceWorkerInfo> {
        for reg in self.registrations.values() {
            if reg.matches_scope(url) {
                return Some(ServiceWorkerInfo {
                    script_url: reg.script_url.clone(),
                    scope: reg.scope.clone(),
                    state: reg.state,
                });
            }
        }
        None
    }

    pub fn unregister(&mut self, scope: &str) -> bool {
        self.registrations.remove(scope).is_some()
    }

    pub fn intercept_fetch(&self, url: &Url) -> Option<CacheEntry> {
        for reg in self.registrations.values() {
            if reg.matches_scope(url)
                && let Some(entry) = reg.intercept_fetch(url)
            {
                return Some(entry);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_worker_registration_scope_and_fetch_interception() {
        let doc_url = Url::parse("https://app.test/app/index.html").unwrap();
        let mut mgr = ServiceWorkerManager::new();

        // Register worker
        let info = mgr
            .register(&doc_url, "sw.js", Some("/app/"), None)
            .unwrap();
        assert_eq!(info.scope, "https://app.test/app/");
        assert_eq!(info.state, ServiceWorkerState::Activated);

        // Scope match verification
        let matched = mgr
            .get_registration(&Url::parse("https://app.test/app/api/data").unwrap())
            .unwrap();
        assert_eq!(matched.scope, "https://app.test/app/");

        // Non-matching scope
        assert!(
            mgr.get_registration(&Url::parse("https://app.test/other/path").unwrap())
                .is_none()
        );

        // Cross-origin rejected
        let cross = mgr.register(&doc_url, "https://evil.test/sw.js", None, None);
        assert!(cross.is_err());
        assert!(cross.unwrap_err().contains("SecurityError"));

        // Unregister
        assert!(mgr.unregister("https://app.test/app/"));
        assert!(
            mgr.get_registration(&Url::parse("https://app.test/app/api/data").unwrap())
                .is_none()
        );
    }
}
