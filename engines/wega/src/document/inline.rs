//! Inline CSSOM uses Stylo declaration parsing against the authoritative attribute.
use super::*;
use cssparser::Parser;
use html5ever::{LocalName, ns};
use style::{
    context::QuirksMode,
    parser::ParserContext,
    properties::{
        PropertyDeclarationBlock, PropertyId, SourcePropertyDeclaration,
        declaration_block::{Importance, parse_style_attribute},
        parse_one_declaration_into,
    },
    stylesheets::{CssRuleType, Origin, supports_rule::parse_condition_or_declaration},
};
use style_traits::{CssStringWriter, ParsingMode};
use wega_dom::style::StyleError;
impl WegaDocument {
    pub(super) fn parse_style_attr_block(&self, style_attr: &str) -> PropertyDeclarationBlock {
        parse_style_attribute(
            style_attr,
            &self.url().clone().into(),
            None,
            QuirksMode::NoQuirks,
            CssRuleType::Style,
        )
    }

    /// The canonical CSSOM serialization of a `style` attribute string
    /// (`CSSStyleDeclaration.cssText` getter).
    pub fn style_attr_serialize(&self, style_attr: &str) -> String {
        let block = self.parse_style_attr_block(style_attr);
        let mut css = CssStringWriter::new();
        let _ = block.to_css(&mut css);
        css
    }

    /// The canonical serialization of `property`'s value in a `style` attribute
    /// string (`CSSStyleDeclaration.getPropertyValue()`). Handles shorthands.
    /// Returns an empty string if the property is not set or not recognised.
    pub fn style_attr_get_property(&self, style_attr: &str, property: &str) -> String {
        let Ok(property_id) = PropertyId::parse_enabled_for_all_content(property) else {
            return String::new();
        };
        let block = self.parse_style_attr_block(style_attr);
        let mut css = CssStringWriter::new();
        let _ = block.property_value_to_css(&property_id, &mut css);
        css
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
        let property_id = PropertyId::parse_enabled_for_all_content(property).ok()?;
        let mut block = self.parse_style_attr_block(style_attr);

        if value.trim().is_empty() {
            if let Some(first_declaration) = block.first_declaration_to_remove(&property_id) {
                block.remove_property(&property_id, first_declaration);
            }
        } else {
            let mut source = SourcePropertyDeclaration::default();
            parse_one_declaration_into(
                &mut source,
                property_id,
                value,
                Origin::Author,
                &self.url().clone().into(),
                None,
                ParsingMode::DEFAULT,
                QuirksMode::NoQuirks,
                CssRuleType::Style,
            )
            .ok()?;
            let importance = if important {
                Importance::Important
            } else {
                Importance::Normal
            };
            block.extend(source.drain(), importance);
        }

        let mut css = CssStringWriter::new();
        let _ = block.to_css(&mut css);
        Some(css)
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
        let property_id = PropertyId::parse_enabled_for_all_content(property).ok()?;
        let mut block = self.parse_style_attr_block(style_attr);

        let mut removed_value = CssStringWriter::new();
        let _ = block.property_value_to_css(&property_id, &mut removed_value);
        if let Some(first_declaration) = block.first_declaration_to_remove(&property_id) {
            block.remove_property(&property_id, first_declaration);
        }

        let mut css = CssStringWriter::new();
        let _ = block.to_css(&mut css);
        Some((css, removed_value))
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
        use style::properties::declaration_block::{
            Importance, SourcePropertyDeclarationUpdate, parse_one_declaration_into,
        };
        use style::properties::{PropertyId, SourcePropertyDeclaration};
        use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
        let current = self.node(key).ok_or(MutationError::Stale(key))?;
        if current.element_name().is_none() {
            return Err(MutationError::NotElement(key));
        }
        // Unknown property: CSSOM ignores the write.
        let Ok(property) = PropertyId::parse_enabled_for_all_content(property) else {
            return Ok(false);
        };
        // Work on a copy of the element's block and write it back through the `style`
        // attribute, which is how Blitz keeps the attribute and the block in step and
        // invalidates the element.
        let mut block = self.parse_style_attr_block(current.attr("style").unwrap_or_default());
        let changed = if value.trim().is_empty() {
            match block.first_declaration_to_remove(&property) {
                Some(first) => {
                    block.remove_property(&property, first);
                    true
                }
                None => false,
            }
        } else {
            let mut source = SourcePropertyDeclaration::default();
            let url_data = UrlExtraData(style::servo_arc::Arc::new(self.url().clone()));
            // A value that does not parse (including one carrying `!important`) is ignored.
            if parse_one_declaration_into(
                &mut source,
                property,
                value,
                Origin::Author,
                &url_data,
                None,
                style_traits::ParsingMode::DEFAULT,
                style::context::QuirksMode::NoQuirks,
                CssRuleType::Style,
            )
            .is_err()
            {
                return Ok(false);
            }
            let importance = if important {
                Importance::Important
            } else {
                Importance::Normal
            };
            // CSSOM update: an existing declaration keeps its position.
            let mut updates = SourcePropertyDeclarationUpdate::default();
            let changed = block.prepare_for_update(&source, importance, &mut updates);
            if changed {
                block.update(source.drain(), importance, &mut updates);
            }
            changed
        };
        if !changed {
            return Ok(false);
        }
        let mut css = String::new();
        let _ = block.to_css(&mut css);
        self.mutate().set_attribute(
            key,
            QualName::new(None, ns!(), LocalName::from("style")),
            &css,
        )?;
        Ok(true)
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
