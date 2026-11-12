//! Script host contract. Fatal limits tear down a host; ordinary exceptions do not.
use blitz_dom::{BaseDocument, NodeId};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};
use wega_adapter_v8::{HostConfig, RuntimeLimits, ScriptError, ScriptHost, V8Host};
/// Everything a script host shares with its view.
#[derive(Clone)]
pub(crate) struct HostContext {
    pub document: Rc<RefCell<BaseDocument>>,
    pub navigate: Rc<RefCell<Vec<url::Url>>>,
    pub network: crate::network::Network,
    pub hubs: crate::workers::Hubs,
    pub csp: std::sync::Arc<std::sync::RwLock<crate::csp::Csp>>,
    pub dialog: Rc<RefCell<Option<crate::dom::DocumentDialogHandler>>>,
    pub document_generation: Rc<Cell<u64>>,
    pub document_revision: u64,
    pub activation: Rc<Cell<Option<Instant>>>,
    /// Script elements already prepared ("already started"), shared with the view.
    pub started_scripts: Rc<RefCell<std::collections::HashSet<NodeId>>>,
    pub permissions: Rc<RefCell<crate::permissions::PermissionRegistry>>,
}

/// Trusted bootstrap factories shared by document and worker realms, built by `build.rs`.
pub(crate) static STARTUP_SNAPSHOT: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/wega-startup.bin"));

pub(crate) type DynScriptHost = dyn ScriptHost<Requests = crate::dom::Requests>;

pub(crate) fn create(
    dom: std::rc::Rc<crate::dom::Dom>,
    limits: &crate::Limits,
) -> Result<Box<DynScriptHost>, String> {
    Ok(Box::new(V8Host::new(
        dom,
        &HostConfig {
            limits: RuntimeLimits {
                js_heap_bytes: limits.js_heap_bytes,
                script_time: limits.script_time,
                slow_script: limits.slow_script,
                resource_bytes: limits.resource_bytes,
            },
            snapshot: STARTUP_SNAPSHOT,
            bootstrap: include_str!("dom/bootstrap.js"),
        },
    )?))
}

/// HTML performs a checkpoint after an ordinary exception too. A fatal limit
/// tears down the realm and must never run another callback while recovering.
pub(crate) fn finish_task<T>(
    host: &mut DynScriptHost,
    result: Result<T, ScriptError>,
) -> Result<T, ScriptError> {
    if !result.as_ref().is_err_and(|error| error.fatal) {
        host.checkpoint()?;
    }
    result
}
