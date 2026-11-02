use super::*;
#[test]
fn cpu_uses_interval_and_keeps_multicore_usage() {
    assert_eq!(cpu_percent(12.0, 12.5, 2.0), Some(25.0));
    assert_eq!(cpu_percent(12.0, 17.0, 2.0), Some(250.0));
    assert_eq!(cpu_percent(12.0, 12.0, 2.0), Some(0.0));
    assert_eq!(cpu_percent(12.0, 1.0, 2.0), None);
    assert_eq!(cpu_percent(12.0, 13.0, 0.0), None);
}
#[test]
fn live_process_sample_has_current_memory_and_cpu() {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        let sample = process_sample().expect("native resource counters");
        assert!(sample.memory_bytes > 0);
        assert!(sample.cpu_seconds >= 0.0);
    }
}
#[test]
fn container_uses_total_cpu_and_excludes_inactive_cache() {
    let s = container_usage(
        "usage_usec 2500000\nuser_usec 2000000\n",
        "10485760\n",
        "anon 6291456\ninactive_file 4194304\n",
    )
    .unwrap();
    assert_eq!(s.cpu_seconds, 2.5);
    assert_eq!(s.memory_bytes, 6 * 1_048_576);
    assert!(container_usage("user_usec 1", "100", "inactive_file 0").is_none());
    assert_eq!(
        container_usage("usage_usec 1", "10", "inactive_file 20")
            .unwrap()
            .memory_bytes,
        0
    );
}

#[test]
fn native_monitor_populates_cpu_and_ram_after_its_first_interval() {
    let mut monitor = Monitor::new();
    assert!(monitor.memory_bytes.is_some_and(|bytes| bytes > 0));
    monitor.sampled_at -= INTERVAL;
    assert!(monitor.refresh());
    assert!(
        monitor
            .cpu_percent
            .is_some_and(|percent| percent.is_finite() && percent >= 0.0)
    );
    assert!(!monitor.label().contains('—'));
}

#[test]
fn overall_usage_includes_existing_and_new_workers_and_handles_tab_close() {
    let since = Instant::now();
    let previous = BTreeMap::from([(7, Duration::from_secs(1))]);
    let workers = [
        WorkerUsage {
            id: 7,
            started: since - Duration::from_secs(5),
            resident_bytes: Some(64),
            cpu_time: Some(Duration::from_millis(1500)),
        },
        WorkerUsage {
            id: 8,
            started: since + Duration::from_millis(1),
            resident_bytes: Some(32),
            cpu_time: Some(Duration::from_millis(250)),
        },
    ];
    assert_eq!(combined_memory(128, &workers), Some(224));
    assert_eq!(
        worker_cpu_percent(&previous, &workers, since, 2.0),
        Some(37.5)
    );
    // Closing a worker must not subtract its lifetime clock and blank the total.
    assert_eq!(worker_cpu_percent(&previous, &[], since, 2.0), Some(0.0));
    assert_eq!(combined_memory(128, &[]), Some(128));
}
#[test]
fn unknown_worker_measurements_do_not_become_zero_or_a_lifetime_cpu_spike() {
    let since = Instant::now();
    let mut worker = WorkerUsage {
        id: 9,
        started: since - Duration::from_secs(10),
        resident_bytes: None,
        cpu_time: Some(Duration::from_secs(3)),
    };
    assert_eq!(combined_memory(128, &[worker]), None);
    assert_eq!(
        worker_cpu_percent(&BTreeMap::new(), &[worker], since, 2.0),
        None
    );
    worker.started = since;
    worker.cpu_time = None;
    assert_eq!(
        worker_cpu_percent(&BTreeMap::new(), &[worker], since, 2.0),
        None
    );
}
