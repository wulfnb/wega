//! Selector matching and specificity against a `DocumentView`, including `:has()`.
use crate::*;
use wega_dom::{DocumentView, HTML_NAMESPACE, NodeKey, NodeKind, NodeView};

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
                SimpleSelector::Universal
                | SimpleSelector::Where(_)
                | SimpleSelector::Namespace(_) => {}
                _ => classes += 1,
            }
        }
        (ids, classes, types)
    }

    pub fn matches<D: DocumentView>(&self, doc: &D, node_key: NodeKey) -> bool {
        self.matches_scoped(doc, node_key, None)
    }

    /// Match with `:scope` as `scope` (a query's scoping element), or the root element.
    pub fn matches_scoped<D: DocumentView>(
        &self,
        doc: &D,
        node_key: NodeKey,
        scope: Option<NodeKey>,
    ) -> bool {
        let Some(node) = doc.node(node_key) else {
            return false;
        };
        if node.kind() != NodeKind::Element {
            return false;
        };
        let (raw_tag, html) = node
            .element_name()
            .map(|e| (e.local, e.namespace == HTML_NAMESPACE))
            .unwrap_or_default();
        let tag = raw_tag.to_ascii_lowercase();
        // Attribute names match case-insensitively on HTML elements (Selectors 4 §6.1).
        let attr = |name: &str| -> Option<&str> {
            node.attributes()
                .find(|(n, _)| {
                    if html {
                        (*n.local).eq_ignore_ascii_case(name)
                    } else {
                        &*n.local == name
                    }
                })
                .map(|(_, v)| v)
        };

        for comp in &self.components {
            match comp {
                SimpleSelector::Universal => {}
                SimpleSelector::Type(name) => {
                    let matched = if html {
                        name.eq_ignore_ascii_case(raw_tag)
                    } else {
                        name == raw_tag
                    };
                    if !matched {
                        return false;
                    }
                }
                SimpleSelector::Namespace(url) => {
                    if node.element_name().is_none_or(|e| e.namespace != url) {
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
                    if attr(name).is_none() {
                        return false;
                    }
                }
                SimpleSelector::AttrEquals(name, val) => {
                    if attr(name) != Some(val.as_str()) {
                        return false;
                    }
                }
                SimpleSelector::AttrMatch(name, value, op, flag) => {
                    let Some(attr) = attr(name) else {
                        return false;
                    };
                    let insensitive =
                        flag.unwrap_or_else(|| html && case_insensitive_attribute(name));
                    let (attr, value) = if insensitive {
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
                    if !list.iter().any(|s| s.matches_scoped(doc, node_key, scope)) {
                        return false;
                    }
                }
                SimpleSelector::Not(list) => {
                    if list.iter().any(|s| s.matches_scoped(doc, node_key, scope)) {
                        return false;
                    }
                }
                SimpleSelector::Scope => {
                    let is_scope = match scope {
                        Some(scope) => scope == node_key,
                        None => node
                            .parent()
                            .and_then(|p| doc.node(p))
                            .is_some_and(|p| p.kind() == NodeKind::Document),
                    };
                    if !is_scope {
                        return false;
                    }
                }
                SimpleSelector::Never | SimpleSelector::PseudoElement(_) => return false,
                SimpleSelector::Has(list) => {
                    if !list
                        .iter()
                        .any(|(combinator, s)| has_relative(doc, node_key, *combinator, s, scope))
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
                    // Comments and processing instructions do not count (Selectors 4 §14.2).
                    if node.children().filter_map(|c| doc.node(c)).any(|c| {
                        c.kind() == NodeKind::Element
                            || (c.kind() == NodeKind::Text
                                && c.text().is_some_and(|t| !t.is_empty()))
                    }) {
                        return false;
                    }
                }
                structural => {
                    // An element without a parent is its own only sibling (Selectors 4 §14.3).
                    let Some(parent) = node.parent().and_then(|p| doc.node(p)) else {
                        let only = match structural {
                            SimpleSelector::NthChild(a, b, _)
                            | SimpleSelector::NthChildOf(a, b, _, _)
                            | SimpleSelector::NthOfType(a, b, _) => {
                                let n = 1 - *b as i64;
                                if *a == 0 {
                                    n == 0
                                } else {
                                    n % *a as i64 == 0 && n / *a as i64 >= 0
                                }
                            }
                            _ => true,
                        };
                        let of = match structural {
                            SimpleSelector::NthChildOf(_, _, _, list) => {
                                list.iter().any(|s| s.matches_scoped(doc, node_key, scope))
                            }
                            _ => true,
                        };
                        if !(only && of) {
                            return false;
                        }
                        continue;
                    };
                    let of_type = matches!(structural, SimpleSelector::NthOfType(..));
                    let of = match structural {
                        SimpleSelector::NthChildOf(_, _, _, list) => Some(list),
                        _ => None,
                    };
                    if of.is_some_and(|list| {
                        !list.iter().any(|s| s.matches_scoped(doc, node_key, scope))
                    }) {
                        return false;
                    }
                    let siblings: Vec<_> = parent
                        .children()
                        .filter(|k| {
                            doc.node(*k).is_some_and(|n| {
                                n.kind() == NodeKind::Element
                                    && (!of_type
                                        || n.element_name().is_some_and(|e| {
                                            e.local == raw_tag
                                                && node
                                                    .element_name()
                                                    .is_some_and(|me| me.namespace == e.namespace)
                                        }))
                                    && of.is_none_or(|list| {
                                        list.iter().any(|s| s.matches_scoped(doc, *k, scope))
                                    })
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
                        | SimpleSelector::NthChildOf(a, b, reverse, _)
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
    scope: Option<NodeKey>,
) -> bool {
    const LIMIT: usize = 4096;
    let Some(node) = doc.node(subject) else {
        return false;
    };
    match combinator {
        Combinator::Child => node
            .children()
            .any(|c| selector.matches_scoped(doc, c, scope)),
        Combinator::Descendant => {
            let mut stack: Vec<NodeKey> = node.children().collect();
            let mut seen = 0;
            while let Some(k) = stack.pop() {
                seen += 1;
                if seen > LIMIT {
                    return false;
                }
                if selector.matches_scoped(doc, k, scope) {
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
                following
                    .take(1)
                    .any(|k| selector.matches_scoped(doc, k, scope))
            } else {
                following
                    .into_iter()
                    .any(|k| selector.matches_scoped(doc, k, scope))
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
        self.matches_scoped(doc, node_key, None)
    }

    /// Match with `:scope` as `scope` (a query's scoping element), or the root element.
    pub fn matches_scoped<D: DocumentView>(
        &self,
        doc: &D,
        node_key: NodeKey,
        scope: Option<NodeKey>,
    ) -> bool {
        if self.items.is_empty() {
            return false;
        }
        self.matches_at(doc, node_key, self.items.len() - 1, scope)
    }
    fn matches_at<D: DocumentView>(
        &self,
        doc: &D,
        key: NodeKey,
        index: usize,
        scope: Option<NodeKey>,
    ) -> bool {
        if !self.items[index].0.matches_scoped(doc, key, scope) {
            return false;
        }
        if index == 0 {
            return true;
        }
        let combinator = self.items[index - 1].1.unwrap_or(Combinator::Descendant);
        let parent = || {
            doc.node(key)
                .and_then(NodeView::parent)
                .filter(|p| doc.node(*p).is_some_and(|n| n.kind() == NodeKind::Element))
        };
        match combinator {
            Combinator::Child => {
                parent().is_some_and(|p| self.matches_at(doc, p, index - 1, scope))
            }
            Combinator::Descendant => {
                let mut ancestor = parent();
                while let Some(p) = ancestor {
                    if self.matches_at(doc, p, index - 1, scope) {
                        return true;
                    }
                    ancestor = doc
                        .node(p)
                        .and_then(NodeView::parent)
                        .filter(|p| doc.node(*p).is_some_and(|n| n.kind() == NodeKind::Element));
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
                        .is_some_and(|p| self.matches_at(doc, *p, index - 1, scope))
                } else {
                    previous
                        .into_iter()
                        .rev()
                        .any(|p| self.matches_at(doc, p, index - 1, scope))
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CSS PARSING (RULES & SELECTORS)
// ---------------------------------------------------------------------------

/// HTML attributes whose values match case-insensitively in selectors unless the `s` flag is
/// given (HTML §4.16.2, "case-sensitivity of selectors").
fn case_insensitive_attribute(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "accept"
            | "accept-charset"
            | "align"
            | "alink"
            | "axis"
            | "bgcolor"
            | "charset"
            | "checked"
            | "clear"
            | "codetype"
            | "color"
            | "compact"
            | "declare"
            | "defer"
            | "dir"
            | "direction"
            | "disabled"
            | "enctype"
            | "face"
            | "frame"
            | "hreflang"
            | "http-equiv"
            | "lang"
            | "language"
            | "link"
            | "media"
            | "method"
            | "multiple"
            | "nohref"
            | "noresize"
            | "noshade"
            | "nowrap"
            | "readonly"
            | "rel"
            | "rev"
            | "rules"
            | "scope"
            | "scrolling"
            | "selected"
            | "shape"
            | "target"
            | "text"
            | "type"
            | "valign"
            | "valuetype"
            | "vlink"
    )
}
