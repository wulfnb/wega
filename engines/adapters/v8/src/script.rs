//! Script host contract. Fatal limits tear down a host; ordinary exceptions do not.
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct ScriptError {
    pub fatal: bool,
    pub message: String,
}
impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ScriptError {}

/// A thrown value converted to plain data. The adapter never exposes a V8 type.
#[derive(Clone, Debug)]
pub struct InspectException {
    pub name: String,
    pub message: String,
    pub stack: Option<String>,
    pub url: Option<String>,
    pub line: u32,
    pub column: u32,
}

/// The result of an inspector evaluation: a side-effect-free preview (JSON text produced by
/// the bootstrap-captured generator) or the exception the expression threw.
#[derive(Clone, Debug)]
pub enum InspectOutcome {
    Value { preview_json: String },
    Exception(InspectException),
}

pub trait ScriptHost {
    /// The page's queued host requests; an associated type so the script host does not
    /// name the embedder's document types.
    type Requests;
    fn run(&mut self, source: &str, name: &str) -> Result<String, ScriptError>;
    fn run_module(&mut self, source: &str, name: &str) -> Result<String, ScriptError>;
    fn modules_pending(&self) -> bool;
    fn evaluate_json(&mut self, source: &str) -> Result<String, ScriptError>;
    /// Run `source` in the main world under the same watchdog and heap budget as page
    /// script, and describe the completion value without invoking page code: no JSON
    /// serialisation (`toJSON`, getters) and no `String(value)`. A fatal error tears the
    /// realm down exactly as for any other script.
    fn evaluate_inspect(&mut self, source: &str) -> Result<InspectOutcome, ScriptError>;
    fn checkpoint(&mut self) -> Result<(), ScriptError>;
    fn tick(&mut self) -> Result<(), ScriptError>;
    fn next_timer(&self) -> Option<Duration>;
    fn heap_bytes(&self) -> usize;
    fn take_errors(&mut self) -> Vec<String>;
    fn freeze(&mut self, frozen: bool);
    /// Page requests (dynamic scripts, document.write, popups, history).
    fn take_requests(&mut self) -> Self::Requests;
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
        self.checkpoint()?;
        Ok(self.evaluate_json("__wega_has_frame()")? == "true")
    }
}
