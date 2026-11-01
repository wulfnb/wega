//! Run the actual desktop readout module against native counters on each platform,
//! without pulling the window/Servo dependencies into this regression fixture.
#[path = "../../../src/resources.rs"]
mod resources;
#[test]
fn browser_readout_has_live_memory_and_a_bounded_refresh_deadline() {
    let mut monitor = resources::Monitor::new();
    assert!(monitor.memory_bytes.is_some_and(|bytes| bytes > 0));
    assert!(!monitor.label().contains("RAM —"));
    assert!(monitor.description().contains("Updated every 2 seconds"));
    assert!(monitor.next_sample() > std::time::Instant::now());
    assert!(!monitor.refresh());
}
