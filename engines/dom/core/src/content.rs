//! Tree-only host operations shared by the runtime and owned documents.
use crate::{DocumentView, NodeKey, NodeKind, NodeView};

/// DOM `textContent`: documents have no value, character data returns its own
/// data, and elements/fragments concatenate descendant text in tree order.
pub fn text_content<D: DocumentView>(doc: &D, key: NodeKey) -> Option<String> {
    let node = doc.node(key)?;
    match node.kind() {
        NodeKind::Document if !node.is_fragment() => return None,
        NodeKind::Text | NodeKind::Comment => return Some(node.text().unwrap_or("").into()),
        _ => {}
    }
    let mut out = String::new();
    let mut stack: Vec<_> = node.children().rev().collect();
    while let Some(key) = stack.pop() {
        let Some(node) = doc.node(key) else { continue };
        if node.kind() == NodeKind::Text {
            out.push_str(node.text().unwrap_or(""));
        } else {
            stack.extend(node.children().rev());
        }
    }
    Some(out)
}

/// Form association from the explicit `form` id, otherwise the closest
/// ancestor form. An invalid explicit association does not fall back.
pub fn form_owner<D: DocumentView>(doc: &D, key: NodeKey) -> Option<NodeKey> {
    let node = doc.node(key)?;
    node.element_name()?;
    let is_form = |key| {
        doc.node(key)
            .and_then(NodeView::element_name)
            .is_some_and(|n| n.local == "form")
    };
    if let Some(id) = node.attr("form") {
        return doc.element_by_id(id).filter(|key| is_form(*key));
    }
    let mut parent = node.parent();
    while let Some(key) = parent {
        if is_form(key) {
            return Some(key);
        }
        parent = doc.node(key)?.parent();
    }
    None
}

/// Serialize host-visible markup without recursion or a concrete node type.
/// This is the runtime's existing fragment serialization, with document/fragment
/// containers omitted, raw script/style text, HTML voids and qualified names.
pub fn serialize<D: DocumentView>(doc: &D, key: NodeKey, out: &mut String) {
    enum Work {
        Node(NodeKey),
        Close(String),
    }
    let mut stack = vec![Work::Node(key)];
    while let Some(work) = stack.pop() {
        let key = match work {
            Work::Close(name) => {
                out.push_str("</");
                out.push_str(&name);
                out.push('>');
                continue;
            }
            Work::Node(key) => key,
        };
        let Some(node) = doc.node(key) else { continue };
        match node.kind() {
            NodeKind::Text => {
                let text = node.text().unwrap_or("");
                let raw = node
                    .parent()
                    .and_then(|key| doc.node(key))
                    .and_then(NodeView::element_name)
                    .is_some_and(|name| matches!(name.local, "script" | "style"));
                if raw {
                    out.push_str(text);
                } else {
                    out.push_str(
                        &text
                            .replace('&', "&amp;")
                            .replace('<', "&lt;")
                            .replace('>', "&gt;"),
                    );
                }
            }
            NodeKind::Comment => {
                out.push_str("<!--");
                out.push_str(node.text().unwrap_or(""));
                out.push_str("-->");
            }
            NodeKind::Document => {
                stack.extend(node.children().rev().map(Work::Node));
            }
            _ => {
                let Some(name) = node.element_name() else {
                    continue;
                };
                if name.local == "#document-fragment" {
                    stack.extend(node.children().rev().map(Work::Node));
                    continue;
                }
                let qualified = name
                    .prefix
                    .map_or_else(|| name.local.to_owned(), |p| format!("{p}:{}", name.local));
                out.push('<');
                out.push_str(&qualified);
                for (name, value) in node.attributes() {
                    out.push(' ');
                    if let Some(prefix) = &name.prefix {
                        out.push_str(prefix);
                        out.push(':');
                    }
                    out.push_str(&name.local);
                    out.push_str("=\"");
                    out.push_str(&value.replace('&', "&amp;").replace('"', "&quot;"));
                    out.push('"');
                }
                out.push('>');
                let void = name.namespace == crate::HTML_NAMESPACE
                    && matches!(
                        name.local,
                        "area"
                            | "base"
                            | "br"
                            | "col"
                            | "embed"
                            | "hr"
                            | "img"
                            | "input"
                            | "link"
                            | "meta"
                            | "param"
                            | "source"
                            | "track"
                            | "wbr"
                    );
                if !void {
                    stack.push(Work::Close(qualified));
                    if let Some(contents) = doc.owned_fragment(key) {
                        stack.push(Work::Node(contents));
                    } else {
                        stack.extend(node.children().rev().map(Work::Node));
                    }
                }
            }
        }
    }
}
