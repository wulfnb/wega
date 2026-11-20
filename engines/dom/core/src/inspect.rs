//! Bounded, read-only CSS snapshots. The document remains the only style authority.
use crate::NodeKey;
use std::{
    fmt,
    time::{Duration, Instant},
};
pub const STYLE_PAGE: usize = 32;
pub const STYLE_TEXT: usize = 512;
pub const STYLE_PATH: usize = 16;
pub const STYLE_SHEETS: usize = 512;
pub const STYLE_WORK_MS: u64 = 10;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleReadError {
    Stale,
    Invalid,
    Depth,
    Timeout,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Available,
    Cached,
    NotComputed,
    Unsupported,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    pub url: String,
    pub line: u32,
    pub column: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchedSelector {
    pub selector: String,
    pub specificity: Option<[u32; 3]>,
    pub source: Option<SourceLocation>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetInfo {
    pub owner: NodeKey,
    pub url: String,
    pub disabled: bool,
    pub rules: u32,
    pub truncated: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleInfo {
    pub path: Vec<u32>,
    pub kind: String,
    pub selector: Option<String>,
    pub children: u32,
    pub declarations: u32,
    pub source: Option<SourceLocation>,
    pub truncated: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub name: String,
    pub value: String,
    pub important: bool,
    pub truncated: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StyleItem {
    Sheet(SheetInfo),
    Rule(RuleInfo),
    Declaration(Declaration),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StyleRead {
    Sheets,
    Rules { owner: NodeKey, path: Vec<u32> },
    Declarations { owner: NodeKey, path: Vec<u32> },
    Inline(NodeKey),
    Computed(NodeKey),
    Matched(NodeKey),
}
#[derive(Clone, Debug)]
pub struct StylePage {
    pub availability: Availability,
    pub layout_pending: bool,
    pub generation: u64,
    pub total: u32,
    pub next: Option<u32>,
    pub items: Vec<StyleItem>,
    pub truncated: bool,
}
pub trait StyleInspect: crate::MutationClock {
    fn style_generation(&self) -> u64;
    fn read_style(&self, query: &StyleRead, offset: u32) -> Result<StylePage, StyleReadError>;
}
/// Formatting stops at its first size/deadline refusal, before allocating large values.
pub struct BoundedCss {
    pub text: String,
    pub truncated: bool,
    start: Instant,
}
impl BoundedCss {
    pub fn new(start: Instant) -> Self {
        Self {
            text: String::new(),
            truncated: false,
            start,
        }
    }
    pub fn check(start: Instant) -> Result<(), StyleReadError> {
        if start.elapsed() >= Duration::from_millis(STYLE_WORK_MS) {
            Err(StyleReadError::Timeout)
        } else {
            Ok(())
        }
    }
    pub fn preview(value: &str) -> (String, bool) {
        let mut out = Self::new(Instant::now());
        let _ = fmt::Write::write_str(&mut out, value);
        (out.text, out.truncated)
    }
}
impl fmt::Write for BoundedCss {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if Self::check(self.start).is_err() {
            return Err(fmt::Error);
        }
        let mut end = value.len().min(STYLE_TEXT.saturating_sub(self.text.len()));
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        self.text.push_str(&value[..end]);
        if end < value.len() {
            self.truncated = true;
            Err(fmt::Error)
        } else {
            Ok(())
        }
    }
}
