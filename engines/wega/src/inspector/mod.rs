//! Content-side inspector backend. It answers the closed `wega_protocol::inspector`
//! command set from facts the view already maintains, through the [`PagePort`] trait, and
//! names no layout, style or script engine type. A status request never forces layout;
//! evaluation runs only through the same watchdog-bounded path as page script, and only
//! after the grant check below.
use wega_protocol::{
    DocumentContext,
    inspector::{
        CONSOLE_BATCH_BYTES, CONSOLE_BATCH_ENTRIES, EvalOutcome, INSPECTOR_VERSION, InspectorArgs,
        InspectorCommand, InspectorError, InspectorErrorCode, InspectorEvent, InspectorRequest,
        InspectorResponse, InspectorSessionId, MAX_HISTORY_BYTES, StatusSnapshot, TargetId,
        TargetInfo, Viewport, session::ContentSessions,
    },
};

/// Everything a snapshot needs, gathered by the port only when a snapshot was requested.
#[derive(Clone, Debug)]
pub struct StatusInput {
    pub url: String,
    pub title: String,
    pub loading: bool,
    /// CSS width, height and device pixel ratio.
    pub viewport: Option<(f64, f64, f64)>,
    pub nodes: u64,
    pub js_heap_bytes: u64,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

/// Values only the engine's main loop owns (periodic samples, the requested viewport).
#[derive(Clone, Copy, Debug, Default)]
pub struct Sampled {
    pub nodes: u64,
    pub js_heap_bytes: u64,
    pub viewport: Option<(f64, f64, f64)>,
}

/// What the backend needs from the page. Implemented by the engine's `View`.
pub trait PagePort {
    fn status_input(&self, sampled: Sampled) -> StatusInput;
    fn console_first_seq(&self) -> u64;
    fn console_next_seq(&self) -> u64;
    fn console_len(&self) -> usize;
    fn console_dropped(&self) -> u64;
    fn console_read(
        &self,
        after: u64,
        max_entries: usize,
        max_bytes: usize,
    ) -> Vec<wega_protocol::inspector::ConsoleMessage>;
    fn console_clear(&mut self);
    /// Whether the document has a tuple origin (http, https, file). Opaque and internal
    /// documents need an explicit user override.
    fn origin_allows_evaluation(&self) -> bool;
    /// Run `expression` in the main world under the script watchdog. `Err` means the
    /// evaluation could not return an outcome; the text says what is known to have happened.
    fn evaluate(&mut self, expression: &str) -> Result<EvalOutcome, String>;
}

#[derive(Debug)]
struct Cursor {
    /// Last console sequence delivered (or deliberately skipped).
    after: u64,
}

#[derive(Debug)]
pub struct Backend {
    own: TargetId,
    protocol_version: u32,
    sessions: ContentSessions,
    baseline: Option<(DocumentContext, bool)>,
    last_document: Option<DocumentContext>,
    console: Option<Cursor>,
    opaque_override: bool,
}

fn engine_name() -> String {
    format!("wega-engine {}", env!("CARGO_PKG_VERSION"))
}

fn refuse(code: InspectorErrorCode, message: &str) -> InspectorError {
    InspectorError::new(code, message)
}

impl Backend {
    pub fn new(own: TargetId, protocol_version: u32) -> Self {
        Self {
            own,
            protocol_version,
            sessions: ContentSessions::default(),
            baseline: None,
            last_document: None,
            console: None,
            opaque_override: false,
        }
    }

    pub fn live(&self) -> Option<InspectorSessionId> {
        self.sessions.live()
    }

    fn end_session_state(&mut self) {
        self.baseline = None;
        self.console = None;
        self.last_document = None;
        self.opaque_override = false;
    }

    /// Execute a decoded, shape-validated request. `current` is the document stamp and loading
    /// flag right now; `port` is consulted only for commands that need it.
    pub fn handle(
        &mut self,
        request: &InspectorRequest,
        current: (DocumentContext, bool),
        sampled: Sampled,
        port: &mut dyn PagePort,
    ) -> Result<InspectorResponse, InspectorError> {
        self.sessions.authorize(request, self.own)?;
        match request.command {
            InspectorCommand::Attach => {
                // A new (or re-attached) session starts from the current document.
                if self.last_document.is_none() {
                    self.last_document = Some(current.0);
                }
                Ok(InspectorResponse::Attached)
            }
            InspectorCommand::Detach => {
                self.sessions.detach(request.session);
                self.end_session_state();
                Ok(InspectorResponse::Detached)
            }
            InspectorCommand::GetTargetInfo => Ok(InspectorResponse::TargetInfo(TargetInfo {
                target: self.own,
                document: current.0,
                protocol_version: self.protocol_version,
                inspector_version: INSPECTOR_VERSION,
                engine: engine_name(),
            })),
            InspectorCommand::SubscribeStatus => {
                self.sessions.set_subscribed(true);
                // Only changes after the subscription are events; the snapshot is the baseline.
                self.baseline = Some(current);
                Ok(InspectorResponse::Subscribed)
            }
            InspectorCommand::UnsubscribeStatus => {
                self.sessions.set_subscribed(false);
                self.baseline = None;
                Ok(InspectorResponse::Unsubscribed)
            }
            InspectorCommand::GetStatusSnapshot => {
                let input = port.status_input(sampled);
                Ok(InspectorResponse::Status(Box::new(
                    StatusSnapshot {
                        target: self.own,
                        document: current.0,
                        url: input.url,
                        title: input.title,
                        loading: input.loading,
                        viewport: input.viewport.map(|(width, height, device_pixel_ratio)| {
                            Viewport {
                                width,
                                height,
                                device_pixel_ratio,
                            }
                        }),
                        nodes: input.nodes,
                        js_heap_bytes: input.js_heap_bytes,
                        can_go_back: input.can_go_back,
                        can_go_forward: input.can_go_forward,
                        protocol_version: self.protocol_version,
                        inspector_version: INSPECTOR_VERSION,
                        engine: engine_name(),
                    }
                    .bounded(),
                )))
            }
            InspectorCommand::EnableConsole => {
                // Delivery starts from the oldest retained entry, so messages logged before
                // the panel opened (including during load) are backfilled, in budget.
                self.console = Some(Cursor {
                    after: port.console_first_seq().saturating_sub(1),
                });
                Ok(InspectorResponse::ConsoleEnabled {
                    retained: port.console_len().min(u32::MAX as usize) as u32,
                    first_seq: port.console_first_seq(),
                })
            }
            InspectorCommand::DisableConsole => {
                self.console = None;
                Ok(InspectorResponse::ConsoleDisabled)
            }
            InspectorCommand::ClearConsole => {
                port.console_clear();
                if let Some(cursor) = &mut self.console {
                    cursor.after = port.console_next_seq().saturating_sub(1);
                }
                Ok(InspectorResponse::ConsoleCleared)
            }
            InspectorCommand::GetConsoleHistory => {
                let InspectorArgs::History { after, limit } = request.args else {
                    return Err(refuse(
                        InspectorErrorCode::InvalidPayload,
                        "Missing history arguments",
                    ));
                };
                let messages = port.console_read(after, limit as usize, MAX_HISTORY_BYTES);
                Ok(InspectorResponse::ConsoleHistory {
                    next_seq: messages.last().map_or(after, |m| m.seq),
                    messages,
                    dropped: port.console_dropped(),
                })
            }
            InspectorCommand::GrantEvaluation => {
                let InspectorArgs::Grant {
                    token,
                    opaque_override,
                } = request.args
                else {
                    return Err(refuse(
                        InspectorErrorCode::InvalidPayload,
                        "Missing grant arguments",
                    ));
                };
                if !port.origin_allows_evaluation() && !opaque_override {
                    return Err(refuse(
                        InspectorErrorCode::Unauthorized,
                        "Evaluation is not permitted for an opaque or internal document",
                    ));
                }
                self.sessions.store_grant(request.session, current.0, token);
                self.opaque_override = opaque_override;
                Ok(InspectorResponse::EvaluationGranted)
            }
            InspectorCommand::RevokeEvaluation => {
                self.sessions.clear_grant();
                self.opaque_override = false;
                Ok(InspectorResponse::EvaluationRevoked)
            }
            InspectorCommand::Evaluate => {
                let InspectorArgs::Evaluate { expression, grant } = &request.args else {
                    return Err(refuse(
                        InspectorErrorCode::InvalidPayload,
                        "Missing expression",
                    ));
                };
                // Revalidated here, immediately before execution, against the *current*
                // document: nothing earlier in the pipeline is trusted.
                self.sessions
                    .check_grant(request.session, current.0, *grant)?;
                if !port.origin_allows_evaluation() && !self.opaque_override {
                    return Err(refuse(
                        InspectorErrorCode::Unauthorized,
                        "Evaluation is not permitted for an opaque or internal document",
                    ));
                }
                if current.1 {
                    return Err(refuse(
                        InspectorErrorCode::Busy,
                        "The document is still loading",
                    ));
                }
                match port.evaluate(expression) {
                    Ok(outcome) => Ok(InspectorResponse::Evaluated(Box::new(outcome))),
                    Err(message) => Err(refuse(InspectorErrorCode::Unavailable, &message)),
                }
            }
        }
    }

    /// Idempotent revocation requested by the browser. Unknown or stale ids change nothing.
    pub fn detach(&mut self, session: InspectorSessionId) {
        if self.sessions.detach(session) {
            self.end_session_state();
        }
    }

    /// Called once per loop iteration. With no live session this is one branch. Emits at most
    /// one status event per kind and a bounded console batch per call, and never queues, so
    /// a slow browser cannot make the engine accumulate anything.
    pub fn observe(
        &mut self,
        document: DocumentContext,
        loading: bool,
        port: &dyn PagePort,
    ) -> Vec<(InspectorSessionId, u64, InspectorEvent)> {
        let Some(session) = self.sessions.live() else {
            return Vec::new();
        };
        let mut events = Vec::new();
        // Document replacement ends document-bound authority and starts a fresh history.
        if let Some(previous) = self.last_document
            && previous != document
        {
            self.sessions.clear_grant();
            self.opaque_override = false;
            if let Some(cursor) = &mut self.console {
                cursor.after = port.console_first_seq().saturating_sub(1);
                events.push(InspectorEvent::ExecutionContextDestroyed { document: previous });
                events.push(InspectorEvent::ExecutionContextCreated { document });
            }
        }
        self.last_document = Some(document);
        if self.sessions.subscribed() {
            match self.baseline {
                None => self.baseline = Some((document, loading)),
                Some((last_document, last_loading)) => {
                    if document != last_document {
                        events.push(InspectorEvent::DocumentChanged { document });
                    }
                    if loading != last_loading {
                        events.push(InspectorEvent::LoadingChanged { loading });
                    }
                    self.baseline = Some((document, loading));
                }
            }
        }
        if let Some(cursor) = &mut self.console {
            let first = port.console_first_seq();
            if cursor.after.saturating_add(1) < first {
                // The ring evicted entries before they were delivered.
                events.push(InspectorEvent::ConsoleMessagesDropped {
                    count: first - 1 - cursor.after,
                });
                cursor.after = first - 1;
            }
            for message in
                port.console_read(cursor.after, CONSOLE_BATCH_ENTRIES, CONSOLE_BATCH_BYTES)
            {
                cursor.after = message.seq;
                events.push(if message.exception.is_some() {
                    InspectorEvent::ExceptionThrown(Box::new(message))
                } else {
                    InspectorEvent::ConsoleMessageAdded(Box::new(message))
                });
            }
        }
        events
            .into_iter()
            .map(|event| (session, self.sessions.next_seq(), event))
            .collect()
    }
}

#[cfg(test)]
mod tests;
