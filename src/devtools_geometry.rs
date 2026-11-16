//! Browser-side state for geometry, the Inspect Element picker and authorized edits. Pure state
//! machines over protocol values: they hold no engine type, never ask for layout, and drop every
//! cached value when the document, session or process changes. One geometry-family request is
//! outstanding at a time; pointer movement is coalesced into the newest position.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use wega::engine_protocol::{
    DocumentContext,
    inspector::{
        INSPECTOR_TIMEOUT_MS, InspectorCommand, InspectorError, InspectorErrorCode,
        InspectorResponse,
        dom::InspectorNodeId,
        edit::{MutationOp, MutationOutcome},
        geometry::{GeometryReport, GeometryState, PickReport, to_milli},
    },
};

/// Pending edits (including the one in flight) kept before further ones are refused visibly.
pub(crate) const MAX_QUEUED_EDITS: usize = 4;
/// Hover picks are not sent more often than this, however fast the pointer moves.
pub(crate) const HOVER_INTERVAL: Duration = Duration::from_millis(40);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pending {
    Geometry(InspectorNodeId),
    Hover,
    Click,
}

#[derive(Debug, Default)]
pub(crate) struct GeometryView {
    document: Option<DocumentContext>,
    /// Inspect Element mode. The browser consumes page pointer input while this is true.
    pub picker: bool,
    /// Picker mode was just entered: the page must be told its pointer is gone (no half-finished
    /// press or hover state), once.
    entered: bool,
    /// Escape cancelled the picker: its key release must not reach the page either.
    pub swallow_escape_release: bool,
    /// Newest hover position (viewport CSS px) not yet asked about.
    hover_wanted: Option<(f32, f32)>,
    /// A click position waiting to be resolved into a selection.
    click_wanted: Option<(f32, f32)>,
    pending: Option<Pending>,
    pending_since: Option<Instant>,
    last_hover_request: Option<Instant>,
    /// The element under the pointer while picking.
    pub hover: Option<GeometryReport>,
    /// Geometry of the selected element, and the node and epoch it was read for.
    pub selected: Option<GeometryReport>,
    selected_for: Option<(InspectorNodeId, u64)>,
    /// A completed click: the node to select and its ancestors (root first), taken once.
    chosen: Option<PickReport>,
    pub notice: String,
}

impl GeometryView {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn document(&mut self, document: DocumentContext) {
        if self.document != Some(document) {
            // A new document ends picking and drops every cached report.
            self.reset();
            self.document = Some(document);
        }
    }
    pub fn set_picker(&mut self, on: bool) {
        if self.picker == on {
            return;
        }
        self.entered = on;
        self.picker = on;
        self.hover = None;
        self.hover_wanted = None;
        self.click_wanted = None;
        // An in-flight pick is correlated until it returns, then discarded by `complete`.
        self.notice.clear();
    }
    /// The pointer moved over the page while picking (viewport CSS px).
    pub fn hover_at(&mut self, x: f32, y: f32) {
        if self.picker {
            self.hover_wanted = Some((x, y));
        }
    }
    pub fn click_at(&mut self, x: f32, y: f32) {
        if self.picker {
            self.click_wanted = Some((x, y));
        }
    }
    /// True once after the picker was entered.
    pub fn take_entered(&mut self) -> bool {
        std::mem::take(&mut self.entered)
    }
    pub fn pointer_left(&mut self) {
        self.hover = None;
        self.hover_wanted = None;
    }
    /// Take the picked node once (the Elements view selects and reveals it).
    pub fn take_chosen(&mut self) -> Option<PickReport> {
        self.chosen.take()
    }
    #[cfg(test)]
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// What to ask next, if anything. `selected` and `epoch` come from the Elements view.
    pub fn next(
        &mut self,
        now: Instant,
        selected: Option<InspectorNodeId>,
        epoch: Option<u64>,
    ) -> Option<(
        InspectorCommand,
        wega::engine_protocol::inspector::InspectorArgs,
    )> {
        use wega::engine_protocol::inspector::InspectorArgs;
        if self.pending.is_some() {
            return None;
        }
        if let Some((x, y)) = self.click_wanted.take() {
            self.pending = Some(Pending::Click);
            self.pending_since = Some(now);
            return Some((
                InspectorCommand::PickNode,
                InspectorArgs::Pick {
                    x_milli: to_milli(x),
                    y_milli: to_milli(y),
                },
            ));
        }
        if let Some((x, y)) = self.hover_wanted
            && self
                .last_hover_request
                .is_none_or(|at| now.saturating_duration_since(at) >= HOVER_INTERVAL)
        {
            self.hover_wanted = None;
            self.last_hover_request = Some(now);
            self.pending = Some(Pending::Hover);
            self.pending_since = Some(now);
            return Some((
                InspectorCommand::PickNode,
                InspectorArgs::Pick {
                    x_milli: to_milli(x),
                    y_milli: to_milli(y),
                },
            ));
        }
        let (node, epoch) = (selected?, epoch?);
        if self.selected_for != Some((node, epoch)) {
            self.pending = Some(Pending::Geometry(node));
            self.pending_since = Some(now);
            self.selected_for = Some((node, epoch));
            return Some((
                InspectorCommand::GetGeometry,
                InspectorArgs::DomNode { node },
            ));
        }
        None
    }

    pub fn expire(&mut self, now: Instant) {
        if self.pending_since.is_some_and(|at| {
            now.saturating_duration_since(at) >= Duration::from_millis(INSPECTOR_TIMEOUT_MS + 100)
        }) {
            self.complete(
                InspectorCommand::GetGeometry,
                Err(InspectorError::new(
                    InspectorErrorCode::Timeout,
                    "Geometry request timed out",
                )),
            );
        }
    }

    /// Whether this view issued a request of `command` and is waiting for it.
    pub fn awaiting(&self, command: InspectorCommand) -> bool {
        matches!(
            (self.pending, command),
            (Some(Pending::Geometry(_)), InspectorCommand::GetGeometry)
                | (
                    Some(Pending::Hover | Pending::Click),
                    InspectorCommand::PickNode
                )
        )
    }

    pub fn complete(
        &mut self,
        command: InspectorCommand,
        result: Result<InspectorResponse, InspectorError>,
    ) {
        let Some(pending) = self.pending else { return };
        if !self.awaiting(command) {
            return;
        }
        self.pending = None;
        self.pending_since = None;
        let document = self.document;
        // A reply for a picker that was cancelled meanwhile is dropped, never applied.
        let cancelled = matches!(pending, Pending::Hover | Pending::Click) && !self.picker;
        match (pending, result) {
            (Pending::Geometry(node), Ok(InspectorResponse::Geometry(report)))
                if report.is_bounded()
                    && Some(report.document) == document
                    && report.node == node =>
            {
                self.selected = Some(*report);
                self.notice.clear();
            }
            (Pending::Hover, Ok(InspectorResponse::Picked(report)))
                if !cancelled
                    && report.is_bounded()
                    && Some(report.target.document) == document =>
            {
                self.hover = Some(report.target);
            }
            (Pending::Click, Ok(InspectorResponse::Picked(report)))
                if !cancelled
                    && report.is_bounded()
                    && Some(report.target.document) == document =>
            {
                // Picking ends with the click: forwarding is restored immediately.
                self.chosen = Some(*report);
                self.set_picker(false);
            }
            (Pending::Geometry(_), Err(error)) => {
                // Retry on the next epoch rather than hammering a failing read.
                self.notice = error.message;
                if error.code == InspectorErrorCode::StaleNode {
                    self.selected = None;
                }
            }
            (Pending::Hover | Pending::Click, Err(error)) => {
                if !cancelled {
                    self.notice = error.message;
                }
                if pending == Pending::Click && !cancelled {
                    // The click found nothing usable; stay in picker mode.
                    self.hover = None;
                }
            }
            _ => self.notice = "Ignored an invalid or stale geometry reply".into(),
        }
    }

    /// The overlay for a document: only reports that belong to it and have boxes.
    pub fn overlay(
        &self,
        document: DocumentContext,
    ) -> (Option<&GeometryReport>, Option<&GeometryReport>) {
        let ok = |r: &&GeometryReport| {
            r.document == document
                && matches!(r.state, GeometryState::Current | GeometryState::Stale)
        };
        (
            self.selected.as_ref().filter(ok),
            self.hover.as_ref().filter(ok),
        )
    }
    /// Test-only visibility into the wanted position conversion.
    #[cfg(test)]
    pub fn wanted(&self) -> Option<(f32, f32)> {
        use wega::engine_protocol::inspector::geometry::from_milli;
        self.hover_wanted
            .map(|(x, y)| (from_milli(to_milli(x)), from_milli(to_milli(y))))
    }
}

/// The edit queue. Edits are serialised: one in flight, a few waiting, the rest refused visibly.
/// Text buffers of the editing controls. UI-only scratch; never sent anywhere unvalidated.
#[derive(Debug, Default)]
pub(crate) struct EditorBuffers {
    pub new_name: String,
    pub new_value: String,
    pub css_property: String,
    pub css_value: String,
    pub css_important: bool,
    /// The attribute being edited: node key, name and the edited value.
    pub attribute: Option<(u64, String, String)>,
    /// The text node being edited: node key and the edited text.
    pub text: Option<(u64, String)>,
}

#[derive(Debug, Default)]
pub(crate) struct EditView {
    pub buffers: EditorBuffers,
    document: Option<DocumentContext>,
    /// The browser's own record that editing is enabled, and for which document.
    pub enabled: Option<DocumentContext>,
    queue: VecDeque<Queued>,
    pending: Option<(Queued, Instant)>,
    pub notice: String,
    /// The last edit applied, for the status line.
    pub last: Option<String>,
    /// A removal completed; the Elements view must forget this node.
    removed: Option<InspectorNodeId>,
    /// The epoch returned by the last successful edit, adopted once by the session.
    adopted: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Queued {
    pub node: InspectorNodeId,
    pub op: MutationOp,
    retried: bool,
}

impl EditView {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn document(&mut self, document: DocumentContext) {
        if self.document != Some(document) {
            self.reset();
            self.document = Some(document);
        }
    }
    /// The browser's record from the poll; editing is on only for the current document.
    pub fn observe_grant(&mut self, grant: Option<DocumentContext>, current: DocumentContext) {
        self.enabled = grant.filter(|g| *g == current);
        if self.enabled.is_none() && (!self.queue.is_empty() || self.pending.is_some()) {
            // Authority is gone: nothing queued may be sent, and the loss is visible.
            self.queue.clear();
            self.pending = None;
            self.notice = "Editing was disabled; pending edits were dropped".into();
        }
    }
    /// Drop everything queued (the user revoked editing).
    pub fn reset_queue(&mut self) {
        self.queue.clear();
        self.pending = None;
    }
    pub fn is_enabled(&self) -> bool {
        self.enabled.is_some()
    }
    #[cfg_attr(not(feature = "test-harness"), allow(dead_code))]
    pub fn busy(&self) -> bool {
        self.pending.is_some() || !self.queue.is_empty()
    }
    /// Queue an edit. Refused with a visible reason when editing is off or the queue is full.
    pub fn submit(&mut self, node: InspectorNodeId, op: MutationOp) -> bool {
        if !self.is_enabled() {
            self.notice = "Editing is not enabled for this document".into();
            return false;
        }
        if Some(node.document) != self.document {
            self.notice = "That node belongs to a previous document".into();
            return false;
        }
        if let Err(message) = op.validate() {
            self.notice = message.into();
            return false;
        }
        if self.queue.len() + usize::from(self.pending.is_some()) >= MAX_QUEUED_EDITS {
            self.notice = "Too many pending edits; wait for the page to apply them".into();
            return false;
        }
        self.queue.push_back(Queued {
            node,
            op,
            retried: false,
        });
        true
    }
    /// The next edit to send, if none is in flight.
    pub fn next(&mut self, now: Instant) -> Option<Queued> {
        if self.pending.is_some() {
            return None;
        }
        let queued = self.queue.pop_front()?;
        self.pending = Some((queued.clone(), now));
        Some(queued)
    }
    /// The edit could not even be sent (queue full, worker busy): put it back, or drop it.
    pub fn unsent(&mut self, error: InspectorError) {
        if let Some((queued, _)) = self.pending.take() {
            if error.code == InspectorErrorCode::Busy {
                self.queue.push_front(queued);
            } else {
                self.notice = error.message;
            }
        }
    }
    pub fn expire(&mut self, now: Instant) {
        if self.pending.as_ref().is_some_and(|(_, at)| {
            now.saturating_duration_since(*at) >= Duration::from_millis(INSPECTOR_TIMEOUT_MS + 100)
        }) {
            self.pending = None;
            self.notice =
                "The page did not answer in time; the edit may or may not have applied".into();
        }
    }
    #[cfg(test)]
    pub fn awaiting(&self) -> bool {
        self.pending.is_some()
    }
    /// Returns true when a status refresh should be forced (a stale epoch was retried).
    pub fn complete(&mut self, result: Result<InspectorResponse, InspectorError>) -> bool {
        let Some((queued, _)) = self.pending.take() else {
            return false;
        };
        match result {
            Ok(InspectorResponse::Mutated(outcome)) if self.accepts(&outcome) => {
                self.notice.clear();
                self.last = Some(if outcome.changed {
                    describe(&queued.op)
                } else {
                    format!(
                        "{} (the engine ignored it: nothing changed)",
                        describe(&queued.op)
                    )
                });
                self.adopted = Some(outcome.epoch);
                if queued.op == MutationOp::RemoveNode {
                    self.removed = Some(queued.node);
                }
                false
            }
            Ok(_) => {
                self.notice = "Ignored an invalid or stale edit reply".into();
                false
            }
            Err(error) if error.code == InspectorErrorCode::StaleMutation && !queued.retried => {
                // The document moved between the user's view and the edit: refresh and retry
                // exactly once against the new epoch; never apply to a guessed state.
                let mut again = queued;
                again.retried = true;
                self.queue.push_front(again);
                true
            }
            Err(error) => {
                self.notice = match error.code {
                    InspectorErrorCode::Unauthorized | InspectorErrorCode::StaleSession => {
                        "Editing is not authorised. Enable editing again for this document.".into()
                    }
                    InspectorErrorCode::StaleDocument => {
                        "The page navigated; the edit was not applied.".into()
                    }
                    InspectorErrorCode::StaleNode => {
                        "That node is gone; the edit was not applied.".into()
                    }
                    InspectorErrorCode::StaleMutation => {
                        "The page keeps changing; the edit was not applied.".into()
                    }
                    InspectorErrorCode::BudgetExceeded => {
                        format!("Over the page's DOM budget: {}", error.message)
                    }
                    InspectorErrorCode::PolicyDenied => format!("Not permitted: {}", error.message),
                    _ => format!("Edit failed: {}", error.message),
                };
                false
            }
        }
    }
    fn accepts(&self, outcome: &MutationOutcome) -> bool {
        outcome.is_bounded() && Some(outcome.document) == self.document
    }
    pub fn take_removed(&mut self) -> Option<InspectorNodeId> {
        self.removed.take()
    }
    pub fn take_adopted_epoch(&mut self) -> Option<u64> {
        self.adopted.take()
    }
}

fn describe(op: &MutationOp) -> String {
    match op {
        MutationOp::SetAttribute { name, .. } => format!("Set attribute {name}"),
        MutationOp::RemoveAttribute { name } => format!("Removed attribute {name}"),
        MutationOp::SetText { .. } => "Edited text".into(),
        MutationOp::RemoveNode => "Removed node".into(),
        MutationOp::SetInlineProperty {
            property, value, ..
        } if value.is_empty() => format!("Removed style {property}"),
        MutationOp::SetInlineProperty { property, .. } => format!("Set style {property}"),
    }
}

#[cfg(test)]
#[path = "devtools_geometry_tests.rs"]
mod tests;
