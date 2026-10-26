//! Owned page orchestration on Blitz, independent of Servo and SpiderMonkey.
#![forbid(unsafe_code)]
pub use blitz_traits::events;
pub use blitz_traits::shell::{ColorScheme, Viewport};
pub use url::Url;

mod cookies;
mod csp;
mod dom;
mod encoding;
mod extension_adapter;
mod module_loader;
mod native_ops;
mod network;
mod parser;
mod raster;
mod script;
mod v8;
mod view;
mod web;
mod web_extensions;
pub use dom::DialogHandler;
pub use network::NetworkPolicy;
pub use raster::{Painted, Pixels};
pub use view::{Frame, View};
pub use view::{PageEvent, Pointer, PointerKind};

#[derive(Clone, Debug)]
pub struct Limits {
    pub js_heap_bytes: usize,
    /// Hard limit for one script task; exceeding it tears the page's VM down.
    pub script_time: std::time::Duration,
    /// Tasks longer than this are reported as slow but keep running.
    pub slow_script: std::time::Duration,
    pub html_bytes: usize,
    pub resource_bytes: usize,
    pub dom_nodes: usize,
    pub dom_text_bytes: usize,
    pub max_pixels: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            js_heap_bytes: 128 * 1024 * 1024,
            script_time: std::time::Duration::from_secs(5),
            slow_script: std::time::Duration::from_millis(100),
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
    /// Private tab workers delegate HTTP and cookies to the browser's shared broker.
    pub fn brokered(
        limits: Limits,
        policy: NetworkPolicy,
        wake: std::sync::Arc<dyn Fn() + Send + Sync>,
        address: &str,
        token: String,
    ) -> Result<Self, String> {
        Ok(Self {
            network: network::Network::brokered(
                policy,
                wake,
                wega_network::Remote::new(address, token)?,
            )?,
            limits,
        })
    }
    /// Storage is always partitioned by the document's tuple origin by the host.
    pub fn with_storage_dir(mut self, path: std::path::PathBuf) -> Self {
        self.network.cookies.set_directory(path.clone());
        self.network.storage_dir = Some(path);
        self
    }
    /// Keep each view on its owning DOM thread. As required by Rusty V8, nested
    /// views on one thread must be destroyed/reset in reverse creation order.
    /// Native browser tabs each have their own worker process and DOM thread.
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
