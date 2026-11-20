//! Stateless, bounded reads of Wega's authoritative document. No script, layout or
//! adapter types. No mirror, handle table, descendant text or recursive walk.
use std::time::{Duration, Instant};
use wega_dom::{DocumentView, MutationClock, NodeKey, NodeKind, NodeView};
use wega_protocol::{
    DocumentContext,
    inspector::{InspectorArgs, InspectorCommand, InspectorError, InspectorErrorCode, dom::*},
};

fn error(code: InspectorErrorCode, message: &str) -> InspectorError {
    InspectorError::new(code, message)
}
struct Budget(Instant);
impl Budget {
    fn check(&self) -> Result<(), InspectorError> {
        if self.0.elapsed() >= Duration::from_millis(DOM_WORK_MS) {
            Err(error(
                InspectorErrorCode::Timeout,
                "DOM work deadline exceeded",
            ))
        } else {
            Ok(())
        }
    }
}
fn preview(text: &str, truncated: &mut bool) -> String {
    let mut end = text.len().min(MAX_DOM_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    *truncated |= end < text.len();
    text[..end].to_owned()
}

/// Confirms connectivity to this root, without ever entering detached/shadow/frame
/// documents. Generation checks happen at every lookup, including ancestor lookups.
fn connected<D: DocumentView>(
    doc: &D,
    key: NodeKey,
    budget: &Budget,
) -> Result<usize, InspectorError> {
    let mut at = key;
    for depth in 0..=MAX_DOM_DEPTH {
        budget.check()?;
        let node = doc
            .node(at)
            .filter(|n| n.kind() != NodeKind::AnonymousBlock)
            .ok_or_else(|| error(InspectorErrorCode::StaleNode, "Node no longer inspectable"))?;
        if at == doc.root() {
            return Ok(depth);
        }
        at = node.parent().ok_or_else(|| {
            error(
                InspectorErrorCode::StaleNode,
                "Node is outside the connected light DOM",
            )
        })?;
    }
    Err(error(
        InspectorErrorCode::BudgetExceeded,
        "DOM ancestry depth limit reached",
    ))
}
fn describe<D: DocumentView>(
    doc: &D,
    key: NodeKey,
    document: DocumentContext,
    budget: &Budget,
) -> Result<DomNode, InspectorError> {
    budget.check()?;
    let node = doc
        .node(key)
        .ok_or_else(|| error(InspectorErrorCode::StaleNode, "Node was dropped"))?;
    let node_type = match node.kind() {
        NodeKind::Document => 9,
        NodeKind::Element => 1,
        NodeKind::Text => 3,
        NodeKind::Comment => 8,
        NodeKind::AnonymousBlock => {
            return Err(error(
                InspectorErrorCode::StaleNode,
                "Layout boxes are not DOM nodes",
            ));
        }
    };
    let mut truncated = false;
    let (name, namespace, prefix) = if let Some(name) = node.element_name() {
        (
            preview(name.local, &mut truncated),
            preview(name.namespace, &mut truncated),
            preview(name.prefix.unwrap_or(""), &mut truncated),
        )
    } else {
        (
            match node_type {
                9 => "#document",
                3 => "#text",
                _ => "#comment",
            }
            .to_owned(),
            String::new(),
            String::new(),
        )
    };
    let text = preview(node.text().unwrap_or(""), &mut truncated);
    let mut attributes = Vec::new();
    for (name, value) in node.attributes().take(MAX_DOM_ATTRIBUTES + 1) {
        budget.check()?;
        if attributes.len() == MAX_DOM_ATTRIBUTES {
            truncated = true;
            break;
        }
        let local = preview(&name.local, &mut truncated);
        let attr_name = if let Some(prefix) = &name.prefix {
            // Bound each source before formatting; hostile names cannot cause a large allocation.
            let prefix = preview(prefix, &mut truncated);
            preview(&format!("{prefix}:{local}"), &mut truncated)
        } else {
            local
        };
        attributes.push(DomAttribute {
            name: attr_name,
            namespace: preview(&name.ns, &mut truncated),
            value: preview(value, &mut truncated),
        });
    }
    Ok(DomNode {
        id: InspectorNodeId {
            document,
            key: key.as_u64(),
        },
        node_type,
        name,
        namespace,
        prefix,
        text,
        child_count: node.children().len().min(u32::MAX as usize) as u32,
        attributes,
        truncated,
    })
}

pub fn inspect<D: MutationClock>(
    doc: &D,
    document: DocumentContext,
    command: InspectorCommand,
    args: &InspectorArgs,
) -> Result<DomPage, InspectorError> {
    inspect_budget(doc, document, command, args, &Budget(Instant::now()))
}
fn inspect_budget<D: MutationClock>(
    doc: &D,
    document: DocumentContext,
    command: InspectorCommand,
    args: &InspectorArgs,
    budget: &Budget,
) -> Result<DomPage, InspectorError> {
    budget.check()?;
    let epoch = doc.mutation_epoch().as_u64();
    let mut page = DomPage {
        document,
        epoch,
        parent: None,
        offset: 0,
        next_offset: None,
        nodes: Vec::new(),
    };
    let key = match args {
        InspectorArgs::None if command == InspectorCommand::GetDomRoot => doc.root(),
        InspectorArgs::DomNode { node } | InspectorArgs::DomChildren { node, .. } => {
            if node.document != document {
                return Err(error(
                    InspectorErrorCode::StaleDocument,
                    "Node belongs to a replaced document",
                ));
            }
            NodeKey::from_u64(node.key)
        }
        _ => {
            return Err(error(
                InspectorErrorCode::InvalidPayload,
                "Missing DOM arguments",
            ));
        }
    };
    let depth = connected(doc, key, budget)?;
    if command == InspectorCommand::GetDomChildren {
        let InspectorArgs::DomChildren {
            node,
            epoch: expected,
            offset,
            limit,
        } = args
        else {
            return Err(error(
                InspectorErrorCode::InvalidPayload,
                "Missing child page",
            ));
        };
        if *expected != epoch {
            return Err(error(
                InspectorErrorCode::StaleMutation,
                "Child cursor was invalidated by mutation",
            ));
        }
        if *limit == 0 || *limit > MAX_DOM_NODES as u32 {
            return Err(error(
                InspectorErrorCode::InvalidPayload,
                "Invalid DOM page size",
            ));
        }
        if depth >= MAX_DOM_DEPTH {
            return Err(error(
                InspectorErrorCode::BudgetExceeded,
                "DOM expansion depth limit reached",
            ));
        }
        let parent = doc
            .node(key)
            .ok_or_else(|| error(InspectorErrorCode::StaleNode, "Parent was dropped"))?;
        let count = parent.children().len();
        if *offset as usize > count {
            return Err(error(
                InspectorErrorCode::InvalidPayload,
                "Child offset exceeds child count",
            ));
        }
        page.parent = Some(*node);
        page.offset = *offset;
        let mut index = *offset as usize;
        // Reserve envelope space; each node is serialized exactly once for accounting.
        let mut bytes = 1024;
        let mut visited = 0;
        while index < count && visited < *limit as usize {
            budget.check()?;
            let child = parent
                .child_at(index)
                .ok_or_else(|| error(InspectorErrorCode::StaleNode, "Missing child"))?;
            if doc
                .node(child)
                .is_some_and(|n| n.kind() == NodeKind::AnonymousBlock)
            {
                index += 1;
                visited += 1;
                continue;
            }
            let value = describe(doc, child, document, budget)?;
            let size = serde_json::to_vec(&value)
                .map_err(|_| {
                    error(
                        InspectorErrorCode::InvalidPayload,
                        "DOM serialization failed",
                    )
                })?
                .len()
                + 1;
            if bytes + size > MAX_DOM_BYTES {
                break;
            }
            bytes += size;
            page.nodes.push(value);
            index += 1;
            visited += 1;
        }
        page.next_offset = (index < count).then_some(index as u32);
    } else if matches!(
        command,
        InspectorCommand::GetDomRoot | InspectorCommand::DescribeDomNode
    ) {
        page.nodes.push(describe(doc, key, document, budget)?);
    } else {
        return Err(error(
            InspectorErrorCode::InvalidPayload,
            "Unknown DOM command",
        ));
    }
    budget.check()?;
    if !page.is_bounded() {
        return Err(error(
            InspectorErrorCode::BudgetExceeded,
            "DOM response byte limit reached",
        ));
    }
    budget.check()?;
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_truncation_is_utf8_safe_and_byte_bounded() {
        let mut truncated = false;
        let value = preview(&"界".repeat(300), &mut truncated);
        assert!(value.len() <= MAX_DOM_TEXT && truncated);
    }
    #[test]
    fn expired_work_budget_is_refused() {
        assert_eq!(
            Budget(Instant::now() - Duration::from_secs(1))
                .check()
                .unwrap_err()
                .code,
            InspectorErrorCode::Timeout
        );
    }
}
