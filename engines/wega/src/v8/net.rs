//! The network operations the script runtime glue (module loader, WebSocket broker
//! connector, extension setup) needs. The glue depends on this narrow interface
//! rather than on the engine's concrete `Network`, so it can move out of the engine.
use std::{future::Future, path::PathBuf, pin::Pin};
use url::Url;

pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub(crate) trait ScriptNetwork: Send + Sync {
    /// The browser broker, when this engine delegates HTTP, cookies and storage to it.
    fn remote(&self) -> Option<wega_network::Remote>;
    /// Local per-origin storage root when no broker owns storage.
    fn storage_dir(&self) -> Option<PathBuf>;
    /// Queue blocking work on the bounded network dispatcher.
    fn queue(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), String>;
    /// Fetch a module subresource through the document's network stack:
    /// the final URL and the bounded body.
    fn fetch_module(
        &self,
        document: &Url,
        url: Url,
        limit: usize,
        local: Option<PathBuf>,
    ) -> BoxFuture<Result<(Url, Vec<u8>), String>>;
}
