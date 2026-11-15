//! Content-side inspector backend. It answers the closed `wega_protocol::inspector`
//! command set from facts the view already maintains. It names no layout, style or script
//! engine type and runs nothing in the page: a status request never forces layout or script.
use wega_protocol::{
    DocumentContext,
    inspector::{
        INSPECTOR_VERSION, InspectorCommand, InspectorError, InspectorEvent, InspectorRequest,
        InspectorResponse, InspectorSessionId, StatusSnapshot, TargetId, TargetInfo, Viewport,
        session::ContentSessions,
    },
};

/// Everything a snapshot needs, gathered by the caller from the view only when a snapshot
/// was actually requested.
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

#[derive(Debug)]
pub struct Backend {
    own: TargetId,
    protocol_version: u32,
    sessions: ContentSessions,
    baseline: Option<(DocumentContext, bool)>,
}

fn engine_name() -> String {
    format!("wega-engine {}", env!("CARGO_PKG_VERSION"))
}

impl Backend {
    pub fn new(own: TargetId, protocol_version: u32) -> Self {
        Self {
            own,
            protocol_version,
            sessions: ContentSessions::default(),
            baseline: None,
        }
    }

    pub fn live(&self) -> Option<InspectorSessionId> {
        self.sessions.live()
    }

    /// Execute a decoded request. `current` is the document stamp and loading flag right now;
    /// `status` is called only for commands that need a snapshot.
    pub fn handle(
        &mut self,
        request: &InspectorRequest,
        current: (DocumentContext, bool),
        status: impl FnOnce() -> StatusInput,
    ) -> Result<InspectorResponse, InspectorError> {
        self.sessions.authorize(request, self.own)?;
        match request.command {
            InspectorCommand::Attach => Ok(InspectorResponse::Attached),
            InspectorCommand::Detach => {
                self.sessions.detach(request.session);
                self.baseline = None;
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
                let input = status();
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
        }
    }

    /// Idempotent revocation requested by the browser. Unknown or stale ids change nothing.
    pub fn detach(&mut self, session: InspectorSessionId) {
        if self.sessions.detach(session) {
            self.baseline = None;
        }
    }

    /// Called once per loop iteration. With no subscribed session this is one branch.
    /// Emits at most one event per kind per call and never queues: a slow browser cannot
    /// make the engine accumulate anything.
    pub fn observe(
        &mut self,
        document: DocumentContext,
        loading: bool,
    ) -> Vec<(InspectorSessionId, u64, InspectorEvent)> {
        let Some(session) = self.sessions.live().filter(|_| self.sessions.subscribed()) else {
            return Vec::new();
        };
        let Some((last_document, last_loading)) = self.baseline else {
            self.baseline = Some((document, loading));
            return Vec::new();
        };
        let mut events = Vec::new();
        if document != last_document {
            events.push(InspectorEvent::DocumentChanged { document });
        }
        if loading != last_loading {
            events.push(InspectorEvent::LoadingChanged { loading });
        }
        self.baseline = Some((document, loading));
        events
            .into_iter()
            .map(|event| (session, self.sessions.next_seq(), event))
            .collect()
    }
}

#[cfg(test)]
mod tests;
