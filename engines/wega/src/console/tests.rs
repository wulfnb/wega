use super::*;

fn entry(text: &str) -> String {
    serde_json::json!({
        "m": "log", "l": "log", "t": text,
        "a": [{"kind": "String", "text": text}],
        "u": "https://e.test/app.js", "n": 3, "c": 7
    })
    .to_string()
}

#[test]
fn entries_get_increasing_sequence_numbers_and_keep_their_source() {
    let mut log = ConsoleLog::new();
    log.ingest(&entry("a"), "https://e.test/");
    log.ingest(&entry("b"), "https://e.test/");
    let messages = log.read_after(0, 10, 1 << 20);
    assert_eq!(messages.iter().map(|m| m.seq).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(messages[0].method, "log");
    let source = messages[0].source.as_ref().unwrap();
    assert_eq!(
        (source.url.as_str(), source.line, source.column),
        ("https://e.test/app.js", 3, 7)
    );
    assert!(messages[0].timestamp_ms > 1_600_000_000_000);
    assert_eq!(log.read_after(1, 10, 1 << 20).len(), 1);
    assert_eq!(log.first_seq(), 1);
    assert_eq!(log.next_seq(), 3);
}

#[test]
fn the_ring_is_bounded_by_entries_and_counts_what_it_evicts() {
    let mut log = ConsoleLog::new();
    for i in 0..(CONSOLE_RING_ENTRIES * 3) {
        log.ingest(&entry(&format!("m{i}")), "");
        assert!(log.len() <= CONSOLE_RING_ENTRIES);
        assert!(log.bytes() <= CONSOLE_RING_BYTES);
    }
    assert_eq!(log.len(), CONSOLE_RING_ENTRIES);
    assert_eq!(log.dropped() as usize, CONSOLE_RING_ENTRIES * 2);
    assert_eq!(log.first_seq() as usize, CONSOLE_RING_ENTRIES * 2 + 1);
    // The newest entries survive.
    let last = log.read_after(0, usize::MAX, usize::MAX).pop().unwrap();
    assert_eq!(last.text, format!("m{}", CONSOLE_RING_ENTRIES * 3 - 1));
}

#[test]
fn the_ring_is_bounded_by_bytes_before_entries_when_messages_are_large() {
    let mut log = ConsoleLog::new();
    let big = "x".repeat(1000);
    for _ in 0..5000 {
        log.ingest(&entry(&big), "");
        assert!(log.bytes() <= CONSOLE_RING_BYTES, "{}", log.bytes());
    }
    assert!(
        log.len() < CONSOLE_RING_ENTRIES,
        "{} entries retained",
        log.len()
    );
    assert!(log.dropped() > 0);
}

#[test]
fn a_single_message_is_shrunk_to_its_budget_and_never_rejected_for_size_alone() {
    let mut log = ConsoleLog::new();
    let wide: Vec<_> = (0..8)
        .map(|i| serde_json::json!({"key": format!("k{i}"), "value": {"kind": "String", "text": "v".repeat(250)}}))
        .collect();
    let json = serde_json::json!({
        "m": "log", "l": "log", "t": "t".repeat(1024),
        "a": (0..16).map(|_| serde_json::json!({"kind": "Object", "text": "o".repeat(250), "props": wide.clone()})).collect::<Vec<_>>(),
    })
    .to_string();
    assert!(json.len() < MAX_ENTRY_JSON * 2);
    log.ingest(&json, "");
    let messages = log.read_after(0, 10, usize::MAX);
    if let Some(message) = messages.first() {
        assert!(
            message.approx_bytes() <= MAX_MESSAGE_BYTES,
            "{}",
            message.approx_bytes()
        );
        assert!(message.is_bounded());
    } else {
        assert_eq!(
            log.dropped(),
            1,
            "an entry that cannot be stored must be counted"
        );
    }
}

#[test]
fn forged_entries_are_validated_and_counted() {
    let mut log = ConsoleLog::new();
    log.ingest("not json", "");
    log.ingest(&"x".repeat(MAX_ENTRY_JSON + 1), "");
    log.ingest(r#"{"m":"log"}"#, ""); // missing level
    log.ingest(
        r#"{"m":"log","l":"log","a":[{"kind":"Nope","text":"x"}]}"#,
        "",
    );
    assert_eq!(log.len(), 0);
    assert_eq!(log.dropped(), 4);
    // Unknown level names degrade to log; an unbounded preview becomes a placeholder.
    let mut deep = serde_json::json!({"kind": "Object", "text": "{}"});
    for _ in 0..6 {
        deep = serde_json::json!({"kind": "Object", "text": "{}", "props": [{"key": "k", "value": deep}]});
    }
    let json =
        serde_json::json!({"m": "m".repeat(500), "l": "weird", "t": "t", "a": [deep]}).to_string();
    log.ingest(&json, "");
    let message = &log.read_after(0, 1, usize::MAX)[0];
    assert_eq!(message.level, ConsoleLevel::Log);
    assert_eq!(message.method.len(), 32);
    assert_eq!(message.args[0].kind, PreviewKind::Unavailable);
    assert!(message.is_bounded());
}

#[test]
fn a_flood_of_ten_thousand_entries_stays_within_every_budget() {
    let mut log = ConsoleLog::new();
    let started = std::time::Instant::now();
    for i in 0..10_000 {
        log.ingest(&entry(&format!("flood {i} {}", "z".repeat(i % 700))), "");
    }
    assert!(log.len() <= CONSOLE_RING_ENTRIES && log.bytes() <= CONSOLE_RING_BYTES);
    assert_eq!(log.dropped() as usize + log.len(), 10_000);
    assert_eq!(log.next_seq(), 10_001, "sequence numbers never repeat");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn reading_respects_entry_and_byte_budgets_and_always_makes_progress() {
    let mut log = ConsoleLog::new();
    for i in 0..50 {
        log.ingest(&entry(&format!("{i}-{}", "p".repeat(400))), "");
    }
    assert_eq!(log.read_after(0, 7, usize::MAX).len(), 7);
    let page = log.read_after(0, 100, 3000);
    assert!(!page.is_empty() && page.len() < 10, "{}", page.len());
    // A budget smaller than one entry still returns that entry, so a reader cannot stall.
    assert_eq!(log.read_after(0, 100, 1).len(), 1);
    let mut cursor = 0;
    let mut seen = 0;
    loop {
        let batch = log.read_after(cursor, 8, 2048);
        let Some(last) = batch.last() else { break };
        cursor = last.seq;
        seen += batch.len();
    }
    assert_eq!(seen, 50);
}

#[test]
fn clearing_empties_the_ring_but_sequence_numbers_keep_growing() {
    let mut log = ConsoleLog::new();
    log.ingest(&entry("a"), "");
    log.clear();
    assert_eq!((log.len(), log.bytes()), (0, 0));
    log.ingest(&entry("b"), "");
    assert_eq!(log.read_after(0, 10, usize::MAX)[0].seq, 2);
}

#[test]
fn uncaught_errors_keep_name_message_stack_and_first_frame() {
    let mut log = ConsoleLog::new();
    log.exception(
        ExceptionKind::Uncaught,
        "ReferenceError: nope is not defined\n    at run (https://e.test/app.js:12:5)\n    at https://e.test/app.js:20:1",
        "https://e.test/",
    );
    let message = log.read_after(0, 1, usize::MAX).remove(0);
    assert_eq!(message.level, ConsoleLevel::Error);
    let exception = message.exception.unwrap();
    assert_eq!(exception.kind, ExceptionKind::Uncaught);
    assert_eq!(exception.name, "ReferenceError");
    assert_eq!(exception.message, "nope is not defined");
    assert!(exception.stack.unwrap().contains("app.js:20:1"));
    let source = exception.source.unwrap();
    assert_eq!(
        (source.url.as_str(), source.line, source.column),
        ("https://e.test/app.js", 12, 5)
    );
    assert!(message.text.starts_with("ReferenceError: nope"));
}

#[test]
fn exception_reports_in_other_shapes_still_produce_useful_entries() {
    // Script-host prefix and "Uncaught".
    let (name, message, _, source) = parse_report(
        "inline script: Uncaught TypeError: x.y is not a function\n    at inline:4:9",
        "https://e.test/",
    );
    assert_eq!(
        (name.as_str(), message.as_str()),
        ("TypeError", "x.y is not a function")
    );
    assert_eq!(source.unwrap().line, 4);
    // A thrown string has no name.
    let (name, message, stack, source) = parse_report("plain failure\n", "https://e.test/page");
    assert_eq!(
        (name.as_str(), message.as_str()),
        ("Error", "plain failure")
    );
    assert!(stack.is_none());
    assert_eq!(
        source.unwrap().url,
        "https://e.test/page",
        "falls back to the document URL"
    );
    // Hostile first line and garbage frames never panic or overflow budgets.
    let hostile = format!(
        "{}: {}\nat ((((:::\n   at x:y:z\n{}",
        "N".repeat(500),
        "m".repeat(100_000),
        "s\n".repeat(100_000)
    );
    let (name, message, stack, _) = parse_report(&hostile, "");
    assert_eq!(name, "Error");
    assert!(message.len() <= 2048 && stack.unwrap().len() <= 16 * 1024);
    // Windows-style paths with drive letters.
    let (_, _, _, source) = parse_report("Error: x\n    at f (file:///C:/p/a.js:7:3)", "");
    let source = source.unwrap();
    assert_eq!(
        (source.url.as_str(), source.line, source.column),
        ("file:///C:/p/a.js", 7, 3)
    );
}

#[test]
fn exceptions_count_in_the_same_bounded_ring() {
    let mut log = ConsoleLog::new();
    for _ in 0..(CONSOLE_RING_ENTRIES * 2) {
        log.exception(
            ExceptionKind::UnhandledRejection,
            "Error: x\n    at a:1:1",
            "",
        );
    }
    assert_eq!(log.len(), CONSOLE_RING_ENTRIES);
    assert!(
        log.read_after(0, 1, usize::MAX)[0]
            .exception
            .as_ref()
            .unwrap()
            .kind
            == ExceptionKind::UnhandledRejection
    );
}

#[test]
fn byte_accounting_matches_the_stored_messages_after_a_single_size_pass() {
    // The ring charges each message once, at its serialised size when stored. The only
    // difference from re-measuring afterwards is the digits of the sequence number.
    let mut log = ConsoleLog::new();
    let mut shrunk = 0;
    for i in 0..400 {
        // Some entries are over one message's budget and go through shrinking.
        let text = "w".repeat(if i % 7 == 0 { 3000 } else { 40 });
        let wide: Vec<_> = (0..8)
            .map(|k| serde_json::json!({"key": format!("k{k}"), "value": {"kind": "String", "text": text.clone()}}))
            .collect();
        let json = serde_json::json!({"m":"log","l":"log","t":text,
            "a": (0..6).map(|_| serde_json::json!({"kind":"Object","text":"Object","props":wide.clone()})).collect::<Vec<_>>()})
        .to_string();
        let before = log.len();
        log.ingest(&json, "");
        shrunk += usize::from(log.len() > before);
    }
    assert!(shrunk > 0);
    let stored = log.read_after(0, usize::MAX, usize::MAX);
    let measured: usize = stored.iter().map(|m| m.approx_bytes()).sum();
    assert!(measured >= log.bytes(), "{measured} < {}", log.bytes());
    assert!(
        measured - log.bytes() <= stored.len() * 8,
        "accounting drifted: {} vs {measured}",
        log.bytes()
    );
    assert!(
        stored
            .iter()
            .all(|m| m.approx_bytes() <= MAX_MESSAGE_BYTES + 8)
    );
}
