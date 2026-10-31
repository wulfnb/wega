use super::*;

#[test]
fn navigation_invalidates_old_replies_without_reusing_request_ids() {
    let mut requests = DocumentRequests::default();
    let old = requests
        .begin(RequestKind::PageState(7), Duration::from_secs(2))
        .unwrap();
    let cancelled = requests
        .navigate(requests.next_document().unwrap())
        .unwrap();
    assert_eq!(cancelled.len(), 1);
    assert_eq!(cancelled[0].context, old);
    assert_eq!(cancelled[0].kind, RequestKind::PageState(7));
    let current = requests
        .begin(RequestKind::PageState(8), Duration::from_secs(2))
        .unwrap();
    assert_ne!(old.document, current.document);
    assert_ne!(old.request, current.request);
    assert!(requests.take(old).is_none());
    assert_eq!(
        requests.take(current).unwrap().kind,
        RequestKind::PageState(8)
    );
    assert!(requests.take(current).is_none());
}

#[test]
fn forged_generation_does_not_consume_the_real_pending_request() {
    let mut requests = DocumentRequests::default();
    let context = requests
        .begin(RequestKind::Evaluate, Duration::from_secs(2))
        .unwrap();
    assert!(
        requests
            .take(RequestContext {
                document: DocumentId(50),
                ..context
            })
            .is_none()
    );
    assert!(
        requests
            .take(RequestContext {
                request: RequestId(50),
                ..context
            })
            .is_none()
    );
    assert!(requests.take(context).is_some());
}

#[test]
fn expired_requests_release_bounded_capacity_and_cannot_complete_late() {
    let mut requests = DocumentRequests::default();
    let mut contexts = Vec::new();
    for _ in 0..MAX_PENDING {
        contexts.push(
            requests
                .begin(RequestKind::Evaluate, Duration::ZERO)
                .unwrap(),
        );
    }
    assert_eq!(
        requests
            .begin(RequestKind::Evaluate, Duration::ZERO)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
    let expired = requests.expired(Instant::now());
    assert_eq!(expired.len(), MAX_PENDING);
    for context in contexts {
        assert!(requests.take(context).is_none());
    }
    assert!(
        requests
            .begin(RequestKind::Evaluate, Duration::from_secs(1))
            .is_ok()
    );
}

#[test]
fn rejected_navigation_does_not_invalidate_the_current_document() {
    let mut requests = DocumentRequests::default();
    let context = requests
        .begin(RequestKind::PageState(12), Duration::from_secs(2))
        .unwrap();
    assert!(requests.navigate(DocumentId(8)).is_err());
    assert_eq!(requests.document(), DocumentId(0));
    assert!(requests.take(context).is_some());
}

#[test]
fn identities_never_wrap_and_shutdown_cancels_each_request_once() {
    let mut requests = DocumentRequests::default();
    requests
        .begin(RequestKind::PageState(99), Duration::from_secs(2))
        .unwrap();
    requests
        .begin(RequestKind::Evaluate, Duration::from_secs(2))
        .unwrap();
    assert_eq!(requests.cancel_all().len(), 2);
    assert!(requests.cancel_all().is_empty());
    requests.next = u64::MAX;
    assert_eq!(
        requests
            .begin(RequestKind::Evaluate, Duration::ZERO)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
    requests.document = DocumentId(u64::MAX);
    assert_eq!(
        requests.next_document().unwrap_err().code,
        ErrorCode::ResourceLimit
    );
}
