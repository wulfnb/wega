//! Request-local permission lifecycle. Responders are consumed exactly once.
use std::collections::VecDeque;
pub trait PermissionPort {
    fn respond(self, allow: bool);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestId(u64);
/// Upper bound for every queue of site-initiated dialogs.
pub const MAX_PENDING: usize = 16;
/// One origin cannot occupy more than this many queued dialogs.
pub const MAX_PER_ORIGIN: usize = 3;
/// Whether one more dialog from `origin` may queue behind dialogs from `pending` origins.
/// Callers deny (or drop, which denies) requests that are not admitted.
pub fn admit<'a>(pending: impl IntoIterator<Item = &'a str>, origin: &str) -> bool {
    let (mut total, mut same) = (0, 0);
    for queued in pending {
        total += 1;
        same += usize::from(queued == origin);
    }
    total < MAX_PENDING && same < MAX_PER_ORIGIN
}
pub struct Permissions<R: PermissionPort> {
    pending: VecDeque<(RequestId, String, String, R)>,
    next: u64,
}
impl<R: PermissionPort> Default for Permissions<R> {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            next: 0,
        }
    }
}
impl<R: PermissionPort> Permissions<R> {
    /// Queues a request. Duplicates of a pending origin/feature pair and requests beyond
    /// the queue bounds are denied immediately; returns whether the request was queued.
    pub fn push(&mut self, origin: String, feature: String, responder: R) -> bool {
        if self
            .pending
            .iter()
            .any(|(_, o, f, _)| *o == origin && *f == feature)
            || !admit(self.pending.iter().map(|(_, o, _, _)| o.as_str()), &origin)
        {
            responder.respond(false);
            return false;
        }
        let Some(next) = self.next.checked_add(1) else {
            responder.respond(false);
            return false;
        };
        let id = RequestId(self.next);
        self.next = next;
        self.pending.push_back((id, origin, feature, responder));
        true
    }
    pub fn front(&self) -> Option<(RequestId, &str, &str)> {
        self.pending
            .front()
            .map(|(id, origin, feature, _)| (*id, origin.as_str(), feature.as_str()))
    }
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
    /// A stale UI action must not answer the next site's request.
    pub fn answer(&mut self, id: RequestId, allow: bool) -> bool {
        if self
            .pending
            .front()
            .is_some_and(|(current, _, _, _)| *current == id)
        {
            let (_, _, _, responder) = self.pending.pop_front().unwrap();
            responder.respond(allow);
            true
        } else {
            false
        }
    }
    /// Resolve queued requests affected by a changed site setting without touching other sites.
    pub fn resolve(&mut self, origin: &str, feature: &str, allow: bool) {
        let mut remaining = VecDeque::new();
        while let Some((id, queued_origin, queued_feature, responder)) = self.pending.pop_front() {
            if queued_origin == origin && queued_feature == feature {
                responder.respond(allow);
            } else {
                remaining.push_back((id, queued_origin, queued_feature, responder));
            }
        }
        self.pending = remaining;
    }
    pub fn clear(&mut self) {
        while let Some((_, _, _, responder)) = self.pending.pop_front() {
            responder.respond(false);
        }
    }
}
impl<R: PermissionPort> Drop for Permissions<R> {
    fn drop(&mut self) {
        self.clear();
    }
}
#[cfg(test)]
#[path = "permissions_tests.rs"]
mod tests;

use crate::engine_protocol::{
    DocumentContext,
    authority::{FrameAuthority, TrustedRequest, can_request_permission},
    identity::SessionIdentity,
};
use std::time::{Duration, Instant};

struct ScopedPending<R> {
    id: RequestId,
    requester: TrustedRequest,
    feature: String,
    deadline: Instant,
    responder: R,
}

/// Native permission port for engines with host-issued document authority.
/// The UI cannot turn a renderer origin string or an expired request into a grant.
pub struct ScopedPermissions<R: PermissionPort> {
    pending: VecDeque<ScopedPending<R>>,
    next: u64,
}
impl<R: PermissionPort> Default for ScopedPermissions<R> {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            next: 0,
        }
    }
}
impl<R: PermissionPort> ScopedPermissions<R> {
    pub fn push(
        &mut self,
        requester: TrustedRequest,
        feature: String,
        responder: R,
        now: Instant,
    ) -> bool {
        self.expire(now);
        let origin = &requester.principal.origin;
        if !can_request_permission(&requester)
            || self.pending.len() >= MAX_PENDING
            || self
                .pending
                .iter()
                .filter(|pending| pending.requester.principal.origin == *origin)
                .count()
                >= MAX_PER_ORIGIN
            || self
                .pending
                .iter()
                .any(|pending| pending.requester == requester && pending.feature == feature)
        {
            responder.respond(false);
            return false;
        }
        let Some(next) = self.next.checked_add(1) else {
            responder.respond(false);
            return false;
        };
        let Some(deadline) = now.checked_add(Duration::from_secs(60)) else {
            responder.respond(false);
            return false;
        };
        self.pending.push_back(ScopedPending {
            id: RequestId(self.next),
            requester,
            feature,
            deadline,
            responder,
        });
        self.next = next;
        true
    }
    pub fn front(&self) -> Option<(RequestId, &TrustedRequest, &str)> {
        self.pending
            .front()
            .map(|pending| (pending.id, &pending.requester, pending.feature.as_str()))
    }
    /// Matching IDs are consumed exactly once. Changed identity, origin, document or
    /// deadline always denies, including an old Allow action after navigation.
    pub fn answer(
        &mut self,
        id: RequestId,
        authority: &FrameAuthority,
        allow: bool,
        now: Instant,
    ) -> bool {
        if !self.pending.front().is_some_and(|pending| pending.id == id) {
            return false;
        }
        let pending = self.pending.pop_front().unwrap();
        let current = authority.resolve(pending.requester.document);
        pending.responder.respond(
            allow && pending.deadline > now && current.as_ref() == Some(&pending.requester),
        );
        true
    }
    fn deny_where(&mut self, mut matches: impl FnMut(&ScopedPending<R>) -> bool) {
        let mut retained = VecDeque::new();
        while let Some(pending) = self.pending.pop_front() {
            if matches(&pending) {
                pending.responder.respond(false);
            } else {
                retained.push_back(pending);
            }
        }
        self.pending = retained;
    }
    pub fn expire(&mut self, now: Instant) {
        self.deny_where(|pending| pending.deadline <= now);
    }
    pub fn cancel_document(&mut self, session: SessionIdentity, document: DocumentContext) {
        self.deny_where(|pending| {
            pending.requester.session == session && pending.requester.document == document
        });
    }
    pub fn cancel_session(&mut self, session: SessionIdentity) {
        self.deny_where(|pending| pending.requester.session == session);
    }
    pub fn clear(&mut self) {
        self.deny_where(|_| true);
    }
}
impl<R: PermissionPort> Drop for ScopedPermissions<R> {
    fn drop(&mut self) {
        self.clear();
    }
}
