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
use crate::{
    devtools_console::{ConsoleLine, ConsoleView},
    devtools_geometry::{EditView, GeometryView},
    wega_page::InspectorPoll,
};
use std::time::{Duration, Instant};
use wega::engine_protocol::{
    DocumentContext,
    identity::ProcessSessionId,
    inspector::{
        EvalOutcome, InspectorArgs, InspectorCommand, InspectorError, InspectorErrorCode,
        InspectorEvent, InspectorResponse, InspectorSessionId, StatusSnapshot, TargetId,
        session::Completed,
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
    fn request_args(
        &self,
        session: InspectorSessionId,
        command: InspectorCommand,
        args: InspectorArgs,
    ) -> Result<(), InspectorError> {
        let _ = (session, command, args);
        Err(InspectorError::new(
            InspectorErrorCode::Unavailable,
            "DOM unavailable",
        ))
    }
    fn target(&self) -> Option<TargetId>;
    fn open(&self) -> Result<InspectorSessionId, InspectorError>;
    fn request(
        &self,
        session: InspectorSessionId,
        command: InspectorCommand,
    ) -> Result<(), InspectorError>;
    fn close(&self, session: InspectorSessionId);
    fn poll(&self, session: InspectorSessionId) -> InspectorPoll;
    /// Mint and send an evaluation grant (a trusted user action only).
    fn grant(
        &self,
        session: InspectorSessionId,
        opaque_override: bool,
    ) -> Result<(), InspectorError>;
    fn revoke_evaluation(&self, session: InspectorSessionId);
    fn evaluate(&self, session: InspectorSessionId, expression: &str)
    -> Result<(), InspectorError>;
    fn request_args_for_frame(
        &self,
        session: InspectorSessionId,
        command: InspectorCommand,
        args: InspectorArgs,
        frame: Option<wega::engine_protocol::identity::FrameId>,
    ) -> Result<(), InspectorError> {
        let _ = frame;
        self.request_args(session, command, args)
    }
    fn frames(&self) -> Vec<wega::engine_protocol::inspector::FrameDescription> {
        Vec::new()
    }
    /// Mint and send a **mutation** grant (a trusted user action only).
    fn grant_mutation(
        &self,
        session: InspectorSessionId,
        opaque_override: bool,
    ) -> Result<(), InspectorError> {
        let _ = (session, opaque_override);
        Err(InspectorError::new(
            InspectorErrorCode::Unavailable,
            "Editing unavailable",
        ))
    }
    fn grant_mutation_for_frame(
        &self,
        session: InspectorSessionId,
        opaque_override: bool,
        frame: Option<wega::engine_protocol::identity::FrameId>,
    ) -> Result<(), InspectorError> {
        let _ = frame;
        self.grant_mutation(session, opaque_override)
    }
    fn revoke_mutation(&self, session: InspectorSessionId) {
        let _ = session;
    }
    fn mutate(
        &self,
        session: InspectorSessionId,
        node: wega::engine_protocol::inspector::dom::InspectorNodeId,
        epoch: u64,
        op: wega::engine_protocol::inspector::edit::MutationOp,
    ) -> Result<(), InspectorError> {
        let _ = (session, node, epoch, op);
        Err(InspectorError::new(
            InspectorErrorCode::Unavailable,
            "Editing unavailable",
        ))
    }
    fn mutate_for_frame(
        &self,
        session: InspectorSessionId,
        node: wega::engine_protocol::inspector::dom::InspectorNodeId,
        epoch: u64,
        op: wega::engine_protocol::inspector::edit::MutationOp,
        frame: Option<wega::engine_protocol::identity::FrameId>,
    ) -> Result<(), InspectorError> {
        let _ = frame;
        self.mutate(session, node, epoch, op)
    }
}

impl InspectorPort for crate::wega_page::Page {
    fn request_args(
        &self,
        session: InspectorSessionId,
        command: InspectorCommand,
        args: InspectorArgs,
    ) -> Result<(), InspectorError> {
        self.inspector_request_args(session, command, args)
    }
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
    fn grant(
        &self,
        session: InspectorSessionId,
        opaque_override: bool,
    ) -> Result<(), InspectorError> {
        self.inspector_grant(session, opaque_override)
    }
    fn revoke_evaluation(&self, session: InspectorSessionId) {
        self.inspector_revoke_evaluation(session)
    }
    fn evaluate(
        &self,
        session: InspectorSessionId,
        expression: &str,
    ) -> Result<(), InspectorError> {
        self.inspector_evaluate(session, expression)
    }
    fn grant_mutation(
        &self,
        session: InspectorSessionId,
        opaque_override: bool,
    ) -> Result<(), InspectorError> {
        self.inspector_grant_mutation(session, opaque_override)
    }
    fn revoke_mutation(&self, session: InspectorSessionId) {
        self.inspector_revoke_mutation(session)
    }
    fn mutate(
        &self,
        session: InspectorSessionId,
        node: wega::engine_protocol::inspector::dom::InspectorNodeId,
        epoch: u64,
        op: wega::engine_protocol::inspector::edit::MutationOp,
    ) -> Result<(), InspectorError> {
        self.inspector_mutate(session, node, epoch, op)
    }
    fn request_args_for_frame(
        &self,
        session: InspectorSessionId,
        command: InspectorCommand,
        args: InspectorArgs,
        frame: Option<wega::engine_protocol::identity::FrameId>,
    ) -> Result<(), InspectorError> {
        self.inspector_request_args_for_frame(session, command, args, frame)
    }
    fn frames(&self) -> Vec<wega::engine_protocol::inspector::FrameDescription> {
        self.frame_tree()
    }
    fn grant_mutation_for_frame(
        &self,
        session: InspectorSessionId,
        opaque_override: bool,
        frame: Option<wega::engine_protocol::identity::FrameId>,
    ) -> Result<(), InspectorError> {
        self.inspector_grant_mutation_for_frame(session, opaque_override, frame)
    }
    fn mutate_for_frame(
        &self,
        session: InspectorSessionId,
        node: wega::engine_protocol::inspector::dom::InspectorNodeId,
        epoch: u64,
        op: wega::engine_protocol::inspector::edit::MutationOp,
        frame: Option<wega::engine_protocol::identity::FrameId>,
    ) -> Result<(), InspectorError> {
        self.inspector_mutate_for_frame(session, node, epoch, op, frame)
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
    console_requested: bool,
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
    pub(crate) console: ConsoleView,
    pub(crate) elements: crate::devtools_elements::ElementsView,
    pub(crate) elements_active: bool,
    pub(crate) styles: crate::devtools_styles::StyleView,
    pub(crate) geometry: GeometryView,
    pub(crate) edit: EditView,
    pub(crate) target_frame: Option<wega::engine_protocol::identity::FrameId>,
    pub(crate) frames_cache: Vec<wega::engine_protocol::inspector::FrameDescription>,
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
            console: ConsoleView::default(),
            elements: Default::default(),
            elements_active: false,
            styles: Default::default(),
            geometry: Default::default(),
            edit: Default::default(),
            target_frame: None,
            frames_cache: Vec::new(),
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
        self.session_id()
    }
    pub(crate) fn session_id(&self) -> Option<InspectorSessionId> {
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
        self.target_frame = None;
        self.frames_cache.clear();
        self.state = ViewState::Idle;
        self.dropped_events = 0;
        self.next_open = None;
    }

    /// The tab was closed (the page, and with it the registry, is already gone).
    pub(crate) fn on_tab_closed(&mut self, key: u64) {
        if self.open && self.target_tab == Some(key) {
            self.live = None;
            self.console.reset_session();
            self.elements.reset();
            self.styles.reset();
            self.geometry.reset();
            self.edit.reset();
            self.snapshot = None;
            self.target_frame = None;
            self.frames_cache.clear();
            self.state = ViewState::TargetClosed;
        }
    }

    fn live_page<'a>(
        &self,
        resolve: &dyn Fn(u64) -> Resolved<'a>,
    ) -> Option<(&'a dyn InspectorPort, InspectorSessionId)> {
        let live = self.live.as_ref().filter(|live| live.attached)?;
        match resolve(self.target_tab?) {
            Resolved::Page(page) => Some((page, live.session)),
            _ => None,
        }
    }

    /// The user's explicit action: authorise evaluation for the current document of the bound
    /// tab. `opaque_override` is the user's separate confirmation for an opaque or internal page.
    pub(crate) fn enable_evaluation<'a>(
        &mut self,
        opaque_override: bool,
        resolve: &dyn Fn(u64) -> Resolved<'a>,
    ) {
        let Some((page, session)) = self.live_page(resolve) else {
            self.console.notice("The page is not connected.");
            return;
        };
        if let Err(error) = page.grant(session, opaque_override) {
            self.console
                .notice(format!("Evaluation was not enabled: {}", error.message));
        }
    }

    /// The user's explicit action: authorise **editing** for the current document of the bound
    /// tab. Separate from evaluation; the opaque override is a separate confirmation too.
    pub(crate) fn select_frame<'a>(
        &mut self,
        frame: Option<wega::engine_protocol::identity::FrameId>,
        resolve: &dyn Fn(u64) -> Resolved<'a>,
    ) {
        if self.target_frame == frame {
            return;
        }
        self.revoke_editing(resolve);
        self.elements.reset();
        self.styles.reset();
        self.geometry.reset();
        self.target_frame = frame;
        if let Some((page, session)) = self.live_page(resolve) {
            let _ = page.request_args_for_frame(
                session,
                InspectorCommand::GetDomRoot,
                InspectorArgs::None,
                self.target_frame,
            );
        }
    }

    pub(crate) fn enable_editing<'a>(
        &mut self,
        opaque_override: bool,
        resolve: &dyn Fn(u64) -> Resolved<'a>,
    ) {
        let Some((page, session)) = self.live_page(resolve) else {
            self.edit.notice = "The page is not connected.".into();
            return;
        };
        if let Err(error) =
            page.grant_mutation_for_frame(session, opaque_override, self.target_frame)
        {
            self.edit.notice = format!("Editing was not enabled: {}", error.message);
        }
    }
    pub(crate) fn revoke_editing<'a>(&mut self, resolve: &dyn Fn(u64) -> Resolved<'a>) {
        if let Some((page, session)) = self.live_page(resolve) {
            page.revoke_mutation(session);
        }
        self.edit.enabled = None;
        self.edit.reset_queue();
    }
    pub(crate) fn revoke_evaluation<'a>(&mut self, resolve: &dyn Fn(u64) -> Resolved<'a>) {
        if let Some((page, session)) = self.live_page(resolve) {
            page.revoke_evaluation(session);
        }
        self.console.grant = None;
    }

    /// Run a command in the page. Refused here (and again by the page and the engine) unless a
    /// grant for this session and document is live.
    pub(crate) fn run_command<'a>(
        &mut self,
        now: Instant,
        expression: &str,
        resolve: &dyn Fn(u64) -> Resolved<'a>,
    ) {
        let expression = expression.trim();
        if expression.is_empty() {
            return;
        }
        if self.console.pending.is_some() {
            self.console.notice("An evaluation is already running.");
            return;
        }
        let Some((page, session)) = self.live_page(resolve) else {
            self.console
                .notice("The page is not connected; nothing was run.");
            return;
        };
        self.console.remember_command(expression);
        self.console
            .push(ConsoleLine::Input(expression.to_string()));
        match page.evaluate(session, expression) {
            Ok(()) => self.console.pending = Some(now),
            Err(error) => self.console.notice(match error.code {
                InspectorErrorCode::Unauthorized => {
                    "Evaluation is not enabled for this document. Use \"Enable evaluation\" first."
                        .to_string()
                }
                _ => format!("Not run: {}", error.message),
            }),
        }
    }

    fn release<'a>(&mut self, resolve: &dyn Fn(u64) -> Resolved<'a>) {
        if let (Some(live), Some(tab)) = (self.live.take(), self.target_tab)
            && let Resolved::Page(page) = resolve(tab)
        {
            page.close(live.session);
        }
        self.console.reset_session();
        self.elements.reset();
        self.styles.reset();
        self.geometry.reset();
        self.edit.reset();
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
                self.console.reset_session();
                self.elements.reset();
                self.styles.reset();
                self.geometry.reset();
                self.edit.reset();
                self.snapshot = None;
                self.state = ViewState::TargetClosed;
                return;
            }
            Resolved::Unloaded => {
                // Nothing to revoke: the process (and its registry) is already gone.
                self.live = None;
                self.console.reset_session();
                self.elements.reset();
                self.styles.reset();
                self.geometry.reset();
                self.edit.reset();
                self.snapshot = None;
                self.state = ViewState::NoPage;
                return;
            }
            Resolved::Page(page) => page,
        };
        let Some(target) = page.target().filter(|target| target.tab.0 == tab) else {
            self.live = None;
            self.console.reset_session();
            self.elements.reset();
            self.styles.reset();
            self.geometry.reset();
            self.edit.reset();
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
            self.console.reset_session();
            self.elements.reset();
            self.styles.reset();
            self.geometry.reset();
            self.edit.reset();
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
                        console_requested: false,
                    });
                    self.console.reset_session();
                    self.elements.reset();
                    self.styles.reset();
                    self.geometry.reset();
                    self.edit.reset();
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
        self.frames_cache = page.frames();
        if let Some(target_frame) = self.target_frame {
            if !self.frames_cache.iter().any(|f| f.key == target_frame.0) {
                self.select_frame(None, resolve);
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
            self.console.reset_session();
            self.elements.reset();
            self.styles.reset();
            self.geometry.reset();
            self.edit.reset();
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
                // Console-stream events arrive through `poll.console`.
                _ => {}
            }
        }
        self.console.new_document(poll.document);
        self.elements.document(poll.document);
        self.elements.expire(now);
        self.geometry.document(poll.document);
        self.geometry.expire(now);
        self.edit.document(poll.document);
        // The browser's own record of the mutation grant, never the engine's word.
        self.edit.observe_grant(poll.mutation, poll.document);
        self.edit.expire(now);
        if !self.elements_active {
            // Picking is an Elements action: leaving the panel ends it.
            self.geometry.set_picker(false);
        }
        for event in poll.console {
            match event {
                InspectorEvent::ConsoleMessageAdded(message)
                | InspectorEvent::ExceptionThrown(message) => {
                    self.console.push(ConsoleLine::Message(message));
                }
                InspectorEvent::ConsoleMessagesDropped { count } => {
                    self.console.dropped = self.console.dropped.saturating_add(count);
                }
                InspectorEvent::ExecutionContextCreated { document } => {
                    self.console.new_document(document);
                }
                _ => {}
            }
        }
        // The browser's own record, never the engine's word.
        self.console.grant = poll.grant;
        for completed in poll.results {
            self.complete(completed, target, poll.document, page, now, &mut refresh);
            if self.live.is_none() {
                return;
            }
        }
        if let Some(pick) = self.geometry.take_chosen() {
            self.elements.reveal(&pick.ancestors, pick.target.node);
        }
        if let Some(removed) = self.edit.take_removed() {
            self.elements.node_removed(removed);
        }
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if let Some(epoch) = self.edit.take_adopted_epoch() {
            // The edit's own epoch: pages and the selection refresh through the normal path,
            // one page at a time; no cache is cleared.
            self.elements.status_changed(poll.document, epoch);
            live.last_snapshot = None;
            refresh = true;
        }
        // Never keep a snapshot of a document the browser has moved past.
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.document != poll.document || snapshot.target != target)
        {
            self.snapshot = None;
            refresh = true;
        }
        if live.attached && !live.console_requested {
            // Console history exists whether or not anyone looks, so enabling is just a cursor:
            // the engine backfills what was logged before this panel opened.
            if page
                .request(session, InspectorCommand::EnableConsole)
                .is_ok()
            {
                live.console_requested = true;
            }
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
        if self.elements_active && self.state == ViewState::Attached {
            if let Some(read) = self.elements.next() {
                let (command, args) = read.request();
                match page.request_args_for_frame(session, command, args, self.target_frame) {
                    Ok(()) => self.elements.begin(read, now),
                    Err(error) if error.code == InspectorErrorCode::Busy => {}
                    Err(error) => self.elements.notice = error.message,
                }
            }
        }
        if self.elements_active && self.state == ViewState::Attached {
            // Geometry and picking: one outstanding request, pointer movement coalesced.
            if let Some((command, args)) =
                self.geometry
                    .next(now, self.elements.selected, self.elements.epoch)
            {
                match page.request_args_for_frame(session, command, args, self.target_frame) {
                    Ok(()) => {}
                    Err(error) => self.geometry.complete(command, Err(error)),
                }
            }
            // Edits: serial, against the epoch the user is looking at.
            if let Some(epoch) = self.elements.epoch
                && let Some(queued) = self.edit.next(now)
            {
                if let Err(error) = page.mutate_for_frame(
                    session,
                    queued.node,
                    epoch,
                    queued.op.clone(),
                    self.target_frame,
                ) {
                    self.edit.unsent(error);
                }
            }
        }
        if self.elements_active && self.state == ViewState::Attached {
            if let Some(epoch) = self.elements.epoch {
                if let Some(query) = self.styles.prepare(
                    now,
                    target,
                    session,
                    poll.document,
                    epoch,
                    self.elements.selected,
                ) {
                    match page.request_args_for_frame(
                        session,
                        InspectorCommand::InspectStyle,
                        InspectorArgs::Style {
                            query: query.clone(),
                        },
                        self.target_frame,
                    ) {
                        Ok(()) => self.styles.begin(query, now),
                        Err(error) if error.code == InspectorErrorCode::Busy => {}
                        Err(error) => {
                            self.styles.begin(query, now);
                            self.styles.complete(Err(error));
                        }
                    }
                }
            }
        }
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
        if completed.command.needs_style_version() {
            self.styles.complete(completed.result);
            return;
        }
        match completed.command {
            InspectorCommand::GetGeometry | InspectorCommand::PickNode => {
                self.geometry.complete(completed.command, completed.result);
                return;
            }
            InspectorCommand::Mutate => {
                if self.edit.complete(completed.result) {
                    // A stale epoch was retried: ask for a fresh status first.
                    live.last_snapshot = None;
                    *refresh = true;
                }
                return;
            }
            InspectorCommand::GrantMutation => {
                if let Err(error) = completed.result {
                    self.edit.notice = format!("Editing was not enabled: {}", error.message);
                }
                return;
            }
            InspectorCommand::RevokeMutation => return,
            _ => {}
        }
        if completed.command.needs_dom_version() {
            self.elements.complete(completed.command, completed.result);
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
            (InspectorCommand::EnableConsole, Ok(_)) => self.console.enabled = true,
            (InspectorCommand::EnableConsole, Err(error)) => {
                if is_transient(error.code) {
                    live.console_requested = false;
                } else {
                    self.console
                        .notice(format!("The console is unavailable: {}", error.message));
                }
            }
            (InspectorCommand::GrantEvaluation, Ok(_)) => {}
            (InspectorCommand::GrantEvaluation, Err(error)) => {
                self.console
                    .notice(format!("Evaluation was not enabled: {}", error.message));
            }
            (InspectorCommand::Evaluate, Ok(InspectorResponse::Evaluated(outcome))) => {
                self.console.pending = None;
                self.console.push(match *outcome {
                    EvalOutcome::Value { preview } => ConsoleLine::Result(preview),
                    EvalOutcome::Exception { details } => ConsoleLine::Failure(Box::new(details)),
                });
            }
            (InspectorCommand::Evaluate, Ok(_)) => self.console.pending = None,
            (InspectorCommand::Evaluate, Err(error)) => {
                self.console.pending = None;
                self.console.notice(match error.code {
                    InspectorErrorCode::Timeout => "No answer from the page in time. The evaluation may still be running: it was not confirmed stopped.".to_string(),
                    InspectorErrorCode::Cancelled => "The evaluation was cancelled (navigation, detach or the page stopped).".to_string(),
                    InspectorErrorCode::StaleDocument => "The page navigated before the evaluation ran; nothing was executed.".to_string(),
                    InspectorErrorCode::PolicyDenied => {
                        format!("Evaluation is not permitted: {}", error.message)
                    }
                    InspectorErrorCode::Unauthorized | InspectorErrorCode::StaleSession => {
                        format!("Evaluation is not authorised ({}). Enable it again for this document.", error.message)
                    }
                    _ => format!("Evaluation failed: {}", error.message),
                });
            }
            (InspectorCommand::GetStatusSnapshot, result) => {
                live.snapshot_pending_since = None;
                match result {
                    Ok(InspectorResponse::Status(snapshot))
                        if snapshot.target == target
                            && snapshot.document == document
                            && snapshot.is_bounded() =>
                    {
                        live.last_snapshot = Some(now);
                        self.elements
                            .status_changed(document, snapshot.mutation_epoch);
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
