use super::*;
use crate::{DocumentId, DocumentRevision, RequestId};

fn context(id: u64) -> RequestContext {
    RequestContext {
        document: DocumentId(0),
        revision: DocumentRevision(1),
        request: RequestId(id),
    }
}

#[test]
fn completion_cancellation_and_document_replacement_never_reenable_a_request_id() {
    let mut queue = CommandQueue::default();
    let now = Instant::now();
    let old = context(1);
    queue.admit_once(old, now, Duration::from_secs(5)).unwrap();
    queue.start(old, now).unwrap();
    queue.finish(old);
    let replacement = RequestContext {
        document: DocumentId(9),
        revision: DocumentRevision(9),
        ..old
    };
    assert!(queue.admit_once(old, now, Duration::from_secs(5)).is_err());
    assert!(
        queue
            .admit_once(replacement, now, Duration::from_secs(5))
            .is_err()
    );
    let next = context(2);
    queue.admit_once(next, now, Duration::from_secs(5)).unwrap();
    queue.cancel(next);
    assert!(queue.start(next, now).is_err());
    assert!(queue.admit_once(next, now, Duration::from_secs(5)).is_err());
    assert!(queue.pending.is_empty());
}

#[test]
fn cancelling_a_queued_operation_prevents_its_start() {
    let mut queue = CommandQueue::default();
    let now = Instant::now();
    let request = context(1);
    queue.admit(request, now, Duration::from_secs(5)).unwrap();
    assert!(queue.cancel(request));
    assert_eq!(
        queue.start(request, now).unwrap_err().code,
        ErrorCode::Cancelled
    );
    assert!(!queue.cancel(request));
    assert!(queue.pending.is_empty());
}

#[test]
fn elapsed_queue_time_is_charged_before_the_vm_runs() {
    let mut queue = CommandQueue::default();
    let now = Instant::now();
    queue
        .admit(context(1), now, Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        queue
            .start(context(1), now + Duration::from_secs(2))
            .unwrap_err()
            .code,
        ErrorCode::Timeout
    );
    assert!(queue.pending.is_empty());
}

#[test]
fn unknown_cancellations_do_not_allocate_and_running_work_cannot_claim_rollback() {
    let mut queue = CommandQueue::default();
    let now = Instant::now();
    for id in 0..1000 {
        assert!(!queue.cancel(context(id)));
    }
    assert!(queue.pending.is_empty());
    queue
        .admit(context(1), now, Duration::from_secs(5))
        .unwrap();
    queue.start(context(1), now).unwrap();
    assert!(!queue.cancel(context(1)));
    assert_eq!(
        queue.start(context(1), now).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    queue.finish(context(1));
    assert!(queue.pending.is_empty());
}

#[test]
fn duplicates_and_capacity_failures_do_not_replace_an_admitted_operation() {
    let mut queue = CommandQueue::default();
    let now = Instant::now();
    for id in 0..MAX_PENDING {
        queue
            .admit(context(id as u64), now, Duration::from_secs(5))
            .unwrap();
    }
    assert_eq!(
        queue
            .admit(context(0), now, Duration::ZERO)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        queue
            .admit(context(100), now, Duration::ZERO)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
    assert!(queue.start(context(0), now).is_ok());
    queue.finish(context(0));
    assert!(
        queue
            .admit(context(100), now, Duration::from_secs(5))
            .is_ok()
    );
}
