//! Construction of a Deno runtime restored from the startup snapshot, shared by
//! document and worker realms.
use crate::{
    broker_socket::Connector, module_loader::Loader, native_ops::NativeHost, net::ScriptNetwork,
    web,
};
use deno_core::{JsRuntime, RuntimeOptions};
use std::{
    rc::Rc,
    sync::{Arc, RwLock},
};

/// Which realm the runtime is for; the two differ in storage and extension setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeKind {
    /// A page: origin-partitioned Web Storage (brokered or local) plus the WebSocket broker.
    Document,
    /// A dedicated, shared or service worker: WebSocket broker only, no Web Storage directory.
    Worker,
}

pub struct RuntimeSpec {
    pub kind: RuntimeKind,
    /// The document URL (documents) or the worker script URL (workers).
    pub url: url::Url,
    pub network: Arc<dyn ScriptNetwork>,
    pub policy: Arc<RwLock<wega_network::Policy>>,
    /// Host callbacks stored in the runtime's `OpState`; lives as long as the runtime.
    pub host: Rc<dyn NativeHost>,
    /// The snapshot built with [`crate::build_startup_snapshot`].
    pub snapshot: &'static [u8],
    pub js_heap_bytes: usize,
    pub module_limit_bytes: usize,
}

/// Creates the runtime and installs host state in its `OpState`. Must be called on the
/// thread that will own the isolate, inside the executor the host will drive it with.
/// Bootstrap scripts and secure-context exposure are the caller's (they differ per realm).
pub(crate) fn create_runtime(spec: RuntimeSpec) -> Result<JsRuntime, String> {
    let mut extensions = match spec.kind {
        RuntimeKind::Document => web::extensions(&spec.url, &*spec.network),
        RuntimeKind::Worker => crate::web_extensions::extensions(spec.url.clone(), None),
    };
    extensions.push(crate::native_ops::wega_dom::init());
    if spec.kind == RuntimeKind::Document {
        for extension in &extensions {
            for source in extension
                .js_files
                .iter()
                .chain(extension.esm_files.iter())
                .chain(extension.lazy_loaded_js_files.iter())
                .chain(extension.lazy_loaded_esm_files.iter())
            {
                if !source.is_runtime_loadable() {
                    return Err(format!(
                        "Extension {} must be embedded in the worker binary",
                        source.specifier
                    ));
                }
            }
        }
    }
    let runtime = JsRuntime::try_new(RuntimeOptions {
        startup_snapshot: Some(spec.snapshot),
        module_loader: Some(Rc::new(Loader::new(
            spec.network.clone(),
            &spec.url,
            spec.module_limit_bytes,
        ))),
        extension_transpiler: Some(Rc::new(web::adapt_extension)),
        // Per-realm ownership: the store is never shared across origins.
        shared_array_buffer_store: Some(Default::default()),
        create_params: Some(
            deno_core::v8::CreateParams::default()
                .heap_limits(0, spec.js_heap_bytes.saturating_add(32 * 1024 * 1024)),
        ),
        extensions,
        ..Default::default()
    })
    .map_err(|error| error.to_string())?;
    {
        let state = runtime.op_state();
        let mut state = state.borrow_mut();
        // Deno's trusted bootstrap needs its own heap before page code starts.
        state.put(spec.host);
        state.put(web::permissions(spec.policy));
        state.put(String::new());
        if let Some(remote) = spec.network.remote().filter(|remote| remote.is_bound()) {
            deno_websocket::set_broker(
                &mut state,
                Arc::new(Connector {
                    network: spec.network.clone(),
                }),
            );
            if spec.kind == RuntimeKind::Document {
                deno_webstorage::set_backend(
                    &mut state,
                    Arc::new(web::BrokerStorage {
                        remote,
                        origin: spec.url.origin().ascii_serialization(),
                    }),
                );
            }
        }
    }
    Ok(runtime)
}

/// Exposes `isSecureContext` (and removes secure-only globals) for the realm's URL.
pub(crate) fn configure_secure_context(
    runtime: &mut JsRuntime,
    url: &url::Url,
) -> Result<(), String> {
    crate::security_context::configure(runtime, url)
}
