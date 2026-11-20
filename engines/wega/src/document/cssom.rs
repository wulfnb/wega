// Modified by Wega contributors, 2026: opt-in CSS parse-error diagnostics.
//! CSSOM stylesheet access (`document.styleSheets`, `CSSStyleSheet`,
//! `CSSRuleList`, `CSSRule` and friends) backed by stylo's stylesheet objects.
//!
//! Stylesheets are identified by the `NodeKey` of their owner node (`<style>` or
//! `<link>`), and rules by a *path* of indices from the stylesheet's top-level
//! rule list down through nested rule lists (`@media`, `@supports`, nested
//! style rules, `@keyframes`, ...). An empty path denotes the stylesheet's own
//! top-level rule list.

use style::servo_arc::Arc as ServoArc;
use style::shared_lock::{Locked, SharedRwLockReadGuard, ToCssWithGuard};
use style::stylesheets::keyframes_rule::{Keyframe, KeyframesRule};
use style::stylesheets::{
    AllowImportRules, CssRule, CssRuleType, CssRuleTypes, CssRules, DocumentStyleSheet,
    RulesMutateError, StylesheetInDocument,
};
use style_traits::{CssStringWriter, ToCss};
// For `SelectorList::to_css_string`
use cssparser::ToCss as _;

use wega_dom::NodeKey;

use super::WegaDocument;
use super::resources::StylesheetLoader;

/// Errors from CSSOM mutation operations, mirroring the `DOMException` names
/// which the corresponding JavaScript APIs throw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssomError {
    /// The owner node has no associated stylesheet, or the rule path does not
    /// resolve to a rule (list).
    NotFound,
    /// `SyntaxError`
    Syntax,
    /// `IndexSizeError`
    IndexSize,
    /// `HierarchyRequestError`
    HierarchyRequest,
    /// `InvalidStateError`
    InvalidState,
    /// `NotSupportedError`
    NotSupported,
}

impl CssomError {
    /// The `DOMException` name corresponding to this error
    pub fn dom_exception_name(self) -> &'static str {
        match self {
            Self::NotFound => "NotFoundError",
            Self::Syntax => "SyntaxError",
            Self::IndexSize => "IndexSizeError",
            Self::HierarchyRequest => "HierarchyRequestError",
            Self::InvalidState => "InvalidStateError",
            Self::NotSupported => "NotSupportedError",
        }
    }
}

impl From<RulesMutateError> for CssomError {
    fn from(err: RulesMutateError) -> Self {
        match err {
            RulesMutateError::Syntax => Self::Syntax,
            RulesMutateError::IndexSize => Self::IndexSize,
            RulesMutateError::HierarchyRequest => Self::HierarchyRequest,
            RulesMutateError::InvalidState => Self::InvalidState,
        }
    }
}

/// A snapshot of the CSSOM-visible attributes of a rule
#[derive(Debug, Clone)]
pub struct CssRuleInfo {
    /// The name of the CSSOM interface for the rule (e.g. `"CSSStyleRule"`)
    pub interface: &'static str,
    /// The legacy `CSSRule.type` constant (0 for rules without one)
    pub rule_type: u16,
    /// `CSSRule.cssText`
    pub css_text: String,
    /// Whether the rule has a child rule list (`cssRules`)
    pub has_child_rules: bool,
    /// Whether the rule has a `style` declaration block
    pub has_style: bool,
    /// Additional interface-specific string attributes (e.g. `selectorText`)
    pub attributes: Vec<(&'static str, String)>,
}

/// A rule list that CSSOM rule paths can index into
#[derive(Clone)]
enum RuleList {
    Css(ServoArc<Locked<CssRules>>),
    Keyframes(ServoArc<Locked<KeyframesRule>>),
}

/// A rule resolved from a CSSOM rule path
#[derive(Clone)]
enum RuleHandle {
    Rule(CssRule),
    Keyframe(ServoArc<Locked<Keyframe>>),
}

fn child_rule_list(rule: &CssRule, guard: &SharedRwLockReadGuard) -> Option<RuleList> {
    let rules = match rule {
        CssRule::Style(r) => r.read_with(guard).rules.clone()?,
        CssRule::Media(r) => r.rules.clone(),
        CssRule::Supports(r) => r.rules.clone(),
        CssRule::Container(r) => r.rules.clone(),
        CssRule::Page(r) => r.read_with(guard).rules.clone(),
        CssRule::Document(r) => r.rules.clone(),
        CssRule::LayerBlock(r) => r.rules.clone(),
        CssRule::Scope(r) => r.rules.clone(),
        CssRule::StartingStyle(r) => r.rules.clone(),
        CssRule::Keyframes(r) => return Some(RuleList::Keyframes(r.clone())),
        _ => return None,
    };
    Some(RuleList::Css(rules))
}

fn rule_has_style(handle: &RuleHandle) -> bool {
    match handle {
        RuleHandle::Keyframe(_) => true,
        RuleHandle::Rule(rule) => matches!(
            rule,
            CssRule::Style(_)
                | CssRule::Page(_)
                | CssRule::Margin(_)
                | CssRule::NestedDeclarations(_)
                | CssRule::PositionTry(_)
                | CssRule::FontFace(_)
        ),
    }
}

fn css_rule_interface(rule: &CssRule) -> &'static str {
    match rule {
        CssRule::Style(_) => "CSSStyleRule",
        CssRule::Namespace(_) => "CSSNamespaceRule",
        CssRule::Import(_) => "CSSImportRule",
        CssRule::Media(_) => "CSSMediaRule",
        CssRule::CustomMedia(_) => "CSSCustomMediaRule",
        CssRule::Container(_) => "CSSContainerRule",
        CssRule::FontFace(_) => "CSSFontFaceRule",
        CssRule::FontFeatureValues(_) => "CSSFontFeatureValuesRule",
        CssRule::FontPaletteValues(_) => "CSSFontPaletteValuesRule",
        CssRule::CounterStyle(_) => "CSSCounterStyleRule",
        CssRule::Keyframes(_) => "CSSKeyframesRule",
        CssRule::Margin(_) => "CSSMarginRule",
        CssRule::Supports(_) => "CSSSupportsRule",
        CssRule::Page(_) => "CSSPageRule",
        CssRule::Property(_) => "CSSPropertyRule",
        CssRule::Document(_) => "CSSMozDocumentRule",
        CssRule::LayerBlock(_) => "CSSLayerBlockRule",
        CssRule::LayerStatement(_) => "CSSLayerStatementRule",
        CssRule::Scope(_) => "CSSScopeRule",
        CssRule::StartingStyle(_) => "CSSStartingStyleRule",
        CssRule::AppearanceBase(_) => "CSSAppearanceBaseRule",
        CssRule::PositionTry(_) => "CSSPositionTryRule",
        CssRule::NestedDeclarations(_) => "CSSNestedDeclarations",
        CssRule::ViewTransition(_) => "CSSViewTransitionRule",
    }
}

/// Join the CSS serializations of `items` with `", "`
fn comma_separated<'a, T: ToCss + 'a>(items: impl IntoIterator<Item = &'a T>) -> String {
    let mut css = CssStringWriter::new();
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            css.push_str(", ");
        }
        let _ = item.to_css(&mut style_traits::CssWriter::new(&mut css));
    }
    css
}

fn rule_attributes(rule: &CssRule, guard: &SharedRwLockReadGuard) -> Vec<(&'static str, String)> {
    match rule {
        CssRule::Style(r) => {
            vec![("selectorText", r.read_with(guard).selectors.to_css_string())]
        }
        CssRule::Namespace(r) => vec![
            ("namespaceURI", r.url.to_string()),
            (
                "prefix",
                r.prefix.as_ref().map(|p| p.to_string()).unwrap_or_default(),
            ),
        ],
        CssRule::Import(r) => {
            let r = r.read_with(guard);
            vec![
                ("href", r.url.as_str().to_string()),
                (
                    "media",
                    r.stylesheet
                        .media(guard)
                        .map(|m| m.to_css_string())
                        .unwrap_or_default(),
                ),
            ]
        }
        CssRule::Media(r) => vec![(
            "conditionText",
            r.media_queries.read_with(guard).to_css_string(),
        )],
        CssRule::Supports(r) => vec![("conditionText", r.condition.to_css_string())],
        CssRule::Container(r) => vec![("conditionText", r.conditions.to_css_string())],
        CssRule::Page(r) => vec![("selectorText", r.read_with(guard).selectors.to_css_string())],
        CssRule::Keyframes(r) => vec![("name", r.read_with(guard).name.to_css_string())],
        CssRule::LayerBlock(r) => vec![(
            "name",
            r.name
                .as_ref()
                .map(|n| n.to_css_string())
                .unwrap_or_default(),
        )],
        CssRule::LayerStatement(r) => vec![("nameList", comma_separated(r.names.iter()))],
        CssRule::Property(r) => {
            let mut attrs = vec![("name", r.name.to_css_string())];
            for (attr, id) in [
                ("syntax", style::properties::property::DescriptorId::Syntax),
                (
                    "inherits",
                    style::properties::property::DescriptorId::Inherits,
                ),
                (
                    "initialValue",
                    style::properties::property::DescriptorId::InitialValue,
                ),
            ] {
                let mut css = CssStringWriter::new();
                let _ = r.descriptors.get(id, &mut css);
                attrs.push((attr, css));
            }
            attrs
        }
        CssRule::CounterStyle(r) => vec![("name", r.read_with(guard).name().to_css_string())],
        CssRule::FontFeatureValues(r) => {
            vec![("fontFamily", comma_separated(r.family_names.iter()))]
        }
        CssRule::FontPaletteValues(r) => vec![
            ("name", r.name.to_css_string()),
            ("fontFamily", comma_separated(r.family_names.iter())),
            (
                "basePalette",
                r.base_palette
                    .as_ref()
                    .map(|b| b.to_css_string())
                    .unwrap_or_default(),
            ),
            ("overrideColors", comma_separated(r.override_colors.iter())),
        ],
        CssRule::Margin(r) => vec![("name", format!("{:?}", r.rule_type))],
        CssRule::PositionTry(r) => vec![("name", r.read_with(guard).name.to_css_string())],
        _ => Vec::new(),
    }
}

impl WegaDocument {
    pub fn stylesheet_url(&self, node_id: NodeKey) -> Option<url::Url> {
        let sheet = self.nodes_to_stylesheet.get(&node_id)?;
        Some((*sheet.contents(&self.guard.read()).url_data.0).clone())
    }
    /// Serialize the authoritative rules for the native renderer. Imported
    /// sheets are included only after the existing authorized loader completed.
    /// This is a rendering bridge, never a script-visible cross-origin read.
    pub fn native_stylesheet_text(&self, node_id: NodeKey) -> Option<String> {
        fn append(
            rules: &[CssRule],
            guard: &SharedRwLockReadGuard,
            out: &mut String,
            depth: usize,
            remaining: &mut usize,
        ) {
            if depth > 8 {
                return;
            }
            for rule in rules {
                if *remaining == 0 || out.len() > 8 * 1024 * 1024 {
                    break;
                }
                *remaining -= 1;
                if let CssRule::Import(import) = rule {
                    let import = import.read_with(guard);
                    if !matches!(
                        import.layer,
                        style::stylesheets::import_rule::ImportLayer::None
                    ) {
                        continue;
                    }
                    let media = import
                        .stylesheet
                        .media(guard)
                        .map(|m| m.to_css_string())
                        .unwrap_or_default();
                    if !media.is_empty() {
                        out.push_str("@media ");
                        out.push_str(&media);
                        out.push('{');
                    }
                    append(
                        import.stylesheet.rules(guard),
                        guard,
                        out,
                        depth + 1,
                        remaining,
                    );
                    if !media.is_empty() {
                        out.push('}');
                    }
                } else {
                    let _ = rule.to_css(guard, out);
                    out.push('\n');
                }
            }
        }
        let sheet = self.nodes_to_stylesheet.get(&node_id)?;
        let guard = self.guard.read();
        let mut text = String::new();
        append(
            sheet.contents(&guard).rules(&guard),
            &guard,
            &mut text,
            0,
            &mut 16384,
        );
        Some(text)
    }
    /// The owner nodes (`<style>` / `<link>` elements) of the document's author
    /// stylesheets (`document.styleSheets`).
    ///
    /// Ordered by node id, which matches document order for stylesheets
    /// created while parsing (and is the order the stylist uses too, see
    /// `add_stylesheet_for_node`).
    pub fn stylesheet_owner_nodes(&self) -> impl Iterator<Item = NodeKey> + '_ {
        self.nodes_to_stylesheet.keys().copied()
    }

    /// Whether `node_id` currently owns a stylesheet
    pub fn node_has_stylesheet(&self, node_id: NodeKey) -> bool {
        self.nodes_to_stylesheet.contains_key(&node_id)
    }

    /// A counter which changes whenever the set of stylesheets (or the
    /// stylesheet object owned by some node) changes. CSSOM wrappers use it to
    /// detect that rule data cached on the script side is stale.
    pub fn stylesheet_generation(&self) -> u64 {
        self.stylesheet_generation
    }

    /// Whether the stylesheet owned by `node_id` has been disabled through the
    /// CSSOM (`CSSStyleSheet.disabled` / `HTMLStyleElement.disabled` /
    /// `HTMLLinkElement.disabled`). `false` if the node owns no stylesheet.
    pub fn stylesheet_disabled(&self, node_id: NodeKey) -> bool {
        self.nodes_to_stylesheet
            .get(&node_id)
            .is_some_and(|sheet| sheet.0.disabled())
    }

    /// Set the disabled flag of the stylesheet owned by `node_id`. A disabled
    /// stylesheet stays associated with its owner node but its rules no longer
    /// apply to the document.
    ///
    /// Returns `false` if the node owns no stylesheet.
    pub fn set_stylesheet_disabled(&mut self, node_id: NodeKey, disabled: bool) -> bool {
        let Some(sheet) = self.nodes_to_stylesheet.get(&node_id) else {
            return false;
        };
        if sheet.0.set_disabled(disabled) {
            self.stylesheet_generation = self.stylesheet_generation.wrapping_add(1);
        }
        true
    }

    fn stylesheet_loader(&self) -> StylesheetLoader {
        self.loader()
    }

    /// Resolve `path` to the rule it denotes. The rule's ancestors (outermost
    /// first) are passed to `on_ancestor` as they are traversed.
    fn resolve_rule(
        sheet: &DocumentStyleSheet,
        guard: &SharedRwLockReadGuard,
        path: &[usize],
        on_ancestor: impl FnMut(&CssRule),
    ) -> Option<RuleHandle> {
        let (index, parent_path) = path.split_last()?;
        let list = Self::resolve_rule_list(sheet, guard, parent_path, on_ancestor)?;
        Some(match list {
            RuleList::Css(rules) => RuleHandle::Rule(rules.read_with(guard).0.get(*index)?.clone()),
            RuleList::Keyframes(keyframes) => {
                RuleHandle::Keyframe(keyframes.read_with(guard).keyframes.get(*index)?.clone())
            }
        })
    }

    /// Resolve `path` to the rule list owned by the rule it denotes (or the
    /// stylesheet's top-level rule list for an empty path). The ancestor rules
    /// of that list (outermost first, including the rule at `path` itself) are
    /// passed to `on_ancestor` as they are traversed.
    fn resolve_rule_list(
        sheet: &DocumentStyleSheet,
        guard: &SharedRwLockReadGuard,
        path: &[usize],
        mut on_ancestor: impl FnMut(&CssRule),
    ) -> Option<RuleList> {
        let mut list = RuleList::Css(sheet.contents(guard).rules.clone());
        for &index in path {
            let RuleList::Css(rules) = &list else {
                return None;
            };
            let rules = rules.read_with(guard);
            let rule = rules.0.get(index)?;
            let child_list = child_rule_list(rule, guard)?;
            on_ancestor(rule);
            list = child_list;
        }
        Some(list)
    }

    /// [`Self::resolve_rule_list`], also collecting the ancestor rules
    fn resolve_rule_list_with_ancestors(
        sheet: &DocumentStyleSheet,
        guard: &SharedRwLockReadGuard,
        path: &[usize],
    ) -> Option<(Vec<CssRule>, RuleList)> {
        let mut ancestors = Vec::with_capacity(path.len());
        let list =
            Self::resolve_rule_list(sheet, guard, path, |rule| ancestors.push(rule.clone()))?;
        Some((ancestors, list))
    }

    /// The number of rules in the rule list at `path` (`CSSRuleList.length`)
    pub fn stylesheet_rule_count(&self, node_id: NodeKey, path: &[usize]) -> Option<usize> {
        let sheet = self.nodes_to_stylesheet.get(&node_id)?;
        let guard = self.guard.read();
        let list = Self::resolve_rule_list(sheet, &guard, path, |_| {})?;
        Some(match list {
            RuleList::Css(rules) => rules.read_with(&guard).0.len(),
            RuleList::Keyframes(keyframes) => keyframes.read_with(&guard).keyframes.len(),
        })
    }

    /// The CSSOM-visible attributes of the rule at `path`
    pub fn stylesheet_rule_info(&self, node_id: NodeKey, path: &[usize]) -> Option<CssRuleInfo> {
        let sheet = self.nodes_to_stylesheet.get(&node_id)?;
        let guard = self.guard.read();
        let handle = Self::resolve_rule(sheet, &guard, path, |_| {})?;
        let mut css_text = CssStringWriter::new();
        let has_style = rule_has_style(&handle);
        Some(match handle {
            RuleHandle::Rule(rule) => {
                let _ = rule.to_css(&guard, &mut css_text);
                CssRuleInfo {
                    interface: css_rule_interface(&rule),
                    rule_type: rule.rule_type() as u16,
                    css_text,
                    has_child_rules: child_rule_list(&rule, &guard).is_some(),
                    has_style,
                    attributes: rule_attributes(&rule, &guard),
                }
            }
            RuleHandle::Keyframe(keyframe) => {
                let keyframe = keyframe.read_with(&guard);
                let _ = keyframe.to_css(&guard, &mut css_text);
                CssRuleInfo {
                    interface: "CSSKeyframeRule",
                    rule_type: CssRuleType::Keyframe as u16,
                    css_text,
                    has_child_rules: false,
                    has_style,
                    attributes: vec![("keyText", keyframe.selector.to_css_string())],
                }
            }
        })
    }

    /// Insert a rule parsed from `rule` at `index` in the rule list at `path`
    /// (`CSSStyleSheet.insertRule` / `CSSGroupingRule.insertRule` /
    /// `CSSKeyframesRule.appendRule`). Returns the index of the inserted rule.
    pub fn stylesheet_insert_rule(
        &mut self,
        node_id: NodeKey,
        path: &[usize],
        rule: &str,
        index: usize,
    ) -> Result<usize, CssomError> {
        let sheet = self
            .nodes_to_stylesheet
            .get(&node_id)
            .ok_or(CssomError::NotFound)?
            .clone();
        let loader = self.stylesheet_loader();
        let lock = &self.guard;

        let (ancestors, list) = {
            let guard = lock.read();
            Self::resolve_rule_list_with_ancestors(&sheet, &guard, path)
                .ok_or(CssomError::NotFound)?
        };

        let (new_rule, index) = match &list {
            RuleList::Css(rules) => {
                let new_rule = {
                    let guard = lock.read();
                    let contents = sheet.contents(&guard);
                    let containing_rule_types =
                        ancestors
                            .iter()
                            .fold(CssRuleTypes::default(), |types, rule| {
                                CssRuleTypes::from_bits(types.bits() | rule.rule_type().bit())
                            });
                    let parse_relative_rule_type = ancestors.last().map(|rule| rule.rule_type());
                    rules.read_with(&guard).parse_rule_for_insert(
                        lock,
                        rule,
                        contents,
                        index,
                        containing_rule_types,
                        parse_relative_rule_type,
                        Some(&loader),
                        AllowImportRules::Yes,
                    )?
                };
                {
                    let mut guard = lock.write();
                    rules
                        .write_with(&mut guard)
                        .0
                        .insert(index, new_rule.clone());
                }
                (new_rule, index)
            }
            RuleList::Keyframes(keyframes) => {
                let keyframe = {
                    let guard = lock.read();
                    let contents = sheet.contents(&guard);
                    Keyframe::parse(rule, contents, lock).map_err(|_| CssomError::Syntax)?
                };
                let index = {
                    let mut guard = lock.write();
                    let keyframes = &mut keyframes.write_with(&mut guard).keyframes;
                    keyframes.push(keyframe);
                    keyframes.len() - 1
                };
                (CssRule::Keyframes(keyframes.clone()), index)
            }
        };

        self.fetch_fonts(std::iter::once(&new_rule));
        self.stylesheet_generation = self.stylesheet_generation.wrapping_add(1);
        Ok(index)
    }

    /// Remove the rule at `index` from the rule list at `path`
    /// (`CSSStyleSheet.deleteRule` / `CSSGroupingRule.deleteRule` /
    /// `CSSKeyframesRule.deleteRule`).
    pub fn stylesheet_delete_rule(
        &mut self,
        node_id: NodeKey,
        path: &[usize],
        index: usize,
    ) -> Result<(), CssomError> {
        let sheet = self
            .nodes_to_stylesheet
            .get(&node_id)
            .ok_or(CssomError::NotFound)?
            .clone();
        let lock = &self.guard;

        let (_ancestors, list) = {
            let guard = lock.read();
            Self::resolve_rule_list_with_ancestors(&sheet, &guard, path)
                .ok_or(CssomError::NotFound)?
        };

        match &list {
            RuleList::Css(rules) => {
                let mut guard = lock.write();
                let rules = rules.write_with(&mut guard);
                rules.remove_rule(index)?;
            }
            RuleList::Keyframes(keyframes) => {
                let mut guard = lock.write();
                let list = &mut keyframes.write_with(&mut guard).keyframes;
                if index >= list.len() {
                    return Err(CssomError::IndexSize);
                }
                list.remove(index);
            }
        };

        self.stylesheet_generation = self.stylesheet_generation.wrapping_add(1);
        Ok(())
    }
}
