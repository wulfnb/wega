//! Read-only style values, independently versioned and authorized by the inspector envelope.
use super::{InspectorSessionId, TargetId, dom::InspectorNodeId};
use crate::DocumentContext;
use serde::{Deserialize, Serialize};
pub const STYLE_VERSION: u32 = 4;
pub const STYLE_PAGE: usize = 32;
pub const STYLE_TEXT: usize = 512;
pub const STYLE_PATH: usize = 16;
pub const STYLE_BYTES: usize = 64 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleScope {
    pub target: TargetId,
    pub session: InspectorSessionId,
    pub document: DocumentContext,
    pub epoch: u64,
    pub generation: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleRef {
    pub scope: StyleScope,
    pub owner: InspectorNodeId,
    pub path: Vec<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StyleQuery {
    Sheets { offset: u32 },
    Rules { parent: RuleRef, offset: u32 },
    Declarations { rule: RuleRef, offset: u32 },
    Inline { node: InspectorNodeId, offset: u32 },
    Computed { node: InspectorNodeId, offset: u32 },
    Matched { node: InspectorNodeId },
}
impl StyleQuery {
    pub fn offset(&self) -> u32 {
        match self {
            Self::Sheets { offset }
            | Self::Rules { offset, .. }
            | Self::Declarations { offset, .. }
            | Self::Inline { offset, .. }
            | Self::Computed { offset, .. } => *offset,
            Self::Matched { .. } => 0,
        }
    }
    pub fn at(&self, offset: u32) -> Self {
        let mut q = self.clone();
        match &mut q {
            Self::Sheets { offset: o }
            | Self::Rules { offset: o, .. }
            | Self::Declarations { offset: o, .. }
            | Self::Inline { offset: o, .. }
            | Self::Computed { offset: o, .. } => *o = offset,
            _ => {}
        };
        q
    }
    pub fn node(&self) -> Option<InspectorNodeId> {
        match self {
            Self::Inline { node, .. } | Self::Computed { node, .. } | Self::Matched { node } => {
                Some(*node)
            }
            _ => None,
        }
    }
    pub fn reference(&self) -> Option<&RuleRef> {
        match self {
            Self::Rules { parent, .. } => Some(parent),
            Self::Declarations { rule, .. } => Some(rule),
            _ => None,
        }
    }
    pub fn valid(&self) -> bool {
        self.reference()
            .is_none_or(|r| r.path.len() <= STYLE_PATH && r.owner.document == r.scope.document)
            && !matches!(self,Self::Declarations{rule,..} if rule.path.is_empty())
            && !matches!(self,Self::Sheets{offset} if *offset>512)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StyleAvailability {
    Available,
    Cached,
    NotComputed,
    Unsupported,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleSource {
    pub url: String,
    pub line: u32,
    pub column: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchedSelector {
    pub selector: String,
    pub specificity: Option<[u32; 3]>,
    pub source: Option<StyleSource>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StyleItem {
    Sheet {
        reference: RuleRef,
        url: String,
        disabled: bool,
        rules: u32,
        truncated: bool,
    },
    Rule {
        reference: RuleRef,
        kind: String,
        selector: Option<String>,
        children: u32,
        declarations: u32,
        source: Option<StyleSource>,
        truncated: bool,
    },
    Declaration {
        name: String,
        value: String,
        important: Option<bool>,
        truncated: bool,
    },
}
impl StyleItem {
    fn bounded(&self, scope: StyleScope) -> bool {
        let reference = match self {
            Self::Sheet { reference, .. } | Self::Rule { reference, .. } => Some(reference),
            _ => None,
        };
        if reference.is_some_and(|r| {
            r.scope != scope || r.owner.document != scope.document || r.path.len() > STYLE_PATH
        }) {
            return false;
        }
        match self {
            Self::Sheet { url, .. } => url.len() <= STYLE_TEXT,
            Self::Rule {
                kind,
                selector,
                source,
                ..
            } => {
                kind.len() <= STYLE_TEXT
                    && selector.as_ref().is_none_or(|s| s.len() <= STYLE_TEXT)
                    && source.as_ref().is_none_or(|s| s.url.len() <= STYLE_TEXT)
            }
            Self::Declaration { name, value, .. } => {
                name.len() <= STYLE_TEXT && value.len() <= STYLE_TEXT
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StylePage {
    pub scope: StyleScope,
    pub query: StyleQuery,
    pub availability: StyleAvailability,
    pub layout_pending: bool,
    pub total: u32,
    pub next: Option<u32>,
    pub items: Vec<StyleItem>,
    pub truncated: bool,
    pub note: String,
}
impl StylePage {
    pub fn is_bounded(&self) -> bool {
        self.query.valid()
            && self.query.reference().is_none_or(|r| r.scope == self.scope)
            && self
                .query
                .node()
                .is_none_or(|n| n.document == self.scope.document)
            && self.items.len() <= STYLE_PAGE
            && self.note.len() <= STYLE_TEXT
            && self.items.iter().all(|i| i.bounded(self.scope))
            && self
                .next
                .is_none_or(|n| n > self.query.offset() && n <= self.total)
            && self.items.iter().all(|i| match &self.query {
                StyleQuery::Sheets { .. } => matches!(i, StyleItem::Sheet { .. }),
                StyleQuery::Rules { .. } => matches!(i, StyleItem::Rule { .. }),
                StyleQuery::Matched { .. } => false,
                _ => matches!(i, StyleItem::Declaration { .. }),
            })
            && serde_json::to_vec(self).is_ok_and(|s| s.len() <= STYLE_BYTES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DocumentId, DocumentRevision,
        identity::{FrameId, ProcessSessionId, TabId},
    };
    fn scope() -> StyleScope {
        StyleScope {
            target: TargetId {
                tab: TabId(1),
                frame: FrameId(1),
                process: ProcessSessionId([1; 16]),
            },
            session: InspectorSessionId(1),
            document: DocumentContext {
                document: DocumentId(1),
                revision: DocumentRevision(1),
            },
            epoch: 1,
            generation: 1,
        }
    }
    #[test]
    fn references_are_bounded_and_declaration_paths_cannot_be_empty() {
        let s = scope();
        let r = RuleRef {
            scope: s,
            owner: InspectorNodeId {
                document: s.document,
                key: 1,
            },
            path: vec![],
        };
        assert!(
            !StyleQuery::Declarations {
                rule: r.clone(),
                offset: 0
            }
            .valid()
        );
        let mut r = r;
        r.path = vec![0; 17];
        assert!(
            !StyleQuery::Rules {
                parent: r,
                offset: 0
            }
            .valid()
        );
        assert!(!StyleQuery::Sheets { offset: 513 }.valid());
    }
    #[test]
    fn consumer_rejects_foreign_scope_items_and_oversized_values() {
        let s = scope();
        let mut page = StylePage {
            scope: s,
            query: StyleQuery::Sheets { offset: 0 },
            availability: StyleAvailability::Available,
            layout_pending: false,
            total: 1,
            next: None,
            items: vec![],
            truncated: false,
            note: String::new(),
        };
        assert!(page.is_bounded());
        let mut foreign = s;
        foreign.session = InspectorSessionId(2);
        page.items.push(StyleItem::Sheet {
            reference: RuleRef {
                scope: foreign,
                owner: InspectorNodeId {
                    document: s.document,
                    key: 1,
                },
                path: vec![],
            },
            url: String::new(),
            disabled: false,
            rules: 0,
            truncated: false,
        });
        assert!(!page.is_bounded());
        page.items.clear();
        page.note = "x".repeat(513);
        assert!(!page.is_bounded());
    }
    #[test]
    fn style_command_requires_version_four() {
        use crate::inspector::*;
        let s = scope();
        let mut r = InspectorRequest {
            version: 3,
            session: s.session,
            target: s.target,
            command: InspectorCommand::InspectStyle,
            args: InspectorArgs::Style {
                query: StyleQuery::Sheets { offset: 0 },
            },
        };
        assert_eq!(
            r.validate().unwrap_err().code,
            InspectorErrorCode::UnsupportedVersion
        );
        r.version = 4;
        assert_eq!(InspectorPayload::new(&r).decode(), Ok(r));
    }
}
