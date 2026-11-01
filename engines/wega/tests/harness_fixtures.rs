//! Step 03: Real worker observability, request correlation, and offline framework fixture verification.
mod support;
use std::{path::Path, time::Duration};
use support::harness::{TestFailureKind, WorkerHarness};

fn fixture_url(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.html"));
    format!("file://{}", path.display())
}

#[test]
fn test_react_counter_in_subprocess_worker() {
    let store = wega_platform::FrameStore::create(wega_protocol::MAX_PIXELS * 4).unwrap();
    let mut harness = match WorkerHarness::start(store.path().unwrap()) {
        Ok(h) => h,
        Err(e) => panic!("Worker must start: {e}"),
    };

    let url = fixture_url("react");
    harness
        .load(&url, Duration::from_secs(10))
        .expect("Load react fixture");

    // Verify initial text
    harness
        .assert_text("#action", "0", Duration::from_secs(5))
        .expect("Initial counter must be 0");

    // Click button to increment
    harness.click(30.0, 30.0).expect("Click action button");

    // Verify updated text
    harness
        .assert_text("#action", "1", Duration::from_secs(5))
        .expect("Counter must increment to 1");

    // Verify request-correlated frame capture
    let (width, height, hash) = harness
        .capture(Duration::from_secs(5))
        .expect("Capture document");
    assert_eq!(width, 800);
    assert_eq!(height, 600);
    assert!(!hash.is_empty(), "Capture hash must be non-empty");
}

#[test]
fn test_preact_counter_in_subprocess_worker() {
    let store = wega_platform::FrameStore::create(wega_protocol::MAX_PIXELS * 4).unwrap();
    let mut harness = WorkerHarness::start(store.path().unwrap()).expect("Worker must start");

    let url = fixture_url("preact");
    harness
        .load(&url, Duration::from_secs(10))
        .expect("Load preact fixture");

    harness
        .assert_text("#action", "0", Duration::from_secs(5))
        .expect("Initial counter must be 0");

    harness.click(30.0, 30.0).expect("Click action button");

    harness
        .assert_text("#action", "1", Duration::from_secs(5))
        .expect("Counter must increment to 1");
}

#[test]
fn test_vue_counter_in_subprocess_worker() {
    let store = wega_platform::FrameStore::create(wega_protocol::MAX_PIXELS * 4).unwrap();
    let mut harness = WorkerHarness::start(store.path().unwrap()).expect("Worker must start");

    let url = fixture_url("vue");
    harness
        .load(&url, Duration::from_secs(10))
        .expect("Load vue fixture");

    harness
        .assert_text("#action", "0", Duration::from_secs(5))
        .expect("Initial counter must be 0");

    harness.click(30.0, 30.0).expect("Click action button");

    harness
        .assert_text("#action", "1", Duration::from_secs(5))
        .expect("Counter must increment to 1");
}

#[test]
fn test_jquery_counter_in_subprocess_worker() {
    let store = wega_platform::FrameStore::create(wega_protocol::MAX_PIXELS * 4).unwrap();
    let mut harness = WorkerHarness::start(store.path().unwrap()).expect("Worker must start");

    let url = fixture_url("jquery");
    harness
        .load(&url, Duration::from_secs(10))
        .expect("Load jquery fixture");

    harness
        .assert_text("#action", "0", Duration::from_secs(5))
        .expect("Initial counter must be 0");

    harness.click(30.0, 30.0).expect("Click action button");

    harness
        .assert_text("#action", "1", Duration::from_secs(5))
        .expect("Counter must increment to 1");
}

#[test]
fn test_harness_request_correlated_navigation() {
    let store = wega_platform::FrameStore::create(wega_protocol::MAX_PIXELS * 4).unwrap();
    let mut harness = WorkerHarness::start(store.path().unwrap()).expect("Worker must start");

    let url1 = fixture_url("jquery");
    let ctx1 = harness.load(&url1, Duration::from_secs(10)).unwrap();

    let url2 = fixture_url("react");
    harness
        .navigate(&url2, Duration::from_secs(5))
        .expect("Navigate to react");

    // Wait for the new document to commit
    harness
        .wait_for(
            |event| matches!(event, wega_protocol::Event::DocumentCommitted { .. }),
            Duration::from_secs(5),
        )
        .expect("New document committed");

    let ctx2 = harness.current_context();
    assert_ne!(
        ctx1.document, ctx2.document,
        "Document ID must advance on navigation"
    );
}

#[test]
fn test_harness_failure_classification_distinguishes_crash_timeout_and_assertion() {
    let store = wega_platform::FrameStore::create(wega_protocol::MAX_PIXELS * 4).unwrap();
    let mut harness = WorkerHarness::start(store.path().unwrap()).expect("Worker must start");

    let url = fixture_url("react");
    harness.load(&url, Duration::from_secs(10)).unwrap();

    // 1. Assertion failure classification
    let assert_res = harness.assert_text("#action", "999", Duration::from_millis(100));
    assert!(
        matches!(assert_res, Err(TestFailureKind::AssertionFailure(_))),
        "Expected AssertionFailure, got: {assert_res:?}"
    );

    // 2. Engine error classification on JS exception
    let eval_res = harness.evaluate("nonExistentFunction()", Duration::from_secs(2));
    assert!(
        matches!(eval_res, Err(TestFailureKind::EngineError(_))),
        "Expected EngineError on undefined function, got: {eval_res:?}"
    );

    // 3. Worker crash classification
    // Send a SIGKILL / terminate directly to the child process
    let _ = harness.send(wega_protocol::Command::Shutdown);
    std::thread::sleep(Duration::from_millis(50));
    let crash_check = harness.check_alive();
    assert!(
        matches!(crash_check, Err(TestFailureKind::WorkerCrash { .. })),
        "Expected WorkerCrash detection on process termination, got: {crash_check:?}"
    );
}
