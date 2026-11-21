//! Selectors matches borrowed Wega nodes; no Stylo DOM or copied tree.
use super::*;
use ::selectors::{
    Element, OpaqueElement, SelectorImpl as SelectorImplementation,
    attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint},
    context::{
        MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
        SelectorCaches,
    },
    matching::{ElementSelectorFlags, matches_selector_list},
};
use style::selector_parser::{NonTSPseudoClass, PseudoElement, SelectorImpl, SelectorParser};
type Identifier = <SelectorImpl as SelectorImplementation>::Identifier;

#[derive(Clone, Copy)]
struct ElementRef<'a> {
    document: &'a WegaDocument,
    key: NodeKey,
}
impl std::fmt::Debug for ElementRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.key.fmt(f)
    }
}
impl<'a> ElementRef<'a> {
    fn new(document: &'a WegaDocument, key: NodeKey) -> Option<Self> {
        document.node(key)?.element_name()?;
        Some(Self { document, key })
    }
    fn node(self) -> wega_dom::document::DocumentNode<'a> {
        self.document.node(self.key).unwrap()
    }
    fn sibling(self, forward: bool) -> Option<Self> {
        let mut key = self.key;
        while let Some(next) = wega_dom::sibling(self.document, key, forward) {
            if let Some(element) = Self::new(self.document, next) {
                return Some(element);
            }
            key = next;
        }
        None
    }
}
impl Element for ElementRef<'_> {
    type Impl = SelectorImpl;
    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(self.node().identity_token())
    }
    fn parent_element(&self) -> Option<Self> {
        Self::new(self.document, self.node().parent()?)
    }
    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }
    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }
    fn is_pseudo_element(&self) -> bool {
        false
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        self.sibling(false)
    }
    fn next_sibling_element(&self) -> Option<Self> {
        self.sibling(true)
    }
    fn first_element_child(&self) -> Option<Self> {
        self.node()
            .children()
            .find_map(|key| Self::new(self.document, key))
    }
    fn is_html_element_in_html_document(&self) -> bool {
        self.node()
            .element_name()
            .is_some_and(|n| n.namespace == wega_dom::HTML_NAMESPACE)
    }
    fn has_local_name(
        &self,
        name: &<Self::Impl as SelectorImplementation>::BorrowedLocalName,
    ) -> bool {
        self.node().qual_name().is_some_and(|n| n.local == *name)
    }
    fn has_namespace(
        &self,
        namespace: &<Self::Impl as SelectorImplementation>::BorrowedNamespaceUrl,
    ) -> bool {
        self.node().qual_name().is_some_and(|n| n.ns == *namespace)
    }
    fn is_same_type(&self, other: &Self) -> bool {
        self.node()
            .qual_name()
            .zip(other.node().qual_name())
            .is_some_and(|(a, b)| a.local == b.local && a.ns == b.ns)
    }
    fn attr_matches(
        &self,
        namespace: &NamespaceConstraint<&<Self::Impl as SelectorImplementation>::NamespaceUrl>,
        name: &<Self::Impl as SelectorImplementation>::LocalName,
        operation: &AttrSelectorOperation<&<Self::Impl as SelectorImplementation>::AttrValue>,
    ) -> bool {
        self.node().attributes().any(|(n, value)| {
            n.local == name.0
                && match namespace {
                    NamespaceConstraint::Any => true,
                    NamespaceConstraint::Specific(ns) => n.ns == ns.0,
                }
                && operation.eval_str(value)
        })
    }
    fn match_non_ts_pseudo_class(
        &self,
        pseudo: &NonTSPseudoClass,
        _: &mut MatchingContext<Self::Impl>,
    ) -> bool {
        let node = self.node();
        let tag = node.element_name().unwrap().local;
        match pseudo {
            NonTSPseudoClass::Active => self.document.active == Some(self.key),
            NonTSPseudoClass::AnyLink | NonTSPseudoClass::Link => self.is_link(),
            NonTSPseudoClass::Checked => {
                if tag == "option" {
                    self.document.option_selected(self.key)
                } else {
                    self.document.checked(self.key)
                }
            }
            NonTSPseudoClass::Disabled => node.attr("disabled").is_some(),
            NonTSPseudoClass::Enabled => {
                matches!(
                    tag,
                    "button" | "input" | "select" | "textarea" | "option" | "optgroup" | "fieldset"
                ) && node.attr("disabled").is_none()
            }
            NonTSPseudoClass::Focus => self.document.focused() == Some(self.key),
            NonTSPseudoClass::Hover => self.document.hovered().is_some_and(|mut key| {
                loop {
                    if key == self.key {
                        return true;
                    }
                    let Some(parent) = self.document.node(key).and_then(NodeView::parent) else {
                        return false;
                    };
                    key = parent;
                }
            }),
            // Other platform-dependent pseudos were unsupported by the
            // reference query implementation. Do not manufacture state.
            _ => false,
        }
    }
    fn match_pseudo_element(&self, _: &PseudoElement, _: &mut MatchingContext<Self::Impl>) -> bool {
        false
    }
    fn apply_selector_flags(&self, _: ElementSelectorFlags) {} // Query caches live only for this immutable borrow.
    fn is_link(&self) -> bool {
        self.node()
            .element_name()
            .is_some_and(|n| matches!(n.local, "a" | "area"))
            && self.node().attr("href").is_some()
    }
    fn is_html_slot_element(&self) -> bool {
        self.node()
            .element_name()
            .is_some_and(|n| n.local == "slot" && n.namespace == wega_dom::HTML_NAMESPACE)
    }
    fn has_id(&self, id: &Identifier, case: CaseSensitivity) -> bool {
        self.node()
            .attr("id")
            .is_some_and(|value| case.eq(value.as_bytes(), id.to_string().as_bytes()))
    }
    fn has_class(&self, name: &Identifier, case: CaseSensitivity) -> bool {
        self.node().attr("class").is_some_and(|v| {
            v.split_ascii_whitespace()
                .any(|v| case.eq(v.as_bytes(), name.to_string().as_bytes()))
        })
    }
    fn has_custom_state(&self, _: &Identifier) -> bool {
        false
    }
    fn imported_part(&self, _: &Identifier) -> Option<Identifier> {
        None
    }
    fn is_part(&self, _: &Identifier) -> bool {
        false
    }
    fn is_empty(&self) -> bool {
        self.node().children().all(|k| {
            self.document.node(k).is_none_or(|n| {
                n.element_name().is_none() && n.text().is_none_or(str::is_empty)
                    || n.kind() == wega_dom::NodeKind::Comment
            })
        })
    }
    fn is_root(&self) -> bool {
        self.node().parent() == Some(self.document.root())
    }
    fn add_element_unique_hashes(&self, _: &mut ::selectors::bloom::BloomFilter) -> bool {
        false
    }
}
impl WegaDocument {
    fn selector_list(
        &self,
        selector: &str,
    ) -> Result<::selectors::SelectorList<SelectorImpl>, String> {
        SelectorParser::parse_author_origin_no_namespace(selector, &self.url().clone().into())
            .map_err(|_| "Invalid selector".into())
    }
    fn matching_context<'a>(
        &self,
        scope: NodeKey,
        caches: &'a mut SelectorCaches,
    ) -> MatchingContext<'a, SelectorImpl> {
        let mut context = MatchingContext::new(
            MatchingMode::Normal,
            None,
            caches,
            QuirksMode::NoQuirks,
            NeedsSelectorFlags::No,
            MatchingForInvalidation::No,
        );
        let scope = if scope == self.root() {
            self.node(scope)
                .and_then(|n| n.children().find(|k| ElementRef::new(self, *k).is_some()))
        } else {
            Some(scope)
        };
        context.scope_element = scope
            .and_then(|k| ElementRef::new(self, k))
            .map(|e| e.opaque());
        context
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
        let mut caches = SelectorCaches::default();
        let mut context = self.matching_context(scope, &mut caches);
        Ok(wega_dom::preorder(self, scope).skip(1).find(|key| {
            ElementRef::new(self, *key)
                .is_some_and(|e| matches_selector_list(&list, &e, &mut context))
        }))
    }
    pub fn query_selector_all_in(
        &self,
        scope: NodeKey,
        selector: &str,
    ) -> Result<Vec<NodeKey>, String> {
        let list = self.selector_list(selector)?;
        let mut caches = SelectorCaches::default();
        let mut context = self.matching_context(scope, &mut caches);
        Ok(wega_dom::preorder(self, scope)
            .skip(1)
            .filter(|key| {
                ElementRef::new(self, *key)
                    .is_some_and(|e| matches_selector_list(&list, &e, &mut context))
            })
            .collect())
    }
    pub fn matches_selector(&self, key: NodeKey, selector: &str) -> Result<bool, String> {
        let list = self.selector_list(selector)?;
        let mut caches = SelectorCaches::default();
        let mut context = self.matching_context(key, &mut caches);
        Ok(ElementRef::new(self, key)
            .is_some_and(|e| matches_selector_list(&list, &e, &mut context)))
    }
    pub fn closest(&self, key: NodeKey, selector: &str) -> Result<Option<NodeKey>, String> {
        let list = self.selector_list(selector)?;
        let mut caches = SelectorCaches::default();
        let mut context = self.matching_context(key, &mut caches);
        let mut current = ElementRef::new(self, key);
        while let Some(element) = current {
            if matches_selector_list(&list, &element, &mut context) {
                return Ok(Some(element.key));
            }
            current = element.parent_element();
        }
        Ok(None)
    }
}
