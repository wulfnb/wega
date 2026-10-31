use super::*;

#[test]
fn a_pending_navigation_cannot_present_or_finish_before_commit() {
    let mut state = Lifecycle::default();
    state.ready();
    state.observe_loading(false);
    assert!(state.loading());
    assert!(!state.presents_frames());
    assert!(state.commit());
    state.observe_loading(false);
    assert_eq!(state.phase(), Phase::Active);
    assert!(state.navigate());
    assert!(!state.accepts_content());
    assert!(!state.presents_frames());
    state.observe_loading(false);
    assert!(state.loading());
}

#[test]
fn worker_loading_updates_cannot_unfreeze_the_host_window() {
    let mut state = Lifecycle::default();
    state.commit();
    state.freeze(true);
    state.observe_loading(false);
    assert_eq!(state.phase(), Phase::Frozen);
    assert!(!state.presents_frames());
    state.freeze(false);
    assert_eq!(state.phase(), Phase::Active);
}

#[test]
fn stale_worker_messages_cannot_reopen_terminal_sessions() {
    for crash in [false, true] {
        let mut state = Lifecycle::default();
        state.commit();
        if crash {
            state.crash();
        } else {
            state.close();
        }
        state.ready();
        state.observe_loading(true);
        state.freeze(false);
        assert!(!state.commit());
        assert!(!state.navigate());
        assert!(!state.accepts_requests());
        assert!(!state.presents_frames());
        assert_eq!(state.failed(), crash);
        if !crash {
            state.crash();
            assert_eq!(state.phase(), Phase::Closing);
            assert!(state.acknowledge_close());
            assert!(!state.acknowledge_close());
        }
    }
}
