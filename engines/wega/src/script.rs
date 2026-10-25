//! VM-neutral contract. Fatal limits tear down a host; ordinary exceptions do not.
use blitz_dom::{BaseDocument, NodeId};
use std::{cell::RefCell, rc::Rc, time::Duration};
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

pub(crate) trait ScriptHost {
    fn run(&mut self, source: &str, name: &str) -> Result<String, ScriptError>;
    fn run_module(&mut self, _source: &str, _name: &str) -> Result<String, ScriptError> {
        Err(ScriptError {
            fatal: false,
            message: "ES modules need the V8 host".into(),
        })
    }
    fn modules_pending(&self) -> bool {
        false
    }
    fn dispatch(&mut self, event: &str, id: NodeId) -> Result<bool, ScriptError>;
    fn evaluate_json(&mut self, source: &str) -> Result<String, ScriptError>;
    fn tick(&mut self) -> Result<(), ScriptError>;
    fn next_timer(&self) -> Option<Duration>;
    fn heap_bytes(&self) -> usize;
    fn take_errors(&mut self) -> Vec<String>;
    fn freeze(&mut self, frozen: bool);
    fn frame(&mut self, _timestamp: f64) -> Result<bool, ScriptError> {
        self.run("__wega_frame(performance.now())", "wega:animation-frame")?;
        self.tick()?;
        Ok(self.evaluate_json("__wega_has_frame()")? == "true")
    }
}
pub(crate) fn create(
    document: Rc<RefCell<BaseDocument>>,
    limits: &crate::Limits,
    navigate: Rc<RefCell<Vec<url::Url>>>,
    initial_bytes: usize,
    network: crate::network::Network,
) -> Result<Box<dyn ScriptHost>, String> {
    #[cfg(feature = "v8")]
    {
        Ok(Box::new(crate::v8::V8Host::new(
            document,
            limits,
            navigate,
            initial_bytes,
            network,
        )?))
    }
    #[cfg(all(not(feature = "v8"), feature = "quickjs"))]
    {
        Ok(Box::new(crate::quickjs::QuickJsHost::new(
            document,
            limits,
            navigate,
            initial_bytes,
            network,
        )?))
    }
}
