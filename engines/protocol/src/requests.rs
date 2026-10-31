//! Host-side document/request correlation, independent of a browser or VM.
use crate::{DocumentId, ErrorCode, ProtocolError, RequestContext, RequestId};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub const MAX_PENDING: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestKind {
    Evaluate,
    /// The browser's destructive-action check ID, independent of the wire ID.
    PageState(u64),
}

#[derive(Clone, Copy, Debug)]
pub struct PendingRequest {
    pub context: RequestContext,
    pub kind: RequestKind,
    pub deadline: Instant,
}

#[derive(Debug, Default)]
pub struct DocumentRequests {
    document: DocumentId,
    next: u64,
    pending: BTreeMap<RequestId, PendingRequest>,
}

impl DocumentRequests {
    pub fn document(&self) -> DocumentId {
        self.document
    }

    pub fn begin(
        &mut self,
        kind: RequestKind,
        timeout: Duration,
    ) -> Result<RequestContext, ProtocolError> {
        if self.pending.len() >= MAX_PENDING {
            return Err(ProtocolError::new(
                ErrorCode::ResourceLimit,
                "Too many pending document requests",
            ));
        }
        let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
            ProtocolError::new(ErrorCode::InvalidRequest, "Invalid request deadline")
        })?;
        let next = self.next.checked_add(1).ok_or_else(|| {
            ProtocolError::new(ErrorCode::ResourceLimit, "Request identity exhausted")
        })?;
        let request = RequestId(next);
        self.next = next;
        let context = RequestContext {
            document: self.document,
            request,
        };
        self.pending.insert(
            request,
            PendingRequest {
                context,
                kind,
                deadline,
            },
        );
        Ok(context)
    }

    /// Ignore late, duplicate and old-document replies without consuming another request.
    pub fn take(&mut self, context: RequestContext) -> Option<PendingRequest> {
        if context.document != self.document {
            return None;
        }
        self.pending.remove(&context.request)
    }

    pub fn expired(&mut self, now: Instant) -> Vec<PendingRequest> {
        let ids: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, request)| request.deadline <= now)
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.pending.remove(&id))
            .collect()
    }

    pub fn cancel_all(&mut self) -> Vec<PendingRequest> {
        std::mem::take(&mut self.pending).into_values().collect()
    }

    pub fn next_document(&self) -> Result<DocumentId, ProtocolError> {
        self.document
            .0
            .checked_add(1)
            .map(DocumentId)
            .ok_or_else(|| {
                ProtocolError::new(ErrorCode::ResourceLimit, "Document identity exhausted")
            })
    }

    /// Call only after queue admission succeeds, while holding the host snapshot lock.
    pub fn navigate(&mut self, document: DocumentId) -> Result<Vec<PendingRequest>, ProtocolError> {
        if document != self.next_document()? {
            return Err(ProtocolError::new(
                ErrorCode::InvalidRequest,
                "Document generations must advance exactly once",
            ));
        }
        let cancelled = self.cancel_all();
        self.document = document;
        Ok(cancelled)
    }
}

#[cfg(test)]
#[path = "requests_tests.rs"]
mod tests;
