//! Owned page orchestration on Blitz, independent of Servo and SpiderMonkey.
#![forbid(unsafe_code)]
pub use blitz_traits::events;
pub use blitz_traits::shell::ColorScheme;
pub use url::Url;

mod cookies;
mod dom;
#[cfg(feature = "v8")]
mod module_loader;
mod network;
#[cfg(feature = "quickjs")]
mod quickjs;
mod script;
#[cfg(feature = "v8")]
mod v8;
#[cfg(feature = "v8")]
mod web;
#[cfg(not(any(feature = "v8", feature = "quickjs")))]
compile_error!("Enable the v8 or quickjs engine feature");
mod view;
pub use network::NetworkPolicy;
pub use view::{Frame, View};

#[derive(Clone, Debug)]
pub struct Limits {
    pub js_heap_bytes: usize,
    pub script_time: std::time::Duration,
    pub html_bytes: usize,
    pub resource_bytes: usize,
    pub dom_nodes: usize,
    pub dom_text_bytes: usize,
    pub max_pixels: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            js_heap_bytes: 32 * 1024 * 1024,
            script_time: std::time::Duration::from_millis(100),
            html_bytes: 4 * 1024 * 1024,
            resource_bytes: 8 * 1024 * 1024,
            dom_nodes: 30_000,
            dom_text_bytes: 16 * 1024 * 1024,
            max_pixels: 4096 * 2160,
        }
    }
}

/// Shared bounded networking; each View owns its DOM, VM and render surfaces.
pub struct Engine {
    network: network::Network,
    limits: Limits,
}
impl Engine {
    pub fn new(
        limits: Limits,
        policy: NetworkPolicy,
        wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        Ok(Self {
            network: network::Network::new(policy, wake)?,
            limits,
        })
    }
    /// Storage is always partitioned by the document's tuple origin by the host.
    pub fn with_storage_dir(mut self, path: std::path::PathBuf) -> Self {
        self.network.cookies.set_directory(path.clone());
        self.network.storage_dir = Some(path);
        self
    }
    pub fn create_view(&self, url: Url) -> Result<View, String> {
        View::new(url, self.network.clone(), self.limits.clone())
    }
    pub fn set_policy(&self, policy: NetworkPolicy) {
        *self
            .network
            .policy
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = policy;
    }
}
