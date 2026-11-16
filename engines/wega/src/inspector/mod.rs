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
        TargetInfo, Viewport,
        policy::{self, GrantScope},
        session::ContentSessions,
    },
};
pub mod dom;
pub mod geometry;
pub mod mutate;
pub mod styles;

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
    /// `wega_dom::MutationClock::mutation_epoch` of the document, as a plain integer.
    pub mutation_epoch: u64,
    /// `wega_dom::layout::GeometryAccess::layout_pending`.
    pub layout_pending: bool,
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
    fn inspect_style(
        &self,
        scope: wega_protocol::inspector::styles::StyleScope,
        query: &wega_protocol::inspector::styles::StyleQuery,
    ) -> Result<wega_protocol::inspector::styles::StylePage, InspectorError> {
        let _ = (scope, query);
        Err(refuse(
            InspectorErrorCode::Unavailable,
            "Style inspection unavailable",
        ))
    }
    fn inspect_dom(
        &self,
        document: DocumentContext,
        command: InspectorCommand,
        args: &InspectorArgs,
    ) -> Result<wega_protocol::inspector::dom::DomPage, InspectorError> {
        let _ = (document, command, args);
        Err(refuse(
            InspectorErrorCode::Unavailable,
            "DOM inspection unavailable",
        ))
    }
    /// Cached-only geometry of a node (never flushes layout).
    fn inspect_geometry(
        &self,
        document: DocumentContext,
        node: wega_protocol::inspector::dom::InspectorNodeId,
    ) -> Result<wega_protocol::inspector::geometry::GeometryReport, InspectorError> {
        let _ = (document, node);
        Err(refuse(
            InspectorErrorCode::Unavailable,
            "Geometry inspection unavailable",
        ))
    }
    /// Hit test at a viewport CSS point against cached layout.
    fn pick_node(
        &self,
        document: DocumentContext,
        x: f32,
        y: f32,
    ) -> Result<wega_protocol::inspector::geometry::PickReport, InspectorError> {
        let _ = (document, x, y);
        Err(refuse(
            InspectorErrorCode::Unavailable,
            "Picking unavailable",
        ))
    }
    /// Apply one already authorized edit. The caller has checked the mutation grant, the scope
    /// and the document stamp; the port checks the epoch, validates against the document and
    /// applies through the page's own mutation path.
    fn apply_edit(
        &mut self,
        document: DocumentContext,
        node: wega_protocol::inspector::dom::InspectorNodeId,
        epoch: u64,
        op: &wega_protocol::inspector::edit::MutationOp,
        allow_code: bool,
    ) -> Result<wega_protocol::inspector::edit::MutationOutcome, InspectorError> {
        let _ = (document, node, epoch, op, allow_code);
        Err(refuse(
            InspectorErrorCode::Unavailable,
            "Editing unavailable",
        ))
    }
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
    /// The tuple origin of this document as the engine itself knows it (`http`/`https` only,
    /// the same rule the broker applies to commits), or `None`. Used to check the browser's
    /// claim for consistency; it is not an authority on the document's principal.
    fn document_origin(&self) -> Option<String>;
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
    /// What the live grant covers. A scope without a matching grant is never used: the grant
    /// check fails first, and the next grant replaces it.
    scope: Option<GrantScope>,
    /// What the live **mutation** grant covers (same rule as `scope`, separate authority).
    mutation_scope: Option<GrantScope>,
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
            scope: None,
            mutation_scope: None,
        }
    }

    pub fn live(&self) -> Option<InspectorSessionId> {
        self.sessions.live()
    }

    fn end_session_state(&mut self) {
        self.baseline = None;
        self.console = None;
        self.last_document = None;
        self.scope = None;
        self.mutation_scope = None;
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
            InspectorCommand::InspectStyle => {
                let InspectorArgs::Style { query } = &request.args else {
                    return Err(refuse(
                        InspectorErrorCode::InvalidPayload,
                        "Missing style query",
                    ));
                };
                if current.1 {
                    return Err(refuse(InspectorErrorCode::Busy, "Document is loading"));
                }
                port.inspect_style(
                    wega_protocol::inspector::styles::StyleScope {
                        target: self.own,
                        session: request.session,
                        document: current.0,
                        epoch: 0,
                        generation: 0,
                    },
                    query,
                )
                .map(|p| InspectorResponse::Style(Box::new(p)))
            }
            InspectorCommand::GetDomRoot
            | InspectorCommand::GetDomChildren
            | InspectorCommand::DescribeDomNode => {
                if current.1 {
                    return Err(refuse(InspectorErrorCode::Busy, "Document is loading"));
                }
                port.inspect_dom(current.0, request.command, &request.args)
                    .map(|page| InspectorResponse::Dom(Box::new(page)))
            }
            InspectorCommand::GetGeometry => {
                let InspectorArgs::DomNode { node } = &request.args else {
                    return Err(refuse(InspectorErrorCode::InvalidPayload, "Missing node"));
                };
                if current.1 {
                    return Err(refuse(InspectorErrorCode::Busy, "Document is loading"));
                }
                port.inspect_geometry(current.0, *node)
                    .map(|report| InspectorResponse::Geometry(Box::new(report)))
            }
            InspectorCommand::PickNode => {
                let InspectorArgs::Pick { x_milli, y_milli } = &request.args else {
                    return Err(refuse(
                        InspectorErrorCode::InvalidPayload,
                        "Missing position",
                    ));
                };
                if current.1 {
                    return Err(refuse(InspectorErrorCode::Busy, "Document is loading"));
                }
                port.pick_node(
                    current.0,
                    wega_protocol::inspector::geometry::from_milli(*x_milli),
                    wega_protocol::inspector::geometry::from_milli(*y_milli),
                )
                .map(|report| InspectorResponse::Picked(Box::new(report)))
            }
            InspectorCommand::GrantMutation => {
                let InspectorArgs::Grant {
                    token,
                    opaque_override,
                    origin,
                } = &request.args
                else {
                    return Err(refuse(
                        InspectorErrorCode::InvalidPayload,
                        "Missing grant arguments",
                    ));
                };
                let scope = policy::verify_scope(
                    origin.as_deref(),
                    *opaque_override,
                    port.document_origin().as_deref(),
                )?;
                self.sessions
                    .store_mutation_grant(request.session, current.0, *token);
                self.mutation_scope = Some(scope);
                Ok(InspectorResponse::MutationGranted)
            }
            InspectorCommand::RevokeMutation => {
                self.sessions.clear_mutation_grant();
                self.mutation_scope = None;
                Ok(InspectorResponse::MutationRevoked)
            }
            InspectorCommand::Mutate => {
                let InspectorArgs::Mutate {
                    grant,
                    node,
                    epoch,
                    op,
                } = &request.args
                else {
                    return Err(refuse(
                        InspectorErrorCode::InvalidPayload,
                        "Missing edit arguments",
                    ));
                };
                // Revalidated immediately before applying, against the *current* document.
                if let Err(error) =
                    self.sessions
                        .check_mutation_grant(request.session, current.0, *grant)
                {
                    if error.code == InspectorErrorCode::StaleDocument {
                        self.sessions.clear_mutation_grant();
                        self.mutation_scope = None;
                    }
                    return Err(error);
                }
                let Some(scope) = &self.mutation_scope else {
                    return Err(refuse(
                        InspectorErrorCode::Unauthorized,
                        "Editing has no recorded scope",
                    ));
                };
                policy::verify_scope(
                    scope.origin.as_deref(),
                    scope.opaque_override,
                    port.document_origin().as_deref(),
                )?;
                if current.1 {
                    return Err(refuse(InspectorErrorCode::Busy, "Document is loading"));
                }
                // Edits that run code need the evaluation grant for this same session and
                // document as well: a mutation grant alone never executes anything.
                let allow_code = self
                    .sessions
                    .evaluation_live_for(request.session, current.0)
                    && self.scope.is_some();
                port.apply_edit(current.0, *node, *epoch, op, allow_code)
                    .map(|outcome| InspectorResponse::Mutated(Box::new(outcome)))
            }
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
                        mutation_epoch: input.mutation_epoch,
                        layout_pending: input.layout_pending,
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
                    origin,
                } = &request.args
                else {
                    return Err(refuse(
                        InspectorErrorCode::InvalidPayload,
                        "Missing grant arguments",
                    ));
                };
                // The browser decided from the committed principal; the engine only refuses a
                // claim that contradicts its own document.
                let scope = policy::verify_scope(
                    origin.as_deref(),
                    *opaque_override,
                    port.document_origin().as_deref(),
                )?;
                self.sessions
                    .store_grant(request.session, current.0, *token);
                self.scope = Some(scope);
                Ok(InspectorResponse::EvaluationGranted)
            }
            InspectorCommand::RevokeEvaluation => {
                self.sessions.clear_grant();
                self.scope = None;
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
                // ...and the scope it was granted for must still match this document.
                let Some(scope) = &self.scope else {
                    return Err(refuse(
                        InspectorErrorCode::Unauthorized,
                        "Evaluation has no recorded scope",
                    ));
                };
                policy::verify_scope(
                    scope.origin.as_deref(),
                    scope.opaque_override,
                    port.document_origin().as_deref(),
                )?;
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

    /// Whether console entries are waiting beyond the delivery cursor. The engine loop uses it to
    /// come back soon (instead of sleeping until the next unrelated event) while still doing at
    /// most one budgeted batch per iteration.
    pub fn has_backlog(&self, port: &dyn PagePort) -> bool {
        self.sessions.live().is_some()
            && self
                .console
                .as_ref()
                .is_some_and(|cursor| cursor.after.saturating_add(1) < port.console_next_seq())
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
            self.scope = None;
            self.sessions.clear_mutation_grant();
            self.mutation_scope = None;
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
