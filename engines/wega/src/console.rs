//! Per-document console history. Created beside the document's script host and dropped with
//! it, so nothing outlives a navigation. It collects whether or not DevTools is open, within
//! hard entry and byte budgets, so a hostile page cannot grow it.
use serde::Deserialize;
use std::collections::VecDeque;
use wega_protocol::inspector::{
    CONSOLE_RING_BYTES, CONSOLE_RING_ENTRIES, ConsoleLevel, ConsoleMessage, ExceptionDetails,
    ExceptionKind, MAX_ARGS, MAX_MESSAGE_BYTES, Preview, PreviewKind, SourceLocation,
};

/// A page-supplied entry larger than this is rejected before parsing.
const MAX_ENTRY_JSON: usize = 64 * 1024;

#[derive(Debug, Default)]
pub struct ConsoleLog {
    entries: VecDeque<(ConsoleMessage, usize)>,
    bytes: usize,
    next_seq: u64,
    /// Entries evicted or rejected since this document began.
    dropped: u64,
}

/// What the page-side wrapper sends. Everything is validated again here: a page can call
/// the host op itself, so this is untrusted input.
#[derive(Deserialize)]
struct PageEntry {
    m: String,
    l: String,
    #[serde(default)]
    a: Vec<Preview>,
    #[serde(default)]
    t: String,
    #[serde(default)]
    u: Option<String>,
    #[serde(default)]
    n: u32,
    #[serde(default)]
    c: u32,
}

fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn level_of(name: &str) -> ConsoleLevel {
    match name {
        "debug" => ConsoleLevel::Debug,
        "info" => ConsoleLevel::Info,
        "warn" => ConsoleLevel::Warn,
        "error" => ConsoleLevel::Error,
        _ => ConsoleLevel::Log,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

impl ConsoleLog {
    pub fn new() -> Self {
        Self {
            next_seq: 1,
            ..Default::default()
        }
    }
    /// Sequence of the oldest retained entry, or the next one when empty.
    pub fn first_seq(&self) -> u64 {
        self.entries.front().map_or(self.next_seq, |(m, _)| m.seq)
    }
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Shrink a message until it fits one message's budget: previews first, then text.
    fn shrink(message: &mut ConsoleMessage) {
        if message.approx_bytes() <= MAX_MESSAGE_BYTES {
            return;
        }
        for arg in &mut message.args {
            arg.props.clear();
            arg.truncated = true;
        }
        if message.approx_bytes() <= MAX_MESSAGE_BYTES {
            return;
        }
        message.args.truncate(4);
        message.text = clip(&message.text, 1024);
        if let Some(exception) = &mut message.exception {
            exception.stack = exception.stack.as_ref().map(|s| clip(s, 2048));
            exception.message = clip(&exception.message, 1024);
        }
        if message.approx_bytes() > MAX_MESSAGE_BYTES {
            message.args.clear();
            message.text = clip(&message.text, 256);
        }
    }

    fn push(&mut self, mut message: ConsoleMessage) {
        Self::shrink(&mut message);
        let size = message.approx_bytes();
        if size > CONSOLE_RING_BYTES {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        message.seq = self.next_seq;
        self.next_seq += 1;
        while self.entries.len() >= CONSOLE_RING_ENTRIES || self.bytes + size > CONSOLE_RING_BYTES {
            let Some((_, old)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= old;
            self.dropped = self.dropped.saturating_add(1);
        }
        self.bytes += size;
        self.entries.push_back((message, size));
    }

    /// An entry from the page-side console wrapper (untrusted).
    pub fn ingest(&mut self, json: &str, document_url: &str) {
        if json.len() > MAX_ENTRY_JSON {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        let Ok(entry) = serde_json::from_str::<PageEntry>(json) else {
            self.dropped = self.dropped.saturating_add(1);
            return;
        };
        let args = entry
            .a
            .into_iter()
            .take(MAX_ARGS)
            .map(|preview| {
                if preview.is_bounded() {
                    preview
                } else {
                    Preview::simple(PreviewKind::Unavailable, "(preview exceeded its budget)")
                }
            })
            .collect();
        let source = entry
            .u
            .filter(|url| !url.is_empty())
            .map(|url| SourceLocation {
                url: clip(&url, 4096),
                line: entry.n,
                column: entry.c,
            });
        let _ = document_url;
        self.push(ConsoleMessage {
            seq: 0,
            timestamp_ms: now_ms(),
            level: level_of(&entry.l),
            method: clip(&entry.m, 32),
            args,
            text: clip(&entry.t, 1024),
            source,
            exception: None,
        });
    }

    /// An uncaught error or unhandled rejection, from the text the runtime already reports
    /// (`Name: message` then stack lines). Nothing here calls page code.
    pub fn exception(&mut self, kind: ExceptionKind, report: &str, document_url: &str) {
        let (name, message, stack, source) = parse_report(report, document_url);
        self.push(ConsoleMessage {
            seq: 0,
            timestamp_ms: now_ms(),
            level: ConsoleLevel::Error,
            method: "exception".into(),
            args: Vec::new(),
            text: clip(&format!("{name}: {message}"), 1024),
            source: source.clone(),
            exception: Some(ExceptionDetails {
                kind,
                name,
                message,
                stack,
                source,
            }),
        });
    }

    /// Entries with `seq > after`, within the entry and byte budget.
    pub fn read_after(
        &self,
        after: u64,
        max_entries: usize,
        max_bytes: usize,
    ) -> Vec<ConsoleMessage> {
        let mut out = Vec::new();
        let mut bytes = 0;
        for (message, size) in &self.entries {
            if message.seq <= after {
                continue;
            }
            if out.len() >= max_entries || (bytes + size > max_bytes && !out.is_empty()) {
                break;
            }
            bytes += size;
            out.push(message.clone());
        }
        out
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

/// `(name, message, stack, first frame)` from `Name: message\n    at fn (url:line:col)...`.
pub(crate) fn parse_report(
    report: &str,
    document_url: &str,
) -> (String, String, Option<String>, Option<SourceLocation>) {
    let report = report.trim_start();
    let (head, rest) = report.split_once('\n').unwrap_or((report, ""));
    // The script host prefixes the script name ("<url> inline #1: ") and "Uncaught ".
    let head = head
        .find("Uncaught ")
        .map_or(head, |at| &head[at + "Uncaught ".len()..]);
    let (name, message) = split_report_head(head);
    let stack = (!rest.trim().is_empty()).then(|| clip(rest.trim_end(), 16 * 1024));
    let source = rest
        .lines()
        .chain(head.lines())
        .find_map(|line| frame_location(line.trim()));
    let source = source.or_else(|| {
        (!document_url.is_empty()).then(|| SourceLocation {
            url: clip(document_url, 4096),
            line: 0,
            column: 0,
        })
    });
    (clip(&name, 128), clip(&message, 2048), stack, source)
}

/// `(name, message)` from `[prefix: ]Name: message`, where `Name` is the first token before a
/// colon that looks like an error class (an identifier starting with an uppercase letter).
fn split_name(head: &str) -> (String, String) {
    let mut from = 0;
    while let Some(offset) = head[from..].find(": ") {
        let colon = from + offset;
        let before = &head[..colon];
        let token = before
            .rsplit(|c: char| c.is_whitespace())
            .next()
            .unwrap_or_default();
        if !token.is_empty()
            && token.len() <= 64
            && token.chars().next().is_some_and(char::is_uppercase)
            && token
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        {
            return (token.to_string(), head[colon + 2..].to_string());
        }
        from = colon + 2;
    }
    ("Error".to_string(), head.to_string())
}

/// `split_name`, preferring an inner class name over the generic `Error:` wrapper the script
/// host adds when it re-reports a script failure (`Error: <script>: TypeError: message`).
fn split_report_head(head: &str) -> (String, String) {
    let (name, message) = split_name(head);
    if name == "Error" {
        let (inner, rest) = split_name(&message);
        if inner != "Error" && (inner.ends_with("Error") || inner.ends_with("Exception")) {
            return (inner, rest);
        }
    }
    (name, message)
}

fn frame_location(line: &str) -> Option<SourceLocation> {
    let frame = line.strip_prefix("at ")?;
    let location = match (frame.rfind('('), frame.ends_with(')')) {
        (Some(open), true) => &frame[open + 1..frame.len() - 1],
        _ => frame,
    };
    let (rest, column) = location.rsplit_once(':')?;
    let (url, line) = rest.rsplit_once(':')?;
    let (line, column) = (line.parse().ok()?, column.parse().ok()?);
    (!url.is_empty()).then(|| SourceLocation {
        url: clip(url, 4096),
        line,
        column,
    })
}

#[cfg(test)]
mod tests;
