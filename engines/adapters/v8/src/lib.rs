//! V8/Deno script runtime adapter.
//!
//! Owns the Deno/V8 integration: the script host (`V8Host`), the op table and
//! extension setup, the module loader, the WebSocket broker connector, snapshot
//! generation and the isolate-entry guard. It does not depend on the embedding engine:
//! the engine implements [`ScriptEnvironment`] (document callbacks), [`NativeHost`]
//! (page callbacks) and [`ScriptNetwork`] (bounded network access), supplies the
//! startup snapshot and bootstrap script, and drives the host through [`ScriptHost`].
//!
//! `unsafe` is denied crate-wide; the single exception is `RawIsolateGuard` in
//! `host.rs`, whose invariants are documented there.
#![deny(unsafe_code)]

mod broker_socket;
mod env;
mod extension_adapter;
mod host;
mod module_loader;
mod native_ops;
mod net;
mod runtime;
mod script;
mod security_context;
mod snapshot;
mod web;
mod web_extensions;
mod worker;

pub use env::ScriptEnvironment;
pub use host::{HostConfig, RuntimeLimits, V8Host};
pub use native_ops::NativeHost;
pub use net::{BoxFuture, ScriptNetwork};
pub use runtime::{RuntimeKind, RuntimeSpec};
pub use script::{ScriptError, ScriptHost};
pub use snapshot::{SnapshotScript, build_startup_snapshot};
pub use worker::{LoopStatus, Turn, WorkerRuntime, WorkerTerminator};

/// V8 flags are process-wide: call before the first isolate is created.
pub fn set_lite_mode() {
    deno_core::v8_set_flags(vec![String::new(), "--lite-mode".into()]);
}
