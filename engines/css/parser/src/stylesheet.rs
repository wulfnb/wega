//! Stylesheet, rule and declaration model.
use crate::*;
use wega_css_selectors::*;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Declaration {
    pub property: String,
    pub value: String,
    pub important: bool,
    /// The value parsed for a ported property, or why it stays a string.
    pub typed: Parsed,
}

impl Declaration {
    /// A declaration, with the value parsed now if the property is ported.
    pub fn new(property: impl Into<String>, value: impl Into<String>, important: bool) -> Self {
        let (property, value) = (property.into(), value.into());
        let typed = PropertyDeclaration::parse(&property, &value);
        Self {
            property,
            value,
            important,
            typed,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FontSourceSpec {
    pub url: String,
    pub format: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FontFaceRule {
    pub font_family: String,
    pub src: Vec<FontSourceSpec>,
    pub font_weight: Option<u16>,
    pub font_style: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportRule {
    pub url: String,
    pub media: Vec<MediaQuery>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub selectors: Vec<ComplexSelector>,
    pub declarations: Vec<Declaration>,
    pub media: Vec<Vec<MediaQuery>>,
    /// Cascade layer order within the sheet (`None` = unlayered).
    pub layer: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct StyleSheet {
    pub rules: Vec<Rule>,
    pub font_faces: Vec<FontFaceRule>,
    pub imports: Vec<ImportRule>,
    pub unsupported: Vec<String>,
    /// `@property` registrations: (name, initial value, inherits).
    pub registered_properties: Vec<(String, String, bool)>,
    /// Candidate rules by id, class and tag. Rebuild with [`StyleSheet::reindex`] after
    /// changing `rules`; a stale count makes the cascade scan every rule instead.
    pub index: wega_css_cache::SelectorMap,
}

impl StyleSheet {
    /// Rebuild the selector map from `rules`.
    pub fn reindex(&mut self) {
        self.index =
            wega_css_cache::SelectorMap::build(self.rules.iter().map(|r| r.selectors.as_slice()));
    }
    pub fn parse(css: &str) -> Self {
        stylesheet(css)
    }
}

pub fn parse_declarations(s: &str) -> Vec<Declaration> {
    declarations(s)
}
