use super::*;
use wega::engine_protocol::{
    DocumentId, DocumentRevision,
    inspector::{ConsoleLevel, ExceptionKind, PreviewKind, SourceLocation},
};

fn doc(n: u64) -> DocumentContext {
    DocumentContext {
        document: DocumentId(n),
        revision: DocumentRevision(n),
    }
}
fn message(seq: u64, level: ConsoleLevel, text: &str) -> ConsoleLine {
    ConsoleLine::Message(Box::new(ConsoleMessage {
        seq,
        timestamp_ms: 1,
        level,
        method: "log".into(),
        args: vec![Preview::simple(PreviewKind::String, text)],
        text: text.into(),
        source: Some(SourceLocation {
            url: "https://e.test/app.js".into(),
            line: 1,
            column: 1,
        }),
        exception: None,
    }))
}

#[test]
fn history_is_bounded_by_entries_and_counts_what_it_drops() {
    let mut console = ConsoleView::default();
    for i in 0..(HISTORY_ENTRIES * 3) as u64 {
        console.push(message(i, ConsoleLevel::Log, "m"));
        assert!(console.len() <= HISTORY_ENTRIES && console.bytes() <= HISTORY_BYTES);
    }
    assert_eq!(console.len(), HISTORY_ENTRIES);
    assert_eq!(console.dropped as usize, HISTORY_ENTRIES * 2);
}

#[test]
fn history_is_bounded_by_bytes_when_entries_are_large() {
    let mut console = ConsoleView::default();
    let big = "x".repeat(6000);
    for i in 0..2000 {
        console.push(message(i, ConsoleLevel::Log, &big));
        assert!(console.bytes() <= HISTORY_BYTES);
    }
    assert!(console.len() < HISTORY_ENTRIES);
    assert!(console.dropped > 0);
}

#[test]
fn tool_lines_do_not_count_as_lost_page_messages() {
    let mut console = ConsoleView::default();
    for _ in 0..(HISTORY_ENTRIES + 10) {
        console.notice("tool");
    }
    assert_eq!(console.dropped, 0);
    assert_eq!(console.len(), HISTORY_ENTRIES);
}

#[test]
fn filters_match_text_arguments_and_source_and_respect_levels() {
    let mut console = ConsoleView::default();
    let warn = message(1, ConsoleLevel::Warn, "Disk Almost Full");
    let log = message(2, ConsoleLevel::Log, "hello");
    console.filter = "disk".into();
    assert!(console.visible(&warn) && !console.visible(&log));
    console.filter = "APP.JS".into();
    assert!(
        console.visible(&warn) && console.visible(&log),
        "matches the source URL"
    );
    console.filter.clear();
    console.levels[3] = false;
    assert!(!console.visible(&warn) && console.visible(&log));
    // Tool lines are never filtered away.
    assert!(console.visible(&ConsoleLine::Notice("x".into())));
}

#[test]
fn a_new_document_clears_the_history_but_keeps_the_commands() {
    let mut console = ConsoleView::default();
    console.new_document(doc(1));
    console.push(message(1, ConsoleLevel::Log, "old"));
    console.remember_command("1+1");
    console.new_document(doc(1));
    assert_eq!(console.len(), 1, "the same document keeps its history");
    console.pending = Some(Instant::now());
    console.new_document(doc(2));
    assert!(
        console.pending.is_none(),
        "an evaluation of the old document is abandoned"
    );
    assert_eq!(console.dropped, 0);
    assert!(console.lines().all(|l| matches!(l, ConsoleLine::Notice(_))));
    assert_eq!(console.commands(), 1);
}

#[test]
fn a_new_session_starts_from_nothing() {
    let mut console = ConsoleView::default();
    console.new_document(doc(1));
    console.push(message(1, ConsoleLevel::Log, "x"));
    console.enabled = true;
    console.grant = Some((doc(1), false));
    console.reset_session();
    assert_eq!(
        (console.len(), console.enabled, console.grant),
        (0, false, None)
    );
}

#[test]
fn the_grant_is_shown_only_for_the_document_it_covers() {
    let mut console = ConsoleView {
        grant: Some((doc(3), true)),
        ..Default::default()
    };
    assert_eq!(console.evaluation_enabled(doc(3)), Some(true));
    assert_eq!(console.evaluation_enabled(doc(4)), None);
    console.grant = None;
    assert_eq!(console.evaluation_enabled(doc(3)), None);
}

#[test]
fn command_history_recalls_older_and_newer_and_is_bounded() {
    let mut console = ConsoleView::default();
    assert_eq!(console.recall_older(), None);
    for command in ["a", "b", "c"] {
        console.remember_command(command);
    }
    console.remember_command("c");
    assert_eq!(
        console.commands(),
        3,
        "an immediate repeat is not stored twice"
    );
    assert_eq!(console.recall_older(), Some("c"));
    assert_eq!(console.recall_older(), Some("b"));
    assert_eq!(console.recall_older(), Some("a"));
    assert_eq!(console.recall_older(), Some("a"), "stops at the oldest");
    assert_eq!(console.recall_newer(), Some("b"));
    assert_eq!(console.recall_newer(), Some("c"));
    assert_eq!(
        console.recall_newer(),
        Some(""),
        "past the newest is an empty input"
    );
    assert_eq!(console.recall_newer(), None);
    for i in 0..(COMMAND_HISTORY + 50) {
        console.remember_command(&format!("cmd {i}"));
    }
    assert_eq!(console.commands(), COMMAND_HISTORY);
}

#[test]
fn local_lines_are_bounded_too() {
    let mut console = ConsoleView::default();
    console.notice("n".repeat(1_000_000));
    console.push(ConsoleLine::Failure(Box::new(ExceptionDetails {
        kind: ExceptionKind::Evaluation,
        name: "Error".into(),
        message: "m".repeat(8000),
        stack: Some("s".repeat(16000)),
        source: None,
    })));
    assert!(console.bytes() <= HISTORY_BYTES);
}
