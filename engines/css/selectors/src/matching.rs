//! Selector matching and specificity against a `DocumentView`, including `:has()`.
use crate::*;
use wega_dom::{DocumentView, NodeKey, NodeKind, NodeView};

impl CompoundSelector {
    pub fn specificity(&self) -> (usize, usize, usize) {
        let mut ids = 0;
        let mut classes = 0;
        let mut types = 0;
        for c in &self.components {
            match c {
                SimpleSelector::Id(_) => ids += 1,
                SimpleSelector::Class(_)
                | SimpleSelector::AttrPresent(_)
                | SimpleSelector::AttrEquals(_, _) => classes += 1,
                SimpleSelector::Type(_) => types += 1,
                SimpleSelector::Is(list) | SimpleSelector::Not(list) => {
                    let s = list
                        .iter()
                        .map(ComplexSelector::specificity)
                        .max()
                        .unwrap_or_default();
                    ids += s.0;
                    classes += s.1;
                    types += s.2;
                }
                SimpleSelector::Has(list) => {
                    let s = list
                        .iter()
                        .map(|(_, s)| s.specificity())
                        .max()
                        .unwrap_or_default();
                    ids += s.0;
                    classes += s.1;
                    types += s.2;
                }
                SimpleSelector::PseudoElement(_) => types += 1,
                SimpleSelector::Universal | SimpleSelector::Where(_) => {}
                _ => classes += 1,
            }
        }
        (ids, classes, types)
    }

    pub fn matches<D: DocumentView>(&self, doc: &D, node_key: NodeKey) -> bool {
        let Some(node) = doc.node(node_key) else {
            return false;
        };
        if node.kind() != NodeKind::Element {
            return false;
        };
        let tag = node
            .element_name()
            .map(|e| e.local.to_ascii_lowercase())
            .unwrap_or_default();

        for comp in &self.components {
            match comp {
                SimpleSelector::Universal => {}
                SimpleSelector::Type(name) => {
                    if !name.eq_ignore_ascii_case(&tag) {
                        return false;
                    }
                }
                SimpleSelector::Class(name) => {
                    let class_attr = node.attr("class").unwrap_or_default();
                    let has_class = class_attr.split_whitespace().any(|c| c == name);
                    if !has_class {
                        return false;
                    }
                }
                SimpleSelector::Id(name) => {
                    if node.attr("id") != Some(name.as_str()) {
                        return false;
                    }
                }
                SimpleSelector::AttrPresent(name) => {
                    if node.attr(name.as_str()).is_none() {
                        return false;
                    }
                }
                SimpleSelector::AttrEquals(name, val) => {
                    if node.attr(name.as_str()) != Some(val.as_str()) {
                        return false;
                    }
                }
                SimpleSelector::AttrMatch(name, value, op, insensitive) => {
                    let Some(attr) = node.attr(name) else {
                        return false;
                    };
                    let (attr, value) = if *insensitive {
                        (attr.to_ascii_lowercase(), value.to_ascii_lowercase())
                    } else {
                        (attr.to_owned(), value.clone())
                    };
                    let matched = match op {
                        '=' => attr == value,
                        '~' => {
                            !value.is_empty() && attr.split_ascii_whitespace().any(|v| v == value)
                        }
                        '|' => attr == value || attr.starts_with(&format!("{value}-")),
                        '^' => !value.is_empty() && attr.starts_with(&value),
                        '$' => !value.is_empty() && attr.ends_with(&value),
                        '*' => !value.is_empty() && attr.contains(&value),
                        _ => false,
                    };
                    if !matched {
                        return false;
                    }
                }
                SimpleSelector::Is(list) | SimpleSelector::Where(list) => {
                    if !list.iter().any(|s| s.matches(doc, node_key)) {
                        return false;
                    }
                }
                SimpleSelector::Not(list) => {
                    if list.iter().any(|s| s.matches(doc, node_key)) {
                        return false;
                    }
                }
                SimpleSelector::Never | SimpleSelector::PseudoElement(_) => return false,
                SimpleSelector::Has(list) => {
                    if !list
                        .iter()
                        .any(|(combinator, s)| has_relative(doc, node_key, *combinator, s))
                    {
                        return false;
                    }
                }
                SimpleSelector::Lang(range) => {
                    let mut current = Some(node_key);
                    let mut lang = None;
                    while let Some(k) = current {
                        let Some(n) = doc.node(k) else { break };
                        if let Some(l) = n.attr("lang") {
                            lang = Some(l.to_ascii_lowercase());
                            break;
                        }
                        current = n.parent();
                    }
                    if !lang.is_some_and(|l| l == *range || l.starts_with(&format!("{range}-"))) {
                        return false;
                    }
                }
                SimpleSelector::Optional | SimpleSelector::ReadOnly(_) => {
                    let form = matches!(tag.as_str(), "input" | "select" | "textarea");
                    let matched = match comp {
                        SimpleSelector::Optional => form && node.attr("required").is_none(),
                        SimpleSelector::ReadOnly(read_only) => {
                            let editable = matches!(tag.as_str(), "input" | "textarea")
                                && node.attr("readonly").is_none()
                                && node.attr("disabled").is_none()
                                || node
                                    .attr("contenteditable")
                                    .is_some_and(|v| !v.eq_ignore_ascii_case("false"));
                            editable != *read_only
                        }
                        _ => unreachable!(),
                    };
                    if !matched {
                        return false;
                    }
                }
                SimpleSelector::PlaceholderShown => {
                    if !matches!(tag.as_str(), "input" | "textarea")
                        || node.attr("placeholder").is_none()
                        || node.attr("value").is_some_and(|v| !v.is_empty())
                    {
                        return false;
                    }
                }
                SimpleSelector::Hover
                | SimpleSelector::Focus
                | SimpleSelector::FocusWithin
                | SimpleSelector::Active
                | SimpleSelector::Checked
                | SimpleSelector::Disabled
                | SimpleSelector::Enabled => {
                    let state = doc.element_state(node_key);
                    let matches = match comp {
                        SimpleSelector::Hover => state.hovered,
                        SimpleSelector::Focus => state.focused,
                        SimpleSelector::FocusWithin => state.focus_within,
                        SimpleSelector::Active => state.active,
                        SimpleSelector::Checked => state.checked,
                        SimpleSelector::Disabled => state.disabled,
                        SimpleSelector::Enabled => state.enabled,
                        _ => unreachable!(),
                    };
                    if !matches {
                        return false;
                    }
                }
                SimpleSelector::Link => {
                    if !matches!(tag.as_str(), "a" | "area") || node.attr("href").is_none() {
                        return false;
                    }
                }
                SimpleSelector::Root => {
                    if node
                        .parent()
                        .and_then(|p| doc.node(p))
                        .is_none_or(|p| p.kind() != NodeKind::Document)
                    {
                        return false;
                    }
                }
                SimpleSelector::Empty => {
                    if node.children().filter_map(|c| doc.node(c)).any(|c| {
                        c.kind() == NodeKind::Element || c.text().is_some_and(|t| !t.is_empty())
                    }) {
                        return false;
                    }
                }
                structural => {
                    let Some(parent) = node.parent().and_then(|p| doc.node(p)) else {
                        return false;
                    };
                    let of_type = matches!(structural, SimpleSelector::NthOfType(..));
                    let siblings: Vec<_> = parent
                        .children()
                        .filter(|k| {
                            doc.node(*k).is_some_and(|n| {
                                n.kind() == NodeKind::Element
                                    && (!of_type
                                        || n.element_name()
                                            .is_some_and(|e| e.local.eq_ignore_ascii_case(&tag)))
                            })
                        })
                        .collect();
                    let Some(index) = siblings.iter().position(|k| *k == node_key) else {
                        return false;
                    };
                    let index = index as i64 + 1;
                    let count = siblings.len() as i64;
                    let matches = match structural {
                        SimpleSelector::FirstChild => index == 1,
                        SimpleSelector::LastChild => index == count,
                        SimpleSelector::OnlyChild => count == 1,
                        SimpleSelector::NthChild(a, b, reverse)
                        | SimpleSelector::NthOfType(a, b, reverse) => {
                            let n = if *reverse { count - index + 1 } else { index } - *b as i64;
                            if *a == 0 {
                                n == 0
                            } else {
                                n % *a as i64 == 0 && n / *a as i64 >= 0
                            }
                        }
                        _ => false,
                    };
                    if !matches {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// Bounded `:has()` evaluation; large subtrees are cut off rather than
/// making every restyle quadratic.
pub(crate) fn has_relative<D: DocumentView>(
    doc: &D,
    subject: NodeKey,
    combinator: Combinator,
    selector: &ComplexSelector,
) -> bool {
    const LIMIT: usize = 4096;
    let Some(node) = doc.node(subject) else {
        return false;
    };
    match combinator {
        Combinator::Child => node.children().any(|c| selector.matches(doc, c)),
        Combinator::Descendant => {
            let mut stack: Vec<NodeKey> = node.children().collect();
            let mut seen = 0;
            while let Some(k) = stack.pop() {
                seen += 1;
                if seen > LIMIT {
                    return false;
                }
                if selector.matches(doc, k) {
                    return true;
                }
                if let Some(n) = doc.node(k) {
                    stack.extend(n.children());
                }
            }
            false
        }
        Combinator::Adjacent | Combinator::Sibling => {
            let Some(parent) = node.parent().and_then(|p| doc.node(p)) else {
                return false;
            };
            let following = parent
                .children()
                .skip_while(|k| *k != subject)
                .skip(1)
                .filter(|k| doc.node(*k).is_some_and(|n| n.kind() == NodeKind::Element));
            if combinator == Combinator::Adjacent {
                following.take(1).any(|k| selector.matches(doc, k))
            } else {
                following.into_iter().any(|k| selector.matches(doc, k))
            }
        }
    }
}

impl ComplexSelector {
    pub fn specificity(&self) -> (usize, usize, usize) {
        let mut total = (0, 0, 0);
        for (compound, _) in &self.items {
            let s = compound.specificity();
            total.0 += s.0;
            total.1 += s.1;
            total.2 += s.2;
        }
        total
    }

    pub fn matches<D: DocumentView>(&self, doc: &D, node_key: NodeKey) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let (last_compound, _) = self.items.last().unwrap();
        if !last_compound.matches(doc, node_key) {
            return false;
        }
        if self.items.len() == 1 {
            return true;
        }

        self.matches_at(doc, node_key, self.items.len() - 1)
    }
    fn matches_at<D: DocumentView>(&self, doc: &D, key: NodeKey, index: usize) -> bool {
        if !self.items[index].0.matches(doc, key) {
            return false;
        }
        if index == 0 {
            return true;
        }
        let combinator = self.items[index - 1].1.unwrap_or(Combinator::Descendant);
        let parent = || doc.node(key).and_then(NodeView::parent);
        match combinator {
            Combinator::Child => parent().is_some_and(|p| self.matches_at(doc, p, index - 1)),
            Combinator::Descendant => {
                let mut ancestor = parent();
                while let Some(p) = ancestor {
                    if self.matches_at(doc, p, index - 1) {
                        return true;
                    }
                    ancestor = doc.node(p).and_then(NodeView::parent);
                }
                false
            }
            Combinator::Adjacent | Combinator::Sibling => {
                let Some(parent) = parent().and_then(|p| doc.node(p)) else {
                    return false;
                };
                let previous: Vec<_> = parent
                    .children()
                    .take_while(|k| *k != key)
                    .filter(|k| doc.node(*k).is_some_and(|n| n.kind() == NodeKind::Element))
                    .collect();
                if combinator == Combinator::Adjacent {
                    previous
                        .last()
                        .is_some_and(|p| self.matches_at(doc, *p, index - 1))
                } else {
                    previous
                        .into_iter()
                        .rev()
                        .any(|p| self.matches_at(doc, p, index - 1))
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CSS PARSING (RULES & SELECTORS)
// ---------------------------------------------------------------------------
