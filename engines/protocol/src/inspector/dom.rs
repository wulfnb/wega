//! Values for the read-only light-DOM inspector. IDs are document scoped, generation
//! bearing keys, never indexes. Target and session are checked by the enclosing protocol.
use crate::DocumentContext;
use serde::{Deserialize, Serialize};

pub const MAX_DOM_NODES: usize = 64;
pub const MAX_DOM_DEPTH: usize = 128;
pub const MAX_DOM_ATTRIBUTES: usize = 32;
pub const MAX_DOM_TEXT: usize = 256;
pub const MAX_DOM_BYTES: usize = 64 * 1024;
pub const DOM_WORK_MS: u64 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InspectorNodeId {
    pub document: DocumentContext,
    pub key: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomAttribute {
    pub name: String,
    pub namespace: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomNode {
    pub id: InspectorNodeId,
    pub node_type: u8,
    pub name: String,
    pub namespace: String,
    pub prefix: String,
    pub text: String,
    pub child_count: u32,
    pub attributes: Vec<DomAttribute>,
    /// Any string/attribute limit was reached. The UI must not present previews as complete.
    pub truncated: bool,
}
impl DomNode {
    pub fn is_bounded(&self) -> bool {
        [1, 3, 8, 9, 11].contains(&self.node_type)
            && [&self.name, &self.namespace, &self.prefix, &self.text]
                .iter()
                .all(|s| s.len() <= MAX_DOM_TEXT)
            && self.attributes.len() <= MAX_DOM_ATTRIBUTES
            && self.attributes.iter().all(|a| {
                [&a.name, &a.namespace, &a.value]
                    .iter()
                    .all(|s| s.len() <= MAX_DOM_TEXT)
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomPage {
    pub document: DocumentContext,
    pub epoch: u64,
    /// None for root or description, Some for a child page.
    pub parent: Option<InspectorNodeId>,
    pub offset: u32,
    /// Offset in the authoritative child list; may advance past hidden anonymous boxes.
    pub next_offset: Option<u32>,
    pub nodes: Vec<DomNode>,
}
impl DomPage {
    pub fn is_bounded(&self) -> bool {
        self.nodes.len() <= MAX_DOM_NODES
            && self
                .nodes
                .iter()
                .map(|n| n.id.key)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.nodes.len()
            && self
                .parent
                .is_none_or(|p| self.nodes.iter().all(|n| n.id != p))
            && self.parent.is_none_or(|p| p.document == self.document)
            && self.next_offset.is_none_or(|n| n > self.offset)
            && self
                .nodes
                .iter()
                .all(|n| n.id.document == self.document && n.is_bounded())
            && serde_json::to_vec(self).is_ok_and(|s| s.len() <= MAX_DOM_BYTES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{FrameId, ProcessSessionId, TabId};
    use crate::inspector::*;
    fn request(version: u32, command: InspectorCommand, args: InspectorArgs) -> InspectorRequest {
        InspectorRequest {
            version,
            session: InspectorSessionId(1),
            target: TargetId {
                tab: TabId(1),
                frame: FrameId(1),
                process: ProcessSessionId([1; 16]),
            },
            command,
            args,
        }
    }
    #[test]
    fn version_shape_and_page_limits_are_enforced() {
        let node = InspectorNodeId {
            document: DocumentContext {
                document: crate::DocumentId(1),
                revision: crate::DocumentRevision(1),
            },
            key: 1,
        };
        assert_eq!(
            request(2, InspectorCommand::GetDomRoot, InspectorArgs::None)
                .validate()
                .unwrap_err()
                .code,
            InspectorErrorCode::UnsupportedVersion
        );
        for limit in [0, 65, u32::MAX] {
            assert!(
                request(
                    3,
                    InspectorCommand::GetDomChildren,
                    InspectorArgs::DomChildren {
                        node,
                        epoch: 0,
                        offset: 0,
                        limit
                    }
                )
                .validate()
                .is_err()
            );
        }
        assert!(
            request(
                3,
                InspectorCommand::GetDomRoot,
                InspectorArgs::DomNode { node }
            )
            .validate()
            .is_err()
        );
        let req = request(
            3,
            InspectorCommand::GetDomChildren,
            InspectorArgs::DomChildren {
                node,
                epoch: 5,
                offset: u32::MAX,
                limit: 64,
            },
        );
        assert_eq!(InspectorPayload::new(&req).decode(), Ok(req));
    }
    #[test]
    fn responses_reject_foreign_documents_and_oversized_fields() {
        let document = DocumentContext {
            document: crate::DocumentId(1),
            revision: crate::DocumentRevision(1),
        };
        let mut page = DomPage {
            document,
            epoch: 1,
            parent: None,
            offset: 0,
            next_offset: None,
            nodes: vec![DomNode {
                id: InspectorNodeId { document, key: 1 },
                node_type: 9,
                name: "#document".into(),
                namespace: String::new(),
                prefix: String::new(),
                text: String::new(),
                child_count: 0,
                attributes: vec![],
                truncated: false,
            }],
        };
        assert!(page.is_bounded());
        page.nodes[0].name = "x".repeat(257);
        assert!(!page.is_bounded());
        page.nodes[0].name.clear();
        page.nodes[0].id.document.document = crate::DocumentId(2);
        assert!(!page.is_bounded());
    }
}
