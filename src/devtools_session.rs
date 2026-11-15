//! Browser-side DevTools client: owns *which* target the panel inspects and drives the
//! session lifecycle against the page's inspector authority. Pure state machine over an
//! [`InspectorPort`], so every policy below is testable without a window or a process.
//!
//! Policy (see docs/architecture/devtools-d1-design.md §6):
//! * opening the panel attaches to the selected tab; closing it detaches (no suspended state);
//! * a docked panel follows the selection by detaching from A and attaching to B;
//! * a detached window is pinned to the tab it was opened for;
//! * a new process for the same tab re-attaches with a new session id;
//! * a snapshot is shown only if its target and document stamp match what the browser
//!   currently knows, otherwise nothing is shown.
use crate::wega_page::InspectorPoll;
use std::time::{Duration, Instant};
use wega::engine_protocol::{
    DocumentContext,
    identity::ProcessSessionId,
    inspector::{
        InspectorCommand, InspectorError, InspectorErrorCode, InspectorEvent, InspectorResponse,
        InspectorSessionId, StatusSnapshot, TargetId, session::Completed,
    },
};

/// One snapshot request per second while the panel is visible; none when it is closed.
pub(crate) const REFRESH: Duration = Duration::from_secs(1);
/// A snapshot request outstanding this long is reported as "not responding".
pub(crate) const STALL: Duration = Duration::from_millis(1500);
const RETRY: Duration = Duration::from_secs(1);
const MAX_ATTACH_RETRIES: u8 = 3;

/// What the view needs from a page. `Page` implements it; tests use a scripted fake.
pub(crate) trait InspectorPort {
    fn target(&self) -> Option<TargetId>;
    fn open(&self) -> Result<InspectorSessionId, InspectorError>;
    fn request(
        &self,
        session: InspectorSessionId,
        command: InspectorCommand,
    ) -> Result<(), InspectorError>;
    fn close(&self, session: InspectorSessionId);
    fn poll(&self, session: InspectorSessionId) -> InspectorPoll;
}

impl InspectorPort for crate::wega_page::Page {
    fn target(&self) -> Option<TargetId> {
        self.inspector_target()
    }
    fn open(&self) -> Result<InspectorSessionId, InspectorError> {
        self.inspector_open()
    }
    fn request(
        &self,
        session: InspectorSessionId,
        command: InspectorCommand,
    ) -> Result<(), InspectorError> {
        self.inspector_request(session, command)
    }
    fn close(&self, session: InspectorSessionId) {
        self.inspector_close(session)
    }
    fn poll(&self, session: InspectorSessionId) -> InspectorPoll {
        self.inspector_poll(session)
    }
}

/// What a tab key resolves to right now.
pub(crate) enum Resolved<'a> {
    /// The tab no longer exists.
    Closed,
    /// The tab exists but has no content process (internal page, unloaded, discarded).
    Unloaded,
    Page(&'a dyn InspectorPort),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ViewState {
    /// Not open.
    Idle,
    Connecting,
    /// Attached, snapshot shown and current.
    Attached,
    /// Attached, waiting for a snapshot of the current document.
    Updating,
    /// A request has been outstanding longer than `STALL`.
    NotResponding,
    /// The content process is gone or the inspector refused; reason for display.
    Unavailable(String),
    /// The target tab was closed.
    TargetClosed,
    /// The target tab has no page to inspect.
    NoPage,
}

#[derive(Debug)]
struct Live {
    process: ProcessSessionId,
    session: InspectorSessionId,
    attached: bool,
    attach_retries: u8,
    subscribed: bool,
    subscribing: bool,
    snapshot_pending_since: Option<Instant>,
    last_snapshot: Option<Instant>,
}

#[derive(Debug)]
pub(crate) struct DevtoolsView {
    open: bool,
    /// The tab key the view is bound to (a `TabId` value).
    target_tab: Option<u64>,
    pub(crate) state: ViewState,
    live: Option<Live>,
    snapshot: Option<StatusSnapshot>,
    dropped_events: u64,
    next_open: Option<Instant>,
}

impl Default for DevtoolsView {
    fn default() -> Self {
        Self {
            open: false,
            target_tab: None,
            state: ViewState::Idle,
            live: None,
            snapshot: None,
            dropped_events: 0,
            next_open: None,
        }
    }
}

fn is_transient(code: InspectorErrorCode) -> bool {
    matches!(
        code,
        InspectorErrorCode::StaleDocument
            | InspectorErrorCode::Cancelled
            | InspectorErrorCode::Timeout
            | InspectorErrorCode::Busy
    )
}

impl DevtoolsView {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }
    pub(crate) fn target_tab(&self) -> Option<u64> {
        self.target_tab
    }
    #[cfg(any(test, feature = "test-harness"))]
    pub(crate) fn session(&self) -> Option<InspectorSessionId> {
        self.live.as_ref().map(|live| live.session)
    }
    pub(crate) fn dropped_events(&self) -> u64 {
        self.dropped_events
    }
    /// Only ever returns a snapshot that passed every check in `tick`.
    pub(crate) fn snapshot(&self) -> Option<&StatusSnapshot> {
        if matches!(self.state, ViewState::Attached) {
            self.snapshot.as_ref()
        } else {
            None
        }
    }

    pub(crate) fn open(&mut self, selected_tab: u64) {
        if self.open {
            return;
        }
        self.open = true;
        self.target_tab = Some(selected_tab);
        self.state = ViewState::Connecting;
        self.next_open = None;
    }

    /// Detach and forget everything. The page is untouched.
    pub(crate) fn close<'a>(&mut self, resolve: &dyn Fn(u64) -> Resolved<'a>) {
        self.release(resolve);
        self.open = false;
        self.target_tab = None;
        self.state = ViewState::Idle;
        self.dropped_events = 0;
        self.next_open = None;
    }

    /// The tab was closed (the page, and with it the registry, is already gone).
    pub(crate) fn on_tab_closed(&mut self, key: u64) {
        if self.open && self.target_tab == Some(key) {
            self.live = None;
            self.snapshot = None;
            self.state = ViewState::TargetClosed;
        }
    }

    fn release<'a>(&mut self, resolve: &dyn Fn(u64) -> Resolved<'a>) {
        if let (Some(live), Some(tab)) = (self.live.take(), self.target_tab)
            && let Resolved::Page(page) = resolve(tab)
        {
            page.close(live.session);
        }
        self.snapshot = None;
    }

    /// Drive the state machine. Never blocks: every port call is a short mutex section plus a
    /// `try_send`. `pinned` is true for a detached window, which keeps its target while the
    /// selection moves.
    pub(crate) fn tick<'a>(
        &mut self,
        now: Instant,
        selected_tab: u64,
        pinned: bool,
        resolve: &dyn Fn(u64) -> Resolved<'a>,
    ) {
        if !self.open {
            return;
        }
        // A deliberate target switch: detach from the old target before anything else, and
        // clear what was displayed so data never appears under the wrong tab.
        if !pinned && self.target_tab != Some(selected_tab) {
            self.release(resolve);
            self.target_tab = Some(selected_tab);
            self.state = ViewState::Connecting;
            self.next_open = None;
        }
        let Some(tab) = self.target_tab else {
            return;
        };
        let page = match resolve(tab) {
            Resolved::Closed => {
                self.live = None;
                self.snapshot = None;
                self.state = ViewState::TargetClosed;
                return;
            }
            Resolved::Unloaded => {
                // Nothing to revoke: the process (and its registry) is already gone.
                self.live = None;
                self.snapshot = None;
                self.state = ViewState::NoPage;
                return;
            }
            Resolved::Page(page) => page,
        };
        let Some(target) = page.target().filter(|target| target.tab.0 == tab) else {
            self.live = None;
            self.snapshot = None;
            self.state = ViewState::Unavailable("The page has no inspector".into());
            return;
        };
        // A replacement process invalidates the old session: re-attach with a new id.
        if self
            .live
            .as_ref()
            .is_some_and(|live| live.process != target.process)
        {
            self.live = None;
            self.snapshot = None;
            self.state = ViewState::Connecting;
        }
        if self.live.is_none() {
            if self.next_open.is_some_and(|at| now < at) {
                return;
            }
            match page.open() {
                Ok(session) => {
                    self.live = Some(Live {
                        process: target.process,
                        session,
                        attached: false,
                        attach_retries: 0,
                        subscribed: false,
                        subscribing: false,
                        snapshot_pending_since: None,
                        last_snapshot: None,
                    });
                    self.snapshot = None;
                    self.state = ViewState::Connecting;
                    self.next_open = None;
                }
                Err(error) => {
                    self.state = ViewState::Unavailable(error.message);
                    self.next_open = Some(now + RETRY);
                    return;
                }
            }
        }
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let session = live.session;
        let poll = page.poll(session);
        if poll.closed {
            // Crash, kill or shutdown: this process can never be inspected again.
            self.live = None;
            self.snapshot = None;
            self.state = ViewState::Unavailable("The content process stopped".into());
            self.next_open = Some(now + RETRY);
            return;
        }
        let mut refresh = false;
        for event in poll.events {
            match event {
                InspectorEvent::DocumentChanged { .. } => {
                    self.snapshot = None;
                    refresh = true;
                }
                InspectorEvent::LoadingChanged { .. } => refresh = true,
                InspectorEvent::EventsDropped { count } => {
                    self.dropped_events = self.dropped_events.saturating_add(count);
                    refresh = true;
                }
            }
        }
        for completed in poll.results {
            self.complete(completed, target, poll.document, page, now, &mut refresh);
            if self.live.is_none() {
                return;
            }
        }
        let Some(live) = self.live.as_mut() else {
            return;
        };
        // Never keep a snapshot of a document the browser has moved past.
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.document != poll.document || snapshot.target != target)
        {
            self.snapshot = None;
            refresh = true;
        }
        if live.attached {
            if !live.subscribed && !live.subscribing {
                if page
                    .request(session, InspectorCommand::SubscribeStatus)
                    .is_ok()
                {
                    live.subscribing = true;
                }
            }
            let due = live
                .last_snapshot
                .is_none_or(|at| now.duration_since(at) >= REFRESH);
            if live.snapshot_pending_since.is_none() && (refresh || due || self.snapshot.is_none())
            {
                match page.request(session, InspectorCommand::GetStatusSnapshot) {
                    Ok(()) => live.snapshot_pending_since = Some(now),
                    Err(error) if error.code == InspectorErrorCode::Busy => {}
                    Err(error) => {
                        self.state = ViewState::Unavailable(error.message);
                        self.snapshot = None;
                        return;
                    }
                }
            }
        }
        self.state = if let Some(since) = live.snapshot_pending_since
            && now.duration_since(since) >= STALL
        {
            self.snapshot = None;
            ViewState::NotResponding
        } else if !live.attached {
            ViewState::Connecting
        } else if self.snapshot.is_some() {
            ViewState::Attached
        } else {
            ViewState::Updating
        };
    }

    fn complete(
        &mut self,
        completed: Completed,
        target: TargetId,
        document: DocumentContext,
        page: &dyn InspectorPort,
        now: Instant,
        refresh: &mut bool,
    ) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if completed.session != live.session {
            return;
        }
        match (completed.command, completed.result) {
            (InspectorCommand::Attach, Ok(InspectorResponse::Attached)) => {
                live.attached = true;
                *refresh = true;
            }
            (InspectorCommand::Attach, Err(error)) => {
                live.attach_retries += 1;
                if is_transient(error.code) && live.attach_retries <= MAX_ATTACH_RETRIES {
                    // The same session may be re-attached (the engine accepts a retry idempotently).
                    let _ = page.request(live.session, InspectorCommand::Attach);
                } else {
                    let message = error.message;
                    if let Some(live) = self.live.take() {
                        page.close(live.session);
                    }
                    self.snapshot = None;
                    self.state = ViewState::Unavailable(message);
                    self.next_open = Some(now + RETRY);
                }
            }
            (InspectorCommand::SubscribeStatus, Ok(_)) => {
                live.subscribed = true;
                live.subscribing = false;
            }
            (InspectorCommand::SubscribeStatus, Err(_)) => live.subscribing = false,
            (InspectorCommand::GetStatusSnapshot, result) => {
                live.snapshot_pending_since = None;
                match result {
                    Ok(InspectorResponse::Status(snapshot))
                        if snapshot.target == target
                            && snapshot.document == document
                            && snapshot.is_bounded() =>
                    {
                        live.last_snapshot = Some(now);
                        self.snapshot = Some(*snapshot);
                    }
                    // A stale or foreign snapshot is dropped and asked for again.
                    Ok(_) => *refresh = true,
                    Err(error) if is_transient(error.code) => {
                        live.last_snapshot = None;
                    }
                    Err(error) => {
                        let message = error.message;
                        if let Some(live) = self.live.take() {
                            page.close(live.session);
                        }
                        self.snapshot = None;
                        self.state = ViewState::Unavailable(message);
                        self.next_open = Some(now + RETRY);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "devtools_session_tests.rs"]
mod tests;
