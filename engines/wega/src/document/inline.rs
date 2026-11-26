//! Inline CSSOM (`element.style`) on Wega's declaration block, against the authoritative
//! `style` attribute. Property values are still parsed and serialized by Stylo through
//! [`StyloValues`] until native value types replace it.
use super::*;
use cssparser::Parser;
use html5ever::{LocalName, ns};
use style::{
    context::QuirksMode,
    parser::ParserContext,
    properties::{
        PropertyDeclarationBlock, PropertyId, SourcePropertyDeclaration,
        declaration_block::parse_style_attribute, parse_one_declaration_into,
    },
    stylesheets::{CssRuleType, Origin, supports_rule::parse_condition_or_declaration},
};
use style_traits::{CssStringWriter, ParsingMode};
use wega_css_parser::{
    DeclarationBlock, NativeFirst, ValueEngine, properties_table::canonical_property,
};
use wega_dom::style::StyleError;
impl WegaDocument {
    pub(super) fn parse_style_attr_block(&self, style_attr: &str) -> PropertyDeclarationBlock {
        parse_style_attribute(
            style_attr,
            &self.url().clone().into(),
            Some(&super::resources::CssReporter),
            QuirksMode::NoQuirks,
            CssRuleType::Style,
        )
    }

    /// The `style` attribute as a CSSOM declaration block, with values handled by Stylo.
    fn inline_block(&self, style_attr: &str) -> DeclarationBlock {
        DeclarationBlock::parse(style_attr, &self.value_engine())
    }

    /// Native value types where ported, Stylo for the rest.
    fn value_engine(&self) -> NativeFirst<StyloValues> {
        NativeFirst(StyloValues {
            url: self.url().clone().into(),
        })
    }

    /// The canonical CSSOM serialization of a `style` attribute string
    /// (`CSSStyleDeclaration.cssText` getter).
    pub fn style_attr_serialize(&self, style_attr: &str) -> String {
        self.inline_block(style_attr).to_css(&self.value_engine())
    }

    /// The canonical serialization of `property`'s value in a `style` attribute
    /// string (`CSSStyleDeclaration.getPropertyValue()`). Handles shorthands.
    /// Returns an empty string if the property is not set or not recognised.
    pub fn style_attr_get_property(&self, style_attr: &str, property: &str) -> String {
        self.inline_block(style_attr)
            .property_value(property, &self.value_engine())
    }

    /// Set `property` to `value` in a `style` attribute string
    /// (`CSSStyleDeclaration.setProperty()`), expanding shorthands.
    ///
    /// Returns the new (canonically serialized) style attribute, or `None` if
    /// the declaration was invalid (in which case the style is unchanged, per
    /// CSSOM). An empty `value` removes the property.
    pub fn style_attr_set_property(
        &self,
        style_attr: &str,
        property: &str,
        value: &str,
        important: bool,
    ) -> Option<String> {
        let engine = self.value_engine();
        let mut block = self.inline_block(style_attr);
        if !value.trim().is_empty() && !block.set_property(property, value, important, &engine) {
            canonical_property(property)?;
            // An invalid value, unless it equals the current one.
            let unchanged = block.property_value(property, &engine);
            if unchanged.is_empty() {
                return None;
            }
        } else if value.trim().is_empty() {
            block.remove_property(property, &engine);
        }
        Some(block.to_css(&engine))
    }

    /// Remove `property` from a `style` attribute string
    /// (`CSSStyleDeclaration.removeProperty()`), removing all longhands if it
    /// is a shorthand.
    ///
    /// Returns the new (canonically serialized) style attribute and the removed
    /// property's previous serialized value.
    pub fn style_attr_remove_property(
        &self,
        style_attr: &str,
        property: &str,
    ) -> Option<(String, String)> {
        canonical_property(property)?;
        let engine = self.value_engine();
        let mut block = self.inline_block(style_attr);
        let removed = block.remove_property(property, &engine);
        Some((block.to_css(&engine), removed))
    }

    /// Evaluate a `@supports` condition (e.g. `(display: grid)`,
    /// `selector(:hover)`) or a bare declaration (e.g. `display: grid`), as
    /// used by the CSSOM `CSS.supports(conditionText)` API. Returns `false`
    /// for unparseable conditions.
    pub fn css_supports_condition(&self, condition: &str) -> bool {
        let mut parser = Parser::new(condition);
        let Ok(condition) = parser.parse_entirely(parse_condition_or_declaration) else {
            return false;
        };

        let url_data = self.url().clone().into();
        let context = ParserContext::new(
            Origin::Author,
            &url_data,
            Some(CssRuleType::Style),
            ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            Default::default(),
            None,
            None,
            Default::default(),
        );
        condition.eval(&context)
    }

    pub fn set_inline_property(
        &mut self,
        key: NodeKey,
        property: &str,
        value: &str,
        important: bool,
    ) -> Result<bool, MutationError> {
        let current = self.node(key).ok_or(MutationError::Stale(key))?;
        if current.element_name().is_none() {
            return Err(MutationError::NotElement(key));
        }
        // Work on the element's block and write it back through the `style` attribute,
        // which keeps the attribute and the block in step and invalidates the element.
        // Unknown properties and invalid values (including ones carrying `!important`)
        // leave the block unchanged.
        let engine = self.value_engine();
        let mut block = self.inline_block(current.attr("style").unwrap_or_default());
        if !block.set_property(property, value, important, &engine) {
            return Ok(false);
        }
        let css = block.to_css(&engine);
        self.mutate().set_attribute(
            key,
            QualName::new(None, ns!(), LocalName::from("style")),
            &css,
        )?;
        Ok(true)
    }

    /// `CSSStyleDeclaration.length` and `item()`: the inline block's longhands in order.
    pub fn inline_property_names(&self, key: NodeKey) -> Result<Vec<String>, StyleError> {
        Ok(self
            .inline_block(self.style_attribute(key)?)
            .property_names()
            .map(str::to_owned)
            .collect())
    }

    /// `CSSStyleDeclaration.getPropertyPriority()`.
    pub fn inline_priority(
        &self,
        key: NodeKey,
        property: &str,
    ) -> Result<&'static str, StyleError> {
        Ok(self
            .inline_block(self.style_attribute(key)?)
            .property_priority(property))
    }

    fn style_attribute(&self, key: NodeKey) -> Result<&str, StyleError> {
        let node = self.node(key).ok_or(StyleError::Stale(key))?;
        if node.element_name().is_none() {
            return Err(StyleError::NotElement(key));
        }
        Ok(node.attr("style").unwrap_or_default())
    }
    pub fn inline_style(&self, key: NodeKey, out: &mut String) -> Result<(), StyleError> {
        out.push_str(&self.style_attr_serialize(self.style_attribute(key)?));
        Ok(())
    }
    pub fn inline_property(
        &self,
        key: NodeKey,
        property: &str,
        out: &mut String,
    ) -> Result<(), StyleError> {
        out.push_str(&self.style_attr_get_property(self.style_attribute(key)?, property));
        Ok(())
    }
    pub fn supports_condition(&self, condition: &str) -> bool {
        self.css_supports_condition(condition)
    }
    pub fn supports_declaration(&self, property: &str, value: &str) -> bool {
        self.style_attr_set_property("", property, value, false)
            .is_some_and(|s| !s.is_empty())
    }
}

/// Stylo as the [`ValueEngine`] for declaration blocks: it validates one property's value,
/// serializes each longhand it sets, and serializes shorthands from longhand values.
pub(super) struct StyloValues {
    url: style::stylesheets::UrlExtraData,
}

impl StyloValues {
    /// The property as an author style attribute sees it (as `parse_style_attribute` does).
    fn property_id(&self, property: &str) -> Option<PropertyId> {
        let context = style::parser::ParserContext::new(
            Origin::Author,
            &self.url,
            Some(CssRuleType::Style),
            ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            Default::default(),
            None,
            None,
            Default::default(),
        );
        PropertyId::parse(property, &context).ok()
    }
}

impl ValueEngine for StyloValues {
    fn parse(&self, property: &str, value: &str) -> Option<Vec<(String, String)>> {
        let id = self.property_id(property)?;
        let mut source = SourcePropertyDeclaration::default();
        parse_one_declaration_into(
            &mut source,
            id,
            value,
            Origin::Author,
            &self.url,
            None,
            ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            CssRuleType::Style,
        )
        .ok()?;
        source
            .drain()
            .declarations
            .map(|declaration| {
                let mut css = String::new();
                declaration.to_css(&mut css).ok()?;
                Some((declaration.id().name().into_owned(), css))
            })
            .collect()
    }

    fn serialize_shorthand(&self, shorthand: &str, longhands: &[&str]) -> Option<String> {
        let names = wega_css_parser::properties_table::shorthand(shorthand)?.longhands;
        let css: String = names
            .iter()
            .zip(longhands)
            .map(|(name, value)| format!("{name}: {value}; "))
            .collect();
        let block = parse_style_attribute(
            &css,
            &self.url,
            None,
            QuirksMode::NoQuirks,
            CssRuleType::Style,
        );
        let id = self.property_id(shorthand)?;
        let mut out = CssStringWriter::new();
        block.property_value_to_css(&id, &mut out).ok()?;
        (!out.is_empty()).then_some(out)
    }
}
