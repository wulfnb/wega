//! `querySelector`, `matches` and `closest` on Wega's own selector engine
//! (`wega-css-selectors`), matching borrowed Wega nodes directly.
use super::*;
use wega_css_selectors::{ComplexSelector, selectors};

impl WegaDocument {
    /// Parse a selector list for a DOM query: an invalid one is a `SyntaxError`.
    fn selector_list(&self, selector: &str) -> Result<Vec<ComplexSelector>, String> {
        let list = selectors(selector);
        if list.is_empty() {
            return Err("Invalid selector".into());
        }
        Ok(list)
    }

    /// The element `:scope` stands for: the scoping element, or the document element when
    /// the query runs on the document.
    fn scope_element(&self, scope: NodeKey) -> Option<NodeKey> {
        if scope == self.root() {
            self.node(scope)?.children().find(|k| {
                self.node(*k)
                    .is_some_and(|n| n.kind() == wega_dom::NodeKind::Element)
            })
        } else {
            Some(scope)
        }
    }

    fn matches_list(&self, list: &[ComplexSelector], key: NodeKey, scope: Option<NodeKey>) -> bool {
        self.node(key)
            .is_some_and(|n| n.kind() == wega_dom::NodeKind::Element)
            && list.iter().any(|s| s.matches_scoped(self, key, scope))
    }

    pub fn query_selector(&self, selector: &str) -> Result<Option<NodeKey>, String> {
        self.query_selector_in(self.root(), selector)
    }
    pub fn query_selector_all(&self, selector: &str) -> Result<Vec<NodeKey>, String> {
        self.query_selector_all_in(self.root(), selector)
    }
    pub fn query_selector_in(
        &self,
        scope: NodeKey,
        selector: &str,
    ) -> Result<Option<NodeKey>, String> {
        let list = self.selector_list(selector)?;
        let scope_element = self.scope_element(scope);
        Ok(wega_dom::preorder(self, scope)
            .skip(1)
            .find(|key| self.matches_list(&list, *key, scope_element)))
    }
    pub fn query_selector_all_in(
        &self,
        scope: NodeKey,
        selector: &str,
    ) -> Result<Vec<NodeKey>, String> {
        let list = self.selector_list(selector)?;
        let scope_element = self.scope_element(scope);
        Ok(wega_dom::preorder(self, scope)
            .skip(1)
            .filter(|key| self.matches_list(&list, *key, scope_element))
            .collect())
    }
    pub fn matches_selector(&self, key: NodeKey, selector: &str) -> Result<bool, String> {
        let list = self.selector_list(selector)?;
        Ok(self.matches_list(&list, key, Some(key)))
    }
    pub fn closest(&self, key: NodeKey, selector: &str) -> Result<Option<NodeKey>, String> {
        let list = self.selector_list(selector)?;
        let mut current = Some(key);
        while let Some(candidate) = current {
            if self
                .node(candidate)
                .is_none_or(|n| n.kind() != wega_dom::NodeKind::Element)
            {
                return Ok(None);
            }
            if self.matches_list(&list, candidate, Some(key)) {
                return Ok(Some(candidate));
            }
            current = self.node(candidate).and_then(|n| n.parent());
        }
        Ok(None)
    }
}
