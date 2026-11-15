//! Session ownership. The browser process owns issuance, validation and revocation
//! ([`HostSessions`]); the content process keeps a mirror ([`ContentSessions`]) that it
//! uses only to *refuse* requests, never to grant authority.
use super::{
    CONSOLE_QUEUE_BYTES, CONSOLE_QUEUE_ENTRIES, EVENT_RING, GrantToken, InspectorCommand,
    InspectorError, InspectorErrorCode, InspectorEvent, InspectorResponse, InspectorSessionId,
    RESULT_RING, TargetId, supports_inspector_version,
};
use crate::{DocumentContext, authority::Principal};
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
};

fn err(code: InspectorErrorCode, message: &str) -> InspectorError {
    InspectorError::new(code, message)
}

/// Strictly increasing, never reused. Exhaustion is an error, not a wrap.
#[derive(Debug, Default)]
pub struct SessionIssuer(AtomicU64);

impl SessionIssuer {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }
    #[cfg(test)]
    pub(crate) const fn starting_at(value: u64) -> Self {
        Self(AtomicU64::new(value))
    }
    pub fn next(&self) -> Result<InspectorSessionId, InspectorError> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map(|previous| InspectorSessionId(previous + 1))
            .map_err(|_| {
                err(
                    InspectorErrorCode::Unavailable,
                    "Inspector session identity exhausted",
                )
            })
    }
    /// One issuer for the whole browser process: ids are unique across tabs.
    pub fn shared() -> &'static SessionIssuer {
        static SHARED: SessionIssuer = SessionIssuer::new();
        &SHARED
    }
}

#[derive(Debug)]
struct Live {
    id: InspectorSessionId,
    subscribed: bool,
}

/// Browser-side authority for exactly one target (one content process).
#[derive(Debug)]
pub struct SessionRegistry {
    target: TargetId,
    live: Option<Live>,
    /// Highest id ever opened here; anything at or below it that is not live was revoked.
    last: Option<InspectorSessionId>,
    closed: bool,
}

impl SessionRegistry {
    pub fn new(target: TargetId) -> Self {
        Self {
            target,
            live: None,
            last: None,
            closed: false,
        }
    }
    pub fn target(&self) -> TargetId {
        self.target
    }
    pub fn live(&self) -> Option<InspectorSessionId> {
        self.live.as_ref().map(|live| live.id)
    }
    pub fn is_live(&self, session: InspectorSessionId) -> bool {
        !self.closed && self.live() == Some(session)
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
    /// Open a session, superseding (and returning) any existing one. The caller sends the
    /// engine an `InspectorDetach` for the returned id.
    pub fn open(
        &mut self,
        issuer: &SessionIssuer,
    ) -> Result<(InspectorSessionId, Option<InspectorSessionId>), InspectorError> {
        if self.closed {
            return Err(err(
                InspectorErrorCode::Unavailable,
                "The content process is gone",
            ));
        }
        let id = issuer.next()?;
        let superseded = self.live.replace(Live {
            id,
            subscribed: false,
        });
        self.last = Some(id);
        Ok((id, superseded.map(|live| live.id)))
    }
    pub fn authorize(
        &self,
        session: InspectorSessionId,
        target: TargetId,
    ) -> Result<(), InspectorError> {
        if self.closed {
            return Err(err(
                InspectorErrorCode::Unavailable,
                "The content process is gone",
            ));
        }
        if target != self.target {
            return Err(err(
                InspectorErrorCode::UnknownTarget,
                "Session does not own this target",
            ));
        }
        if session.0 == 0 {
            return Err(err(InspectorErrorCode::Unauthorized, "Invalid session"));
        }
        if self.live() == Some(session) {
            Ok(())
        } else if self.last.is_some_and(|last| session <= last) {
            Err(err(
                InspectorErrorCode::StaleSession,
                "Session was revoked or superseded",
            ))
        } else {
            Err(err(InspectorErrorCode::Unauthorized, "Unknown session"))
        }
    }
    pub fn revoke(&mut self, session: InspectorSessionId) -> bool {
        if self.live() == Some(session) {
            self.live = None;
            true
        } else {
            false
        }
    }
    pub fn revoke_all(&mut self) -> Option<InspectorSessionId> {
        self.live.take().map(|live| live.id)
    }
    /// The process crashed, was killed or is being replaced: nothing is ever authorised again.
    pub fn close(&mut self) -> Option<InspectorSessionId> {
        self.closed = true;
        self.revoke_all()
    }
    pub fn set_subscribed(&mut self, session: InspectorSessionId, value: bool) -> bool {
        match &mut self.live {
            Some(live) if live.id == session && !self.closed => {
                live.subscribed = value;
                true
            }
            _ => false,
        }
    }
    pub fn subscribed(&self, session: InspectorSessionId) -> bool {
        !self.closed
            && self
                .live
                .as_ref()
                .is_some_and(|live| live.id == session && live.subscribed)
    }
}

/// A bounded FIFO that drops the oldest entry on overflow and counts what it dropped.
#[derive(Debug)]
pub struct DropOldest<T> {
    items: VecDeque<T>,
    capacity: usize,
    dropped: u64,
}

impl<T> DropOldest<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            items: VecDeque::with_capacity(capacity.min(64)),
            capacity: capacity.max(1),
            dropped: 0,
        }
    }
    pub fn push(&mut self, item: T) {
        if self.items.len() == self.capacity {
            self.items.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.items.push_back(item);
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
    pub fn clear(&mut self) {
        self.items.clear();
    }
    /// Drain everything in order, returning the number of entries lost since the last drain.
    pub fn drain(&mut self) -> (Vec<T>, u64) {
        (
            self.items.drain(..).collect(),
            std::mem::take(&mut self.dropped),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Completed {
    pub session: InspectorSessionId,
    pub command: super::InspectorCommand,
    pub result: Result<InspectorResponse, InspectorError>,
}

/// Everything the browser keeps per content process for the inspector: the authority, the
/// bounded inbound queues, and the replay guard. Lives inside the page's shared state so it
/// is destroyed with the process it protects.
/// The browser's record of an evaluation grant. Never leaves the browser except as the
/// token inside `GrantEvaluation` / `Evaluate` commands on the authenticated pipe.
#[derive(Clone, Debug)]
struct HostGrant {
    session: InspectorSessionId,
    document: DocumentContext,
    token: GrantToken,
    /// The committed principal the grant was authorised for. It is compared again at every use,
    /// so a document re-bound to a different principal never inherits the grant.
    principal: Principal,
    scope: super::policy::GrantScope,
}

/// Console-stream queue bounded by entries and bytes; overflow drops the oldest and counts.
#[derive(Debug, Default)]
struct ConsoleQueue {
    items: VecDeque<(InspectorEvent, usize)>,
    bytes: usize,
    dropped: u64,
}

impl ConsoleQueue {
    fn push(&mut self, event: InspectorEvent, size: usize) {
        if size > CONSOLE_QUEUE_BYTES {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        while self.items.len() >= CONSOLE_QUEUE_ENTRIES || self.bytes + size > CONSOLE_QUEUE_BYTES {
            let Some((_, old)) = self.items.pop_front() else {
                break;
            };
            self.bytes -= old;
            self.dropped = self.dropped.saturating_add(1);
        }
        self.bytes += size;
        self.items.push_back((event, size));
    }
    fn clear(&mut self) {
        self.items.clear();
        self.bytes = 0;
        self.dropped = 0;
    }
}

#[derive(Debug)]
pub struct HostSessions {
    registry: SessionRegistry,
    events: DropOldest<InspectorEvent>,
    results: DropOldest<Completed>,
    last_seq: u64,
    console: ConsoleQueue,
    console_enabled: bool,
    grant: Option<HostGrant>,
}

impl HostSessions {
    pub fn new(target: TargetId) -> Self {
        Self {
            registry: SessionRegistry::new(target),
            events: DropOldest::new(EVENT_RING),
            results: DropOldest::new(RESULT_RING),
            last_seq: 0,
            console: ConsoleQueue::default(),
            console_enabled: false,
            grant: None,
        }
    }
    pub fn registry(&self) -> &SessionRegistry {
        &self.registry
    }
    pub fn registry_mut(&mut self) -> &mut SessionRegistry {
        &mut self.registry
    }
    /// Open a new session. Queued data of the superseded session is discarded.
    pub fn open(
        &mut self,
        issuer: &SessionIssuer,
    ) -> Result<(InspectorSessionId, Option<InspectorSessionId>), InspectorError> {
        let opened = self.registry.open(issuer)?;
        self.reset_queues();
        Ok(opened)
    }
    fn reset_queues(&mut self) {
        self.events.clear();
        let _ = self.events.drain();
        self.results.clear();
        let _ = self.results.drain();
        self.last_seq = 0;
        self.console.clear();
        self.console_enabled = false;
        // A session change can never inherit evaluation authority.
        self.grant = None;
    }

    /// Record a grant for the live session and document. The token comes from the caller's
    /// CSPRNG; it is stored here and sent to the content process, nowhere else.
    pub fn install_grant(
        &mut self,
        session: InspectorSessionId,
        document: DocumentContext,
        token: GrantToken,
        principal: Principal,
        scope: super::policy::GrantScope,
    ) -> Result<(), InspectorError> {
        if !self.registry.is_live(session) {
            return Err(err(InspectorErrorCode::StaleSession, "Session is not live"));
        }
        if token.is_zero() {
            return Err(err(InspectorErrorCode::InvalidPayload, "Empty grant token"));
        }
        self.grant = Some(HostGrant {
            session,
            document,
            token,
            principal,
            scope,
        });
        Ok(())
    }
    /// The token to put in an `Evaluate`, only if `session` is live and holds a grant for
    /// exactly this document.
    pub fn grant_for(
        &self,
        session: InspectorSessionId,
        document: DocumentContext,
        principal: &Principal,
    ) -> Option<GrantToken> {
        self.grant
            .as_ref()
            .filter(|grant| {
                self.registry.is_live(session)
                    && grant.session == session
                    && grant.document == document
                    && grant.principal == *principal
            })
            .map(|grant| grant.token)
    }
    /// For the UI indicator: the document a live grant covers and whether it overrides
    /// the opaque-origin policy.
    pub fn grant_state(&self, session: InspectorSessionId) -> Option<(DocumentContext, bool)> {
        self.grant
            .as_ref()
            .filter(|grant| grant.session == session && self.registry.is_live(session))
            .map(|grant| (grant.document, grant.scope.opaque_override))
    }
    pub fn clear_grant(&mut self) {
        self.grant = None;
    }
    /// The document was replaced: document-bound authority and history are gone.
    pub fn document_changed(&mut self) {
        self.grant = None;
        self.console.clear();
    }
    pub fn revoke(&mut self, session: InspectorSessionId) -> bool {
        let revoked = self.registry.revoke(session);
        if revoked {
            self.reset_queues();
        }
        revoked
    }
    pub fn close(&mut self) -> Option<InspectorSessionId> {
        self.reset_queues();
        self.registry.close()
    }
    /// A response for a request this host issued. Late replies for a revoked session are dropped.
    pub fn complete(
        &mut self,
        session: InspectorSessionId,
        command: super::InspectorCommand,
        result: Result<InspectorResponse, InspectorError>,
    ) -> bool {
        if !self.registry.is_live(session) {
            return false;
        }
        if let Ok(InspectorResponse::Status(snapshot)) = &result
            && (snapshot.target != self.registry.target() || !snapshot.is_bounded())
        {
            return false;
        }
        match &result {
            Ok(InspectorResponse::Evaluated(outcome)) if !outcome.is_bounded() => return false,
            Ok(InspectorResponse::ConsoleHistory { messages, .. })
                if messages.iter().any(|m| !m.is_bounded()) =>
            {
                return false;
            }
            _ => {}
        }
        match (&result, command) {
            (Ok(InspectorResponse::Subscribed), _) => {
                self.registry.set_subscribed(session, true);
            }
            (Ok(InspectorResponse::Unsubscribed), _) => {
                self.registry.set_subscribed(session, false);
            }
            (Ok(InspectorResponse::ConsoleEnabled { .. }), _) => self.console_enabled = true,
            (Ok(InspectorResponse::ConsoleDisabled), _) => self.console_enabled = false,
            (Ok(InspectorResponse::EvaluationRevoked), _) => self.grant = None,
            // A refused grant must not leave a browser-side record behind.
            (Err(_), InspectorCommand::GrantEvaluation) => self.grant = None,
            _ => {}
        }
        self.results.push(Completed {
            session,
            command,
            result,
        });
        true
    }
    /// A pushed event. Dropped when the session is not live, not subscribed, or `seq` is a replay.
    pub fn event(&mut self, session: InspectorSessionId, seq: u64, event: InspectorEvent) -> bool {
        if event.is_console() {
            return self.console_event(session, seq, event);
        }
        if !self.registry.subscribed(session) || seq <= self.last_seq {
            return false;
        }
        self.last_seq = seq;
        self.events.push(event);
        true
    }
    fn console_event(
        &mut self,
        session: InspectorSessionId,
        seq: u64,
        event: InspectorEvent,
    ) -> bool {
        if !self.registry.is_live(session) || !self.console_enabled || seq <= self.last_seq {
            return false;
        }
        self.last_seq = seq;
        let size = match &event {
            InspectorEvent::ConsoleMessageAdded(message)
            | InspectorEvent::ExceptionThrown(message) => {
                if !message.is_bounded() {
                    self.console.dropped = self.console.dropped.saturating_add(1);
                    return false;
                }
                message.approx_bytes()
            }
            // Small fixed-size markers.
            _ => 64,
        };
        self.console.push(event, size);
        true
    }
    /// Console-stream events in order; a loss is reported in-band before the survivors.
    pub fn take_console(&mut self, session: InspectorSessionId) -> Vec<InspectorEvent> {
        if !self.registry.is_live(session) {
            return Vec::new();
        }
        let dropped = std::mem::take(&mut self.console.dropped);
        let mut events: Vec<InspectorEvent> = self
            .console
            .items
            .drain(..)
            .map(|(event, _)| event)
            .collect();
        self.console.bytes = 0;
        if dropped > 0 {
            events.insert(0, InspectorEvent::ConsoleMessagesDropped { count: dropped });
        }
        events
    }
    pub fn console_queued(&self) -> (usize, usize) {
        (self.console.items.len(), self.console.bytes)
    }
    pub fn console_enabled(&self) -> bool {
        self.console_enabled
    }
    /// Events in order. An overflow is reported in-band, before the surviving events.
    pub fn take_events(&mut self, session: InspectorSessionId) -> Vec<InspectorEvent> {
        if !self.registry.is_live(session) {
            return Vec::new();
        }
        let (mut events, dropped) = self.events.drain();
        if dropped > 0 {
            events.insert(0, InspectorEvent::EventsDropped { count: dropped });
        }
        events
    }
    pub fn take_results(&mut self, session: InspectorSessionId) -> Vec<Completed> {
        if !self.registry.is_live(session) {
            return Vec::new();
        }
        let (results, dropped) = self.results.drain();
        let _ = dropped;
        results
    }
    pub fn queued(&self) -> (usize, usize) {
        (self.events.len(), self.results.len())
    }
}

/// The content process's mirror: at most one live session, rejecting anything older or unknown.
#[derive(Debug, Default)]
pub struct ContentSessions {
    live: Option<InspectorSessionId>,
    subscribed: bool,
    highest: u64,
    seq: u64,
    grant: Option<(InspectorSessionId, DocumentContext, GrantToken)>,
}

impl ContentSessions {
    pub fn live(&self) -> Option<InspectorSessionId> {
        self.live
    }
    pub fn subscribed(&self) -> bool {
        self.live.is_some() && self.subscribed
    }
    /// Validate an inspector request against the content process's own identity.
    /// `Attach` registers (or idempotently confirms) a session; everything else needs it live.
    pub fn authorize(
        &mut self,
        request: &super::InspectorRequest,
        own: TargetId,
    ) -> Result<(), InspectorError> {
        if !supports_inspector_version(request.version) {
            return Err(err(
                InspectorErrorCode::UnsupportedVersion,
                "Unsupported inspector version",
            ));
        }
        if request.session.0 == 0 {
            return Err(err(InspectorErrorCode::Unauthorized, "Invalid session"));
        }
        if request.target != own {
            return Err(err(
                InspectorErrorCode::UnknownTarget,
                "Request is addressed to another target",
            ));
        }
        let id = request.session;
        if request.command == super::InspectorCommand::Attach {
            if self.live == Some(id) {
                return Ok(());
            }
            if id.0 <= self.highest {
                return Err(err(
                    InspectorErrorCode::StaleSession,
                    "Attach replays a revoked session",
                ));
            }
            // The browser revoked any previous session before issuing this one.
            self.live = Some(id);
            self.subscribed = false;
            self.highest = id.0;
            self.seq = 0;
            self.grant = None;
            return Ok(());
        }
        if self.live == Some(id) {
            Ok(())
        } else if id.0 <= self.highest {
            Err(err(
                InspectorErrorCode::StaleSession,
                "Session was revoked or superseded",
            ))
        } else {
            Err(err(
                InspectorErrorCode::Unauthorized,
                "Session never attached",
            ))
        }
    }
    pub fn set_subscribed(&mut self, value: bool) {
        self.subscribed = value;
    }
    /// Idempotent; a stale or unknown id changes nothing.
    pub fn detach(&mut self, session: InspectorSessionId) -> bool {
        if self.live == Some(session) {
            self.live = None;
            self.subscribed = false;
            self.grant = None;
            true
        } else {
            false
        }
    }
    /// Remember the browser's grant for the live session and the current document.
    pub fn store_grant(
        &mut self,
        session: InspectorSessionId,
        document: DocumentContext,
        token: GrantToken,
    ) {
        if self.live == Some(session) && !token.is_zero() {
            self.grant = Some((session, document, token));
        }
    }
    pub fn clear_grant(&mut self) {
        self.grant = None;
    }
    pub fn has_grant(&self) -> bool {
        self.grant.is_some()
    }
    /// Independent verification before any evaluation: the stored grant must exist, belong
    /// to the live session, carry the same token and cover the *current* document.
    pub fn check_grant(
        &self,
        session: InspectorSessionId,
        current: DocumentContext,
        token: GrantToken,
    ) -> Result<(), InspectorError> {
        let Some((owner, document, stored)) = self.grant else {
            return Err(err(
                InspectorErrorCode::Unauthorized,
                "Evaluation has not been authorised",
            ));
        };
        if self.live != Some(session) || owner != session {
            return Err(err(
                InspectorErrorCode::StaleSession,
                "The grant belongs to another session",
            ));
        }
        if stored != token {
            return Err(err(
                InspectorErrorCode::Unauthorized,
                "Invalid evaluation grant",
            ));
        }
        if document != current {
            return Err(err(
                InspectorErrorCode::StaleDocument,
                "The grant covers a previous document",
            ));
        }
        Ok(())
    }
    pub fn next_seq(&mut self) -> u64 {
        self.seq = self.seq.saturating_add(1);
        self.seq
    }
}
