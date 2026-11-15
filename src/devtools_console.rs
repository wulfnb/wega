//! The Console panel's model: a bounded per-document history, filters, command history and the
//! state of the (explicitly authorised) evaluation. It holds only engine data that already
//! passed the session's target and document checks, never an engine handle.
// TEMPORARY: wired into the Console panel in the next commit.
#![allow(dead_code)]
use std::{collections::VecDeque, time::Instant};
use wega::engine_protocol::{
    DocumentContext,
    inspector::{ConsoleLevel, ConsoleMessage, ExceptionDetails, Preview},
};

pub(crate) const HISTORY_ENTRIES: usize = 1000;
pub(crate) const HISTORY_BYTES: usize = 1024 * 1024;
pub(crate) const COMMAND_HISTORY: usize = 100;
const LOCAL_LINE_BYTES: usize = 4096;

#[derive(Clone, Debug)]
pub(crate) enum ConsoleLine {
    Message(Box<ConsoleMessage>),
    /// What the user ran.
    Input(String),
    Result(Preview),
    Failure(Box<ExceptionDetails>),
    /// A status line from the tools themselves (not page output).
    Notice(String),
}

impl ConsoleLine {
    fn bytes(&self) -> usize {
        match self {
            Self::Message(message) => message.approx_bytes(),
            Self::Input(text) | Self::Notice(text) => text.len() + 16,
            Self::Result(preview) => preview.text.len() + 64 + 64 * preview.props.len(),
            Self::Failure(details) => {
                details.message.len() + details.stack.as_ref().map_or(0, String::len) + 64
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct ConsoleView {
    lines: VecDeque<(ConsoleLine, usize)>,
    bytes: usize,
    /// Entries lost anywhere between the page and this history (ring, queue or this cap).
    pub dropped: u64,
    pub filter: String,
    /// Debug, Log, Info, Warn, Error.
    pub levels: [bool; 5],
    pub input: String,
    commands: VecDeque<String>,
    recall: Option<usize>,
    /// The engine accepted `EnableConsole` for the current session.
    pub enabled: bool,
    /// Set while an evaluation is outstanding.
    pub pending: Option<Instant>,
    /// The browser's own record of a live grant: the document it covers and whether it
    /// overrides the opaque-origin policy.
    pub grant: Option<(DocumentContext, bool)>,
    last_document: Option<DocumentContext>,
}

impl Default for ConsoleView {
    fn default() -> Self {
        Self {
            lines: VecDeque::new(),
            bytes: 0,
            dropped: 0,
            filter: String::new(),
            levels: [true; 5],
            input: String::new(),
            commands: VecDeque::new(),
            recall: None,
            enabled: false,
            pending: None,
            grant: None,
            last_document: None,
        }
    }
}

fn level_index(level: ConsoleLevel) -> usize {
    match level {
        ConsoleLevel::Debug => 0,
        ConsoleLevel::Log => 1,
        ConsoleLevel::Info => 2,
        ConsoleLevel::Warn => 3,
        ConsoleLevel::Error => 4,
    }
}

impl ConsoleView {
    pub fn len(&self) -> usize {
        self.lines.len()
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn lines(&self) -> impl Iterator<Item = &ConsoleLine> {
        self.lines.iter().map(|(line, _)| line)
    }

    pub fn push(&mut self, line: ConsoleLine) {
        let size = line.bytes().min(HISTORY_BYTES);
        while self.lines.len() >= HISTORY_ENTRIES || self.bytes + size > HISTORY_BYTES {
            let Some((old, bytes)) = self.lines.pop_front() else {
                break;
            };
            self.bytes -= bytes;
            // Only page output counts as a lost message; tool lines are not "messages".
            if matches!(old, ConsoleLine::Message(_)) {
                self.dropped = self.dropped.saturating_add(1);
            }
        }
        self.bytes += size;
        self.lines.push_back((line, size));
    }
    pub fn notice(&mut self, text: impl AsRef<str>) {
        let text: String = text.as_ref().chars().take(LOCAL_LINE_BYTES / 4).collect();
        self.push(ConsoleLine::Notice(text));
    }
    pub fn clear(&mut self) {
        self.lines.clear();
        self.bytes = 0;
        self.dropped = 0;
    }

    /// A new document starts a new history; commands typed for the old one are kept.
    pub fn new_document(&mut self, document: DocumentContext) {
        if self.last_document.is_some_and(|last| last != document) {
            self.clear();
            self.pending = None;
            self.notice("Navigated: the console history was cleared for the new document.");
        }
        self.last_document = Some(document);
    }
    /// A new session or process: nothing from the previous one may mix in (the engine will
    /// backfill from its own ring after `EnableConsole`).
    pub fn reset_session(&mut self) {
        self.clear();
        self.enabled = false;
        self.pending = None;
        self.grant = None;
        self.last_document = None;
    }

    /// The grant shown to the user: only one that covers the document being displayed.
    pub fn evaluation_enabled(&self, current: DocumentContext) -> Option<bool> {
        self.grant
            .filter(|(document, _)| *document == current)
            .map(|(_, opaque)| opaque)
    }

    pub fn visible(&self, line: &ConsoleLine) -> bool {
        let ConsoleLine::Message(message) = line else {
            return true;
        };
        if !self.levels[level_index(message.level)] {
            return false;
        }
        let needle = self.filter.trim().to_lowercase();
        needle.is_empty()
            || message.text.to_lowercase().contains(&needle)
            || message
                .args
                .iter()
                .any(|arg| arg.text.to_lowercase().contains(&needle))
            || message
                .source
                .as_ref()
                .is_some_and(|source| source.url.to_lowercase().contains(&needle))
    }

    pub fn remember_command(&mut self, command: &str) {
        self.recall = None;
        if self.commands.back().is_some_and(|last| last == command) {
            return;
        }
        if self.commands.len() == COMMAND_HISTORY {
            self.commands.pop_front();
        }
        self.commands.push_back(command.to_string());
    }
    /// Up arrow: older commands. Returns the text to place in the input box.
    pub fn recall_older(&mut self) -> Option<&str> {
        let next = match self.recall {
            None => self.commands.len().checked_sub(1)?,
            Some(0) => 0,
            Some(at) => at - 1,
        };
        self.recall = Some(next);
        self.commands.get(next).map(String::as_str)
    }
    /// Down arrow: newer commands, then an empty input.
    pub fn recall_newer(&mut self) -> Option<&str> {
        match self.recall {
            None => None,
            Some(at) if at + 1 < self.commands.len() => {
                self.recall = Some(at + 1);
                self.commands.get(at + 1).map(String::as_str)
            }
            Some(_) => {
                self.recall = None;
                Some("")
            }
        }
    }
    pub fn commands(&self) -> usize {
        self.commands.len()
    }
}

#[cfg(test)]
#[path = "devtools_console_tests.rs"]
mod tests;
