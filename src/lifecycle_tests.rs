use super::*;

#[test]
fn loaded_documents_require_a_state_check_before_manual_destruction() {
    for address in [
        "https://example.org/editor",
        "file:///tmp/draft.html",
        "data:text/html,editor",
        "broken",
        "about:blank#editor",
        "about:blank",
    ] {
        assert!(requires_state_check(true, address), "{address}");
        assert!(!requires_state_check(false, address));
    }
    for address in ["wega://newtab", "wega://settings"] {
        assert!(!requires_state_check(true, address));
    }
}

#[test]
fn zero_minutes_disables_discarding_and_large_values_are_capped() {
    assert_eq!(timeout(0), None);
    assert_eq!(timeout(30), Some(Duration::from_secs(1800)));
    assert_eq!(
        timeout(u32::MAX),
        Some(Duration::from_secs(u64::from(MAX_DISCARD_MINUTES) * 60))
    );
}

#[test]
fn only_idle_loaded_background_tabs_are_discarded() {
    let limit = timeout(1);
    let idle = Activity {
        loaded: true,
        idle: Duration::from_secs(60),
        ..Activity::default()
    };
    assert!(may_discard(idle, limit));
    assert!(!may_discard(idle, None), "0 = never");
    for blocked in [
        Activity {
            active: true,
            ..idle
        },
        Activity {
            loaded: false,
            ..idle
        },
        Activity { busy: true, ..idle },
        Activity {
            idle: Duration::from_secs(59),
            ..idle
        },
    ] {
        assert!(!may_discard(blocked, limit), "{blocked:?}");
    }
}

#[test]
fn next_check_waits_for_the_oldest_tab_but_never_spins() {
    let now = Instant::now();
    let checked = now - Duration::from_secs(3600);
    let limit = timeout(30);
    assert_eq!(next_check([], limit, checked), None);
    assert_eq!(next_check([now], None, checked), None);
    assert_eq!(
        next_check([now, now - Duration::from_secs(60)], limit, checked),
        Some(now + Duration::from_secs(1740))
    );
    // A tab that is overdue but busy must not cause an immediate re-check loop.
    let overdue = now - Duration::from_secs(7200);
    assert_eq!(
        next_check([overdue], limit, now),
        Some(now + CHECK_INTERVAL)
    );
}

#[test]
fn restore_loads_only_the_selected_tab() {
    let loaded: Vec<_> = (0..4).map(|index| restore_loaded(index, 2)).collect();
    assert_eq!(loaded, [false, false, true, false]);
}
