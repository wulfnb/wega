//! Script host contract. Fatal limits tear down a host; ordinary exceptions do not.
use blitz_dom::{BaseDocument, NodeId};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
#[derive(Debug)]
pub(crate) struct ScriptError {
    pub fatal: bool,
    pub message: String,
}
impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ScriptError {}

/// Everything a script host shares with its view.
#[derive(Clone)]
pub(crate) struct HostContext {
    pub document: Rc<RefCell<BaseDocument>>,
    pub navigate: Rc<RefCell<Vec<url::Url>>>,
    pub network: crate::network::Network,
    pub csp: std::sync::Arc<std::sync::RwLock<crate::csp::Csp>>,
    pub dialog: Rc<RefCell<Option<crate::dom::DocumentDialogHandler>>>,
    pub document_generation: Rc<Cell<u64>>,
    pub document_revision: u64,
    pub activation: Rc<Cell<Option<Instant>>>,
    /// Script elements already prepared ("already started"), shared with the view.
    pub started_scripts: Rc<RefCell<std::collections::HashSet<NodeId>>>,
}

pub(crate) trait ScriptHost {
    fn run(&mut self, source: &str, name: &str) -> Result<String, ScriptError>;
    fn run_module(&mut self, source: &str, name: &str) -> Result<String, ScriptError>;
    fn modules_pending(&self) -> bool;
    fn evaluate_json(&mut self, source: &str) -> Result<String, ScriptError>;
    fn tick(&mut self) -> Result<(), ScriptError>;
    fn next_timer(&self) -> Option<Duration>;
    fn heap_bytes(&self) -> usize;
    fn take_errors(&mut self) -> Vec<String>;
    fn freeze(&mut self, frozen: bool);
    /// Page requests (dynamic scripts, document.write, popups, history).
    fn take_requests(&mut self) -> crate::dom::Requests;
    /// CSP without 'unsafe-eval' disables eval and `new Function`.
    fn allow_eval(&mut self, allowed: bool);
    /// Allocated nodes and charged text bytes.
    fn budget(&self) -> (usize, usize);
    /// The parser created nodes outside the script budget; count them once parsed.
    fn sync_nodes(&mut self, count: usize);
    /// The document's own session history: (index, length).
    fn history(&self) -> (usize, usize);
    fn frame(&mut self, _timestamp: f64) -> Result<bool, ScriptError> {
        self.run("__wega_frame(performance.now())", "wega:animation-frame")?;
        self.tick()?;
        Ok(self.evaluate_json("__wega_has_frame()")? == "true")
    }
}
pub(crate) fn create(
    context: HostContext,
    limits: &crate::Limits,
    initial_bytes: usize,
) -> Result<Box<dyn ScriptHost>, String> {
    Ok(Box::new(crate::v8::V8Host::new(
        context,
        limits,
        initial_bytes,
    )?))
}
