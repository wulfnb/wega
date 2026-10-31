//! Worker-side bounded admission and cancellation, before entering any VM API.
use crate::{ErrorCode, ProtocolError, RequestContext, requests::MAX_PENDING};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Debug)]
struct Queued {
    deadline: Instant,
    running: bool,
    cancelled: bool,
}

#[derive(Debug, Default)]
pub struct CommandQueue {
    pending: BTreeMap<RequestContext, Queued>,
    last_request: Option<crate::RequestId>,
}

impl CommandQueue {
    /// v11 hosts issue strictly increasing IDs across document replacements.
    /// A high-water mark rejects replay without retaining completed request payloads.
    pub fn admit_once(
        &mut self,
        context: RequestContext,
        now: Instant,
        timeout: Duration,
    ) -> Result<(), ProtocolError> {
        if self
            .last_request
            .is_some_and(|last| context.request <= last)
        {
            return Err(ProtocolError::new(
                ErrorCode::InvalidRequest,
                "Reused or out-of-order request identity",
            ));
        }
        self.admit(context, now, timeout)?;
        self.last_request = Some(context.request);
        Ok(())
    }
    pub fn admit(
        &mut self,
        context: RequestContext,
        now: Instant,
        timeout: Duration,
    ) -> Result<(), ProtocolError> {
        if self.pending.contains_key(&context) {
            return Err(ProtocolError::new(
                ErrorCode::InvalidRequest,
                "Duplicate request identity",
            ));
        }
        if self.pending.len() >= MAX_PENDING {
            return Err(ProtocolError::new(
                ErrorCode::ResourceLimit,
                "Worker request queue is full",
            ));
        }
        let deadline = now.checked_add(timeout).ok_or_else(|| {
            ProtocolError::new(ErrorCode::InvalidRequest, "Invalid queue deadline")
        })?;
        self.pending.insert(
            context,
            Queued {
                deadline,
                running: false,
                cancelled: false,
            },
        );
        Ok(())
    }

    /// Unknown/running requests allocate nothing and cannot acknowledge rollback.
    pub fn cancel(&mut self, context: RequestContext) -> bool {
        if let Some(request) = self.pending.get_mut(&context)
            && !request.running
        {
            request.cancelled = true;
            return true;
        }
        false
    }

    pub fn start(&mut self, context: RequestContext, now: Instant) -> Result<(), ProtocolError> {
        let request = self.pending.get_mut(&context).ok_or_else(|| {
            ProtocolError::new(ErrorCode::InvalidRequest, "Request was not admitted")
        })?;
        let error = if request.running {
            Some(ProtocolError::new(
                ErrorCode::InvalidRequest,
                "Request is already running",
            ))
        } else if request.cancelled {
            Some(ProtocolError::new(
                ErrorCode::Cancelled,
                "Queued request was cancelled",
            ))
        } else if request.deadline <= now {
            Some(ProtocolError::new(
                ErrorCode::Timeout,
                "Queued request expired before execution",
            ))
        } else {
            None
        };
        if let Some(error) = error {
            if !request.running {
                self.pending.remove(&context);
            }
            return Err(error);
        }
        request.running = true;
        Ok(())
    }

    pub fn finish(&mut self, context: RequestContext) {
        self.pending.remove(&context);
    }
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
