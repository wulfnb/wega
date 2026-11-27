//! Style caches. Phase 5 of the CSS engine modularization plan.
//!
//! [`SelectorMap`] buckets rules by the rightmost compound of their selectors (id, else
//! class, else tag), so the cascade only tests rules that can match an element. It returns
//! candidate rule indices in source order; callers still run full selector matching.
//!
//! Reference (concepts, not code): Stylo `selector_map.rs`.
#![forbid(unsafe_code)]
use std::collections::HashMap;
use wega_css_selectors::{ComplexSelector, SimpleSelector};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectorMap {
    /// Number of rules indexed; a different count means the map is stale.
    rule_count: usize,
    ids: HashMap<String, Vec<usize>>,
    classes: HashMap<String, Vec<usize>>,
    /// Lowercase local names (type selectors match ASCII case-insensitively).
    tags: HashMap<String, Vec<usize>>,
    /// Rules with a selector that has no id, class or type in its subject.
    universal: Vec<usize>,
}

enum Key<'a> {
    Id(&'a str),
    Class(&'a str),
    Tag(String),
    Universal,
}

fn key(selector: &ComplexSelector) -> Key<'_> {
    let Some((subject, _)) = selector.items.last() else {
        return Key::Universal;
    };
    let parts = &subject.components;
    if let Some(id) = parts.iter().find_map(|p| match p {
        SimpleSelector::Id(id) => Some(id.as_str()),
        _ => None,
    }) {
        return Key::Id(id);
    }
    if let Some(class) = parts.iter().find_map(|p| match p {
        SimpleSelector::Class(c) => Some(c.as_str()),
        _ => None,
    }) {
        return Key::Class(class);
    }
    if let Some(tag) = parts.iter().find_map(|p| match p {
        SimpleSelector::Type(t) => Some(t.to_ascii_lowercase()),
        _ => None,
    }) {
        return Key::Tag(tag);
    }
    Key::Universal
}

impl SelectorMap {
    /// Index rules, each given as its selector list, in source order.
    pub fn build<'a>(rules: impl IntoIterator<Item = &'a [ComplexSelector]>) -> Self {
        let mut map = Self::default();
        for (index, selectors) in rules.into_iter().enumerate() {
            map.rule_count = index + 1;
            for selector in selectors {
                let bucket = match key(selector) {
                    Key::Id(id) => map.ids.entry(id.to_owned()).or_default(),
                    Key::Class(c) => map.classes.entry(c.to_owned()).or_default(),
                    Key::Tag(t) => map.tags.entry(t).or_default(),
                    Key::Universal => &mut map.universal,
                };
                if bucket.last() != Some(&index) {
                    bucket.push(index);
                }
            }
        }
        map
    }

    /// Whether the map indexes exactly `rule_count` rules.
    pub fn covers(&self, rule_count: usize) -> bool {
        self.rule_count == rule_count
    }

    /// Indices of the rules that may match an element, ascending and without duplicates.
    pub fn candidates<'a>(
        &self,
        id: Option<&str>,
        classes: impl Iterator<Item = &'a str>,
        local_name: &str,
    ) -> Vec<usize> {
        let mut out = self.universal.clone();
        if let Some(rules) = id.and_then(|id| self.ids.get(id)) {
            out.extend(rules);
        }
        for class in classes {
            if let Some(rules) = self.classes.get(class) {
                out.extend(rules);
            }
        }
        if let Some(rules) = self.tags.get(&local_name.to_ascii_lowercase()) {
            out.extend(rules);
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wega_css_selectors::parse_selector_list;

    fn map(rules: &[&str]) -> SelectorMap {
        let parsed: Vec<_> = rules.iter().map(|r| parse_selector_list(r)).collect();
        SelectorMap::build(parsed.iter().map(Vec::as_slice))
    }

    #[test]
    fn candidates_follow_the_subject_compound_in_source_order() {
        let m = map(&[
            "#main",
            ".card",
            "DIV",
            "p span.note",
            "*",
            "a, .card",
            ":hover",
        ]);
        assert!(m.covers(7));
        assert_eq!(
            m.candidates(None, ["card"].into_iter(), "div"),
            vec![1, 2, 4, 5, 6]
        );
        assert_eq!(
            m.candidates(Some("main"), std::iter::empty(), "SECTION"),
            vec![0, 4, 6]
        );
        assert_eq!(
            m.candidates(None, ["note"].into_iter(), "span"),
            vec![3, 4, 6]
        );
        assert_eq!(m.candidates(None, std::iter::empty(), "a"), vec![4, 5, 6]);
    }
}
