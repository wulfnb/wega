//! Stylesheet, rule and declaration model.
use crate::*;
use wega_css_selectors::*;

#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub property: String,
    pub value: String,
    pub important: bool,
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
}

impl StyleSheet {
    pub fn parse(css: &str) -> Self {
        stylesheet(css)
    }
}

pub fn parse_declarations(s: &str) -> Vec<Declaration> {
    declarations(s)
}
