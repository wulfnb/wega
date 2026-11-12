//! What a script host needs from the document it runs against.
//!
//! The V8 glue depends on this interface instead of the engine's concrete `Dom`.
//! It contains no DOM, Blitz or V8/Deno types: only the document URL, the bounded
//! network handle, counters and queues the host drains after script runs.
//!
//! Threading and reentrancy:
//! - The environment is shared as `Rc<E>` by the script host and the isolate's
//!   `OpState` (as `Rc<dyn NativeHost>`); everything runs on the document's
//!   thread, so `E` need not be `Send` or `Sync`. The values it hands out for use
//!   on other threads (`network`, `wake`) are `Send + Sync`.
//! - Methods are called from the script host between or during script steps and
//!   from V8 callbacks (via `NativeHost`). They must not run script (no JS
//!   re-entry) and must not keep a `RefCell` borrow across a call into script.
use super::{native_ops::NativeHost, net::ScriptNetwork};
use std::sync::{Arc, RwLock};
use url::Url;

pub trait ScriptEnvironment: NativeHost + 'static {
    /// Requests the page queued for the host (popups, history, dynamic scripts, …).
    type Requests: Default;

    // Lifecycle
    /// A script task starts: invalidate cached document state.
    fn begin_task(&self);
    // Security context
    fn document_url(&self) -> Url;
    // Network access
    fn network(&self) -> Arc<dyn ScriptNetwork>;
    fn network_policy(&self) -> Arc<RwLock<wega_network::Policy>>;
    /// Wakes the owning event loop; handed to wakers used off the isolate thread.
    fn wake(&self) -> Arc<dyn Fn() + Send + Sync>;
    // Resource ownership and budgets
    fn budget(&self) -> (usize, usize);
    fn sync_nodes(&self, count: usize);
    fn history(&self) -> (usize, usize);
    // Results drained by the view
    fn take_errors(&self) -> Vec<String>;
    /// Record a notice about a slow script (the environment bounds how many it keeps).
    fn note_slow_script(&self, message: String);
    fn take_requests(&self) -> Self::Requests;
}
