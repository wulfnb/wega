//! Validation of an authorized edit against the authoritative document, producing the exact host
//! operation page script would use. Nothing here mutates: the caller applies the plan through
//! `Dom::call`, so budgets, invalidation and custom-element queues are the script path's own.
use serde_json::{Value, json};
use wega_dom::{DocumentView, NodeKey, NodeKind, NodeView};
use wega_protocol::{
    DocumentContext,
    inspector::{
        InspectorError, InspectorErrorCode,
        dom::{InspectorNodeId, MAX_DOM_DEPTH},
        edit::{MutationOp, is_code_attribute},
    },
};

fn error(code: InspectorErrorCode, message: &str) -> InspectorError {
    InspectorError::new(code, message)
}

/// How far up a connectivity check walks: the layout depth limit plus slack, so a hostile deep
/// tree cannot make an edit expensive.
const MAX_CONNECT_STEPS: usize = 4 * MAX_DOM_DEPTH;

/// A validated edit: the host-op arguments and what the caller may report.
#[derive(Debug, PartialEq)]
pub struct Plan {
    pub args: Vec<Value>,
    /// Only the CSS host op can report "valid but changed nothing" through its result.
    pub reports_change: bool,
}

fn connected<D: DocumentView>(doc: &D, key: NodeKey) -> Result<(), InspectorError> {
    let mut at = key;
    for _ in 0..=MAX_CONNECT_STEPS {
        let node = doc
            .node(at)
            .filter(|n| n.kind() != NodeKind::AnonymousBlock)
            .ok_or_else(|| error(InspectorErrorCode::StaleNode, "Node no longer inspectable"))?;
        if at == doc.root() {
            return Ok(());
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

/// A URL-valued attribute carrying a `javascript:` URL runs code when activated.
fn runs_code_when_used(name: &str, value: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "href" | "src" | "action" | "formaction" | "data" | "xlink:href"
    ) && value
        .trim_start_matches(|c: char| c.is_whitespace() || c.is_control())
        .get(..11)
        .is_some_and(|head| head.eq_ignore_ascii_case("javascript:"))
}

pub fn plan<D: DocumentView>(
    doc: &D,
    document: DocumentContext,
    node: InspectorNodeId,
    op: &MutationOp,
    allow_code: bool,
) -> Result<Plan, InspectorError> {
    if node.document != document {
        return Err(error(
            InspectorErrorCode::StaleDocument,
            "Node belongs to a replaced document",
        ));
    }
    op.validate()
        .map_err(|message| error(InspectorErrorCode::InvalidPayload, message))?;
    let key = NodeKey::from_u64(node.key);
    connected(doc, key)?;
    let view = doc
        .node(key)
        .ok_or_else(|| error(InspectorErrorCode::StaleNode, "Node was dropped"))?;
    let id = node.key.to_string();
    let element_only = |what: &str| -> Result<(), InspectorError> {
        if view.kind() == NodeKind::Element {
            Ok(())
        } else {
            Err(error(
                InspectorErrorCode::InvalidPayload,
                &format!("{what} needs an element"),
            ))
        }
    };
    let need_code = || -> Result<(), InspectorError> {
        if allow_code {
            Ok(())
        } else {
            Err(error(
                InspectorErrorCode::PolicyDenied,
                "This edit can run code and also needs the evaluation grant",
            ))
        }
    };
    match op {
        MutationOp::SetAttribute { name, value } => {
            element_only("Setting an attribute")?;
            if is_code_attribute(name) || runs_code_when_used(name, value) {
                need_code()?;
            }
            Ok(Plan {
                args: vec![json!("setAttr"), json!(id), json!(name), json!(value)],
                reports_change: false,
            })
        }
        MutationOp::RemoveAttribute { name } => {
            element_only("Removing an attribute")?;
            Ok(Plan {
                args: vec![json!("removeAttr"), json!(id), json!(name)],
                reports_change: false,
            })
        }
        MutationOp::SetText { text } => {
            if !matches!(view.kind(), NodeKind::Text | NodeKind::Comment) {
                return Err(error(
                    InspectorErrorCode::InvalidPayload,
                    "Only text and comment nodes can be edited",
                ));
            }
            // Text of a <script> is code.
            let in_script = view
                .parent()
                .and_then(|p| doc.node(p))
                .and_then(NodeView::element_name)
                .is_some_and(|name| name.local == "script");
            if in_script {
                need_code()?;
            }
            Ok(Plan {
                args: vec![json!("setText"), json!(id), json!(text)],
                reports_change: false,
            })
        }
        MutationOp::RemoveNode => {
            if key == doc.root() {
                return Err(error(
                    InspectorErrorCode::InvalidPayload,
                    "The document cannot be removed",
                ));
            }
            if view
                .element_name()
                .is_some_and(|name| matches!(name.local, "html" | "head" | "body"))
            {
                return Err(error(
                    InspectorErrorCode::InvalidPayload,
                    "html, head and body cannot be removed",
                ));
            }
            Ok(Plan {
                args: vec![json!("remove"), json!(id)],
                reports_change: false,
            })
        }
        MutationOp::SetInlineProperty {
            property,
            value,
            important,
        } => {
            element_only("Setting a style")?;
            Ok(Plan {
                args: vec![
                    json!(if *important {
                        "styleSetImportant"
                    } else {
                        "styleSet"
                    }),
                    json!(id),
                    json!(property),
                    json!(value),
                ],
                reports_change: true,
            })
        }
    }
}

/// Map a host-op failure to an inspector error code.
pub fn map_failure(message: &str) -> InspectorError {
    let lower = message.to_ascii_lowercase();
    let code = if lower.contains("budget") || lower.contains("limit") {
        InspectorErrorCode::BudgetExceeded
    } else if lower.contains("no longer exists") {
        InspectorErrorCode::StaleNode
    } else if lower.contains("busy") {
        InspectorErrorCode::Busy
    } else {
        InspectorErrorCode::InvalidPayload
    };
    InspectorError::new(code, &message.chars().take(200).collect::<String>())
}
