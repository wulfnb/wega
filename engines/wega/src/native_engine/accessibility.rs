//! AccessKit representation from the authoritative DOM and native geometry.
//! This never requests reference style/layout and never executes page callbacks.
use super::backend::NativeDocument;
use accesskit::{Action, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use std::collections::HashMap;
use wega_dom::{DocumentView, NodeKey, NodeKind, NodeView};

fn text<D: DocumentView>(doc: &D, key: NodeKey) -> String {
    let mut out = String::new();
    let mut stack = vec![key];
    let mut budget = 4096;
    while let Some(k) = stack.pop() {
        if budget == 0 || out.len() >= 4096 {
            break;
        }
        budget -= 1;
        if let Some(n) = doc.node(k) {
            if let Some(t) = n.text() {
                out.extend(t.chars().take(4096 - out.len()));
            }
            stack.extend(n.children().rev());
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn role(tag: &str, kind: &str, aria: Option<&str>) -> Role {
    if let Some(role) = aria
        .and_then(|a| a.split_whitespace().next())
        .and_then(|a| match a {
            "button" => Some(Role::Button),
            "link" => Some(Role::Link),
            "checkbox" => Some(Role::CheckBox),
            "radio" => Some(Role::RadioButton),
            "heading" => Some(Role::Heading),
            "img" => Some(Role::Image),
            "textbox" => Some(Role::TextInput),
            "list" => Some(Role::List),
            "listitem" => Some(Role::ListItem),
            "navigation" => Some(Role::Navigation),
            "main" => Some(Role::Main),
            "dialog" => Some(Role::Dialog),
            "alert" => Some(Role::Alert),
            "alertdialog" => Some(Role::AlertDialog),
            "banner" => Some(Role::Banner),
            "contentinfo" => Some(Role::ContentInfo),
            "complementary" => Some(Role::Complementary),
            "region" => Some(Role::Region),
            "article" => Some(Role::Article),
            "search" => Some(Role::Search),
            "tab" => Some(Role::Tab),
            "tablist" => Some(Role::TabList),
            "tabpanel" => Some(Role::TabPanel),
            "menu" => Some(Role::Menu),
            "menubar" => Some(Role::MenuBar),
            "menuitem" => Some(Role::MenuItem),
            "menuitemcheckbox" => Some(Role::MenuItemCheckBox),
            "menuitemradio" => Some(Role::MenuItemRadio),
            "progressbar" => Some(Role::ProgressIndicator),
            "status" => Some(Role::Status),
            "slider" => Some(Role::Slider),
            "spinbutton" => Some(Role::SpinButton),
            "switch" => Some(Role::Switch),
            "tree" => Some(Role::Tree),
            "treeitem" => Some(Role::TreeItem),
            "treegrid" => Some(Role::TreeGrid),
            "grid" => Some(Role::Grid),
            "gridcell" => Some(Role::GridCell),
            "toolbar" => Some(Role::Toolbar),
            "tooltip" => Some(Role::Tooltip),
            "form" => Some(Role::Form),
            "timer" => Some(Role::Timer),
            "log" => Some(Role::Log),
            "none" | "presentation" => Some(Role::GenericContainer),
            _ => None,
        })
    {
        return role;
    }
    match tag {
        "input" => match kind {
            "checkbox" => Role::CheckBox,
            "radio" => Role::RadioButton,
            "button" | "submit" | "reset" => Role::Button,
            "password" => Role::PasswordInput,
            "email" => Role::EmailInput,
            "search" => Role::SearchInput,
            "number" => Role::NumberInput,
            "range" => Role::Slider,
            "date" => Role::DateInput,
            "time" => Role::TimeInput,
            "url" => Role::UrlInput,
            "tel" => Role::PhoneNumberInput,
            _ => Role::TextInput,
        },
        "textarea" => Role::MultilineTextInput,
        "select" => Role::ComboBox,
        "option" => Role::ListBoxOption,
        "button" => Role::Button,
        "a" => Role::Link,
        "img" | "svg" => Role::Image,
        "canvas" => Role::Canvas,
        "iframe" => Role::Iframe,
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Role::Heading,
        "p" => Role::Paragraph,
        "label" => Role::Label,
        "ul" | "ol" => Role::List,
        "li" => Role::ListItem,
        "table" => Role::Table,
        "tr" => Role::Row,
        "td" => Role::Cell,
        "th" => Role::ColumnHeader,
        "nav" => Role::Navigation,
        "main" => Role::Main,
        "form" => Role::Form,
        "dialog" => Role::Dialog,
        "header" => Role::Header,
        "footer" => Role::Footer,
        "aside" => Role::Complementary,
        "section" => Role::Region,
        "article" => Role::Article,
        "progress" => Role::ProgressIndicator,
        "meter" => Role::Meter,
        "details" => Role::Details,
        "summary" => Role::Button,
        "figure" => Role::Figure,
        "figcaption" => Role::FigureCaption,
        "blockquote" => Role::Blockquote,
        "code" => Role::Code,
        _ => Role::GenericContainer,
    }
}

pub fn build<D: wega_layout::resources::NativeResources>(
    native: &NativeDocument<D>,
    doc: &D,
    focused: Option<NodeKey>,
) -> TreeUpdate {
    let root = doc.root();
    let mut labels: HashMap<String, String> = HashMap::new();
    let mut stack = vec![root];
    while let Some(k) = stack.pop() {
        if let Some(n) = doc.node(k) {
            if n.element_name().is_some_and(|n| n.local == "label") {
                if let Some(id) = n.attr("for") {
                    labels.entry(id.into()).or_default().push_str(&text(doc, k));
                }
            }
            stack.extend(n.children().rev());
        }
    }
    let mut styles = HashMap::new();
    let mut checked = HashMap::new();
    let mut stack = native.layout_tree.iter().collect::<Vec<_>>();
    while let Some(b) = stack.pop() {
        if let Some(k) = b.node {
            styles.entry(k).or_insert(&b.style);
            if let Some(f) = &b.form_state {
                checked.insert(k, f.checked);
            }
        }
        stack.extend(&b.children);
    }
    let mut nodes = vec![(NodeId(root.as_u64()), Node::new(Role::Document))];
    let mut indices = HashMap::from([(root, 0usize)]);
    let mut stack = doc
        .node(root)
        .map(|n| n.children().rev().map(|k| (k, 0usize)).collect::<Vec<_>>())
        .unwrap_or_default();
    while let Some((key, parent)) = stack.pop() {
        let Some(n) = doc.node(key) else {
            continue;
        };
        if n.attr("aria-hidden") == Some("true")
            || n.attr("hidden").is_some()
            || n.attr("inert").is_some()
        {
            continue;
        }
        let Some(style) = styles.get(&key) else {
            continue;
        };
        if !style.visible {
            stack.extend(n.children().rev().map(|k| (k, parent)));
            continue;
        }
        let tag = n.element_name().map_or("", |n| n.local);
        let kind = n.attr("type").unwrap_or("text").to_ascii_lowercase();
        if tag == "input" && kind == "hidden" {
            continue;
        }
        let r = if n.kind() == NodeKind::Text {
            Role::TextRun
        } else {
            role(tag, &kind, n.attr("role"))
        };
        let mut node = Node::new(r);
        if n.kind() == NodeKind::Text {
            node.set_value(n.text().unwrap_or(""));
        } else {
            node.set_html_tag(tag);
            let referenced = n.attr("aria-labelledby").map(|ids| {
                ids.split_whitespace()
                    .take(32)
                    .filter_map(|id| doc.element_by_id(id))
                    .map(|k| text(doc, k))
                    .collect::<Vec<_>>()
                    .join(" ")
            });
            let mut label = referenced
                .filter(|s| !s.is_empty())
                .or_else(|| n.attr("aria-label").map(str::to_owned))
                .or_else(|| n.attr("id").and_then(|id| labels.get(id)).cloned());
            if label.is_none() {
                let mut at = n.parent();
                while let Some(k) = at {
                    let Some(p) = doc.node(k) else {
                        break;
                    };
                    if p.element_name().is_some_and(|p| p.local == "label") {
                        label = Some(text(doc, k));
                        break;
                    }
                    at = p.parent();
                }
            }
            if label.is_none() {
                label = match tag {
                    "img" => n.attr("alt").map(str::to_owned),
                    "button" | "a" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "label"
                    | "option" => Some(text(doc, key)),
                    "input" if matches!(kind.as_str(), "button" | "submit" | "reset") => {
                        n.attr("value").map(str::to_owned)
                    }
                    _ => n.attr("title").map(str::to_owned),
                };
            }
            if let Some(label) = label {
                node.set_label(label);
            }

            // Description: aria-describedby or aria-description
            let described = n.attr("aria-describedby").map(|ids| {
                ids.split_whitespace()
                    .take(32)
                    .filter_map(|id| doc.element_by_id(id))
                    .map(|k| text(doc, k))
                    .collect::<Vec<_>>()
                    .join(" ")
            });
            let description = described
                .filter(|s| !s.is_empty())
                .or_else(|| n.attr("aria-description").map(str::to_owned));
            if let Some(desc) = description {
                node.set_description(desc);
            }

            // Heading level
            if r == Role::Heading {
                let heading_level =
                    if let Some(lvl) = n.attr("aria-level").and_then(|v| v.parse::<usize>().ok()) {
                        Some(lvl.saturating_sub(1))
                    } else {
                        match tag {
                            "h1" => Some(0),
                            "h2" => Some(1),
                            "h3" => Some(2),
                            "h4" => Some(3),
                            "h5" => Some(4),
                            "h6" => Some(5),
                            _ => None,
                        }
                    };
                if let Some(lvl) = heading_level {
                    node.set_level(lvl);
                }
            }

            // Disabled
            if n.attr("disabled").is_some() || n.attr("aria-disabled") == Some("true") {
                node.set_disabled();
            }

            // Required
            if n.attr("required").is_some() || n.attr("aria-required") == Some("true") {
                node.set_required();
            }

            // ReadOnly
            if n.attr("readonly").is_some() || n.attr("aria-readonly") == Some("true") {
                node.set_read_only();
            }

            // Expanded
            match n.attr("aria-expanded") {
                Some("true") => node.set_expanded(true),
                Some("false") => node.set_expanded(false),
                _ => {}
            }

            // Selected
            match n.attr("aria-selected") {
                Some("true") => node.set_selected(true),
                Some("false") => node.set_selected(false),
                _ => {}
            }

            // Modal
            if n.attr("aria-modal") == Some("true") {
                node.set_modal();
            }

            // Multiselectable
            if n.attr("aria-multiselectable") == Some("true") {
                node.set_multiselectable();
            }

            // Invalid
            if let Some(inv) = n.attr("aria-invalid") {
                match inv {
                    "true" => node.set_invalid(accesskit::Invalid::True),
                    "spelling" => node.set_invalid(accesskit::Invalid::Spelling),
                    "grammar" => node.set_invalid(accesskit::Invalid::Grammar),
                    _ => {}
                }
            }

            // Live region
            if let Some(live) = n.attr("aria-live") {
                match live {
                    "polite" => node.set_live(accesskit::Live::Polite),
                    "assertive" => node.set_live(accesskit::Live::Assertive),
                    "off" => node.set_live(accesskit::Live::Off),
                    _ => {}
                }
            }

            // Numeric values for range / progress / meter / slider
            if let Some(val) = n
                .attr("aria-valuenow")
                .and_then(|v| v.parse::<f64>().ok())
                .or_else(|| n.attr("value").and_then(|v| v.parse::<f64>().ok()))
            {
                node.set_numeric_value(val);
            }
            if let Some(min) = n
                .attr("aria-valuemin")
                .and_then(|v| v.parse::<f64>().ok())
                .or_else(|| n.attr("min").and_then(|v| v.parse::<f64>().ok()))
            {
                node.set_min_numeric_value(min);
            }
            if let Some(max) = n
                .attr("aria-valuemax")
                .and_then(|v| v.parse::<f64>().ok())
                .or_else(|| n.attr("max").and_then(|v| v.parse::<f64>().ok()))
            {
                node.set_max_numeric_value(max);
            }

            if matches!(tag, "input" | "textarea" | "select") {
                let value = if tag == "select" {
                    let mut options = Vec::new();
                    let mut stack = n.children().rev().collect::<Vec<_>>();
                    while let Some(k) = stack.pop() {
                        if let Some(option) = doc.node(k) {
                            if option.element_name().is_some_and(|n| n.local == "option") {
                                options.push(k);
                            } else {
                                stack.extend(option.children().rev());
                            }
                        }
                    }
                    options
                        .iter()
                        .copied()
                        .find(|k| doc.element_state(*k).checked)
                        .or_else(|| options.first().copied())
                        .and_then(|k| {
                            doc.node(k).map(|n| {
                                n.attr("value")
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| text(doc, k))
                            })
                        })
                        .unwrap_or_default()
                } else {
                    n.attr("value").map(str::to_owned).unwrap_or_else(|| {
                        if tag == "textarea" {
                            text(doc, key)
                        } else {
                            String::new()
                        }
                    })
                };
                node.set_value(if kind == "password" {
                    "•".repeat(value.chars().count().min(4096))
                } else {
                    value
                });
            }
            if matches!(r, Role::CheckBox | Role::RadioButton | Role::Switch) {
                node.set_toggled(accesskit::Toggled::from(
                    checked
                        .get(&key)
                        .copied()
                        .unwrap_or_else(|| n.attr("checked").is_some())
                        || n.attr("aria-checked") == Some("true"),
                ));
            }
            if matches!(
                r,
                Role::Button
                    | Role::Link
                    | Role::CheckBox
                    | Role::RadioButton
                    | Role::Switch
                    | Role::MenuItem
                    | Role::MenuItemCheckBox
                    | Role::MenuItemRadio
                    | Role::Tab
            ) {
                node.add_action(Action::Click);
            }
            if matches!(tag, "input" | "textarea" | "select" | "button" | "a")
                || n.attr("tabindex").is_some()
            {
                node.add_action(Action::Focus);
            }
        }
        if let Ok(rect) = native.bounding_rect(key) {
            node.set_bounds(accesskit::Rect::new(
                rect.x,
                rect.y,
                rect.right(),
                rect.bottom(),
            ));
        }
        let index = nodes.len();
        let id = NodeId(key.as_u64());
        nodes[parent].1.push_child(id);
        nodes.push((id, node));
        indices.insert(key, index);
        stack.extend(n.children().rev().map(|k| (k, index)));
    }
    TreeUpdate {
        tree_id: TreeId::ROOT,
        tree: Some(TreeInfo::new(NodeId(root.as_u64()))),
        focus: NodeId(
            focused
                .filter(|k| indices.contains_key(k))
                .unwrap_or(root)
                .as_u64(),
        ),
        nodes,
    }
}
