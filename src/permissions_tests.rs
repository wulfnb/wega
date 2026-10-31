use super::*;
use std::{cell::RefCell, rc::Rc};
#[test]
fn scoped_permissions_deny_cancelled_expired_replayed_and_replaced_documents() {
    use crate::engine_protocol::{
        DocumentId, DocumentRevision,
        authority::{Origin, Principal},
        identity::{FrameId, ProcessSessionId, SessionIdentity, TabId},
    };
    let answers = Rc::new(RefCell::new(Vec::new()));
    let session = SessionIdentity {
        tab: TabId(1),
        frame: FrameId(1),
        process: ProcessSessionId([1; 16]),
        service: None,
    };
    let document = DocumentContext {
        document: DocumentId(1),
        revision: DocumentRevision(1),
    };
    let principal = Principal {
        origin: Origin::Tuple("https://example.test".into()),
        top_level_site: None,
    };
    let mut authority = FrameAuthority::new(session);
    authority.bind(document, principal.clone());
    let request = authority.resolve(document).unwrap();
    let mut queue = ScopedPermissions::default();
    let now = Instant::now();
    assert!(queue.push(
        request.clone(),
        "camera".into(),
        Responder(1, answers.clone()),
        now
    ));
    let first = queue.front().unwrap().0;
    queue.cancel_document(session, document);
    assert!(!queue.answer(first, &authority, true, now));
    assert!(queue.push(
        request.clone(),
        "camera".into(),
        Responder(2, answers.clone()),
        now
    ));
    let next = queue.front().unwrap().0;
    assert_ne!(first, next);
    authority.bind(
        DocumentContext {
            revision: DocumentRevision(2),
            ..document
        },
        principal,
    );
    assert!(queue.answer(next, &authority, true, now));
    assert!(queue.push(
        request.clone(),
        "camera".into(),
        Responder(3, answers.clone()),
        now
    ));
    let expired = queue.front().unwrap().0;
    assert!(queue.answer(expired, &authority, true, now + Duration::from_secs(60)));
    authority.bind(
        document,
        Principal {
            origin: Origin::Opaque,
            top_level_site: None,
        },
    );
    assert!(!queue.push(
        authority.resolve(document).unwrap(),
        "camera".into(),
        Responder(4, answers.clone()),
        now
    ));
    assert!(queue.push(
        request.clone(),
        "camera".into(),
        Responder(5, answers.clone()),
        now
    ));
    queue.cancel_session(session);
    assert!(queue.front().is_none());
    assert!(queue.push(request, "camera".into(), Responder(6, answers.clone()), now));
    drop(queue);
    assert_eq!(
        *answers.borrow(),
        [
            (1, false),
            (2, false),
            (3, false),
            (4, false),
            (5, false),
            (6, false)
        ]
    );
}

#[test]
fn scoped_permission_grants_require_the_exact_host_authority_and_ids_never_wrap() {
    use crate::engine_protocol::{
        DocumentId, DocumentRevision,
        authority::{Origin, Principal},
        identity::{FrameId, ProcessSessionId, SessionIdentity, TabId},
    };
    let answers = Rc::new(RefCell::new(Vec::new()));
    let session = SessionIdentity {
        tab: TabId(1),
        frame: FrameId(1),
        process: ProcessSessionId([1; 16]),
        service: None,
    };
    let document = DocumentContext {
        document: DocumentId(1),
        revision: DocumentRevision(1),
    };
    let principal = Principal {
        origin: Origin::Tuple("https://example.test".into()),
        top_level_site: None,
    };
    let mut authority = FrameAuthority::new(session);
    authority.bind(document, principal.clone());
    let mut queue = ScopedPermissions::default();
    let now = Instant::now();
    let request = authority.resolve(document).unwrap();
    assert!(queue.push(
        request.clone(),
        "camera".into(),
        Responder(1, answers.clone()),
        now
    ));
    assert!(queue.answer(queue.front().unwrap().0, &authority, true, now));
    assert!(queue.push(
        request.clone(),
        "camera".into(),
        Responder(2, answers.clone()),
        now
    ));
    let mut wrong = FrameAuthority::new(SessionIdentity {
        process: ProcessSessionId([2; 16]),
        ..session
    });
    wrong.bind(document, principal);
    assert!(queue.answer(queue.front().unwrap().0, &wrong, true, now));
    queue.next = u64::MAX;
    assert!(!queue.push(request, "camera".into(), Responder(3, answers.clone()), now));
    assert_eq!(*answers.borrow(), [(1, true), (2, false), (3, false)]);
    let mut legacy = Permissions::default();
    legacy.next = u64::MAX;
    assert!(!legacy.push(
        "https://example.test".into(),
        "camera".into(),
        Responder(4, answers.clone())
    ));
    assert_eq!(answers.borrow().last(), Some(&(4, false)));
}
struct Responder(usize, Rc<RefCell<Vec<(usize, bool)>>>);
impl PermissionPort for Responder {
    fn respond(self, allow: bool) {
        self.1.borrow_mut().push((self.0, allow));
    }
}
#[test]
fn permissions_are_request_local_fifo_and_consumed_once() {
    let answers = Rc::new(RefCell::new(Vec::new()));
    let mut requests = Permissions::default();
    assert!(requests.is_empty());
    assert!(requests.front().is_none());
    assert!(!requests.answer(RequestId(0), true));
    requests.push(
        "https://a.test".into(),
        "camera".into(),
        Responder(1, answers.clone()),
    );
    requests.push(
        "https://b.test".into(),
        "location".into(),
        Responder(2, answers.clone()),
    );
    assert_eq!(
        requests.front(),
        Some((RequestId(0), "https://a.test", "camera"))
    );
    assert!(requests.answer(RequestId(0), true));
    assert_eq!(
        requests.front(),
        Some((RequestId(1), "https://b.test", "location"))
    );
    assert!(
        !requests.answer(RequestId(0), true),
        "Stale allow must not grant the next site"
    );
    requests.clear();
    assert!(requests.is_empty());
    assert!(!requests.answer(RequestId(0), false));
    assert_eq!(*answers.borrow(), [(1, true), (2, false)]);
    requests.push(
        "https://a.test".into(),
        "camera".into(),
        Responder(3, answers.clone()),
    );
    drop(requests);
    assert_eq!(*answers.borrow(), [(1, true), (2, false), (3, false)]);
}

#[test]
fn duplicate_and_overflowing_requests_are_denied_immediately() {
    let answers = Rc::new(RefCell::new(Vec::new()));
    let mut requests = Permissions::default();
    assert!(requests.push(
        "https://a.test".into(),
        "camera".into(),
        Responder(0, answers.clone())
    ));
    assert!(
        !requests.push(
            "https://a.test".into(),
            "camera".into(),
            Responder(1, answers.clone())
        ),
        "A pending origin/feature pair is not queued twice"
    );
    for (index, feature) in ["microphone", "location"].into_iter().enumerate() {
        assert!(requests.push(
            "https://a.test".into(),
            feature.into(),
            Responder(index + 2, answers.clone())
        ));
    }
    assert!(
        !requests.push(
            "https://a.test".into(),
            "notifications".into(),
            Responder(4, answers.clone())
        ),
        "One origin cannot flood the queue"
    );
    assert_eq!(*answers.borrow(), [(1, false), (4, false)]);
    for index in 0..MAX_PENDING {
        requests.push(
            format!("https://{index}.test"),
            "camera".into(),
            Responder(100 + index, answers.clone()),
        );
    }
    let mut queued = 0;
    while let Some((id, ..)) = requests.front() {
        requests.answer(id, true);
        queued += 1;
    }
    assert_eq!(queued, MAX_PENDING, "The global queue is bounded");
}

#[test]
fn admission_bounds_total_and_per_origin_queues() {
    assert!(admit([], "a"));
    assert!(admit(["a", "a"], "a"));
    assert!(!admit(["a"; MAX_PER_ORIGIN], "a"));
    assert!(admit(["a"; MAX_PER_ORIGIN], "b"));
    let full: Vec<String> = (0..MAX_PENDING).map(|i| i.to_string()).collect();
    assert!(!admit(full.iter().map(String::as_str), "new"));
}

#[test]
fn site_setting_resolves_only_matching_queued_requests_once() {
    let answers = Rc::new(RefCell::new(Vec::new()));
    let mut requests = Permissions::default();
    requests.push(
        "https://a.test".into(),
        "Camera".into(),
        Responder(1, answers.clone()),
    );
    requests.push(
        "https://a.test".into(),
        "Microphone".into(),
        Responder(2, answers.clone()),
    );
    requests.push(
        "https://b.test".into(),
        "Camera".into(),
        Responder(3, answers.clone()),
    );
    requests.resolve("https://a.test", "Camera", false);
    requests.resolve("https://a.test", "Camera", false);
    assert_eq!(*answers.borrow(), vec![(1, false)]);
    let (id, origin, feature) = requests.front().unwrap();
    assert_eq!((origin, feature), ("https://a.test", "Microphone"));
    requests.answer(id, true);
    drop(requests);
    assert_eq!(*answers.borrow(), vec![(1, false), (2, true), (3, false)]);
}
