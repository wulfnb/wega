//! CSSOM declaration blocks (`CSSStyleDeclaration`): parsing, `setProperty`,
//! `removeProperty`, `getPropertyValue` and serialization, including shorthand
//! recombination (CSSOM §6.6–6.7).
//!
//! Values are validated and serialized by a [`ValueEngine`], so the block algorithms do not
//! depend on how each property's grammar is implemented.
//!
//! Reference (concepts, not code): Stylo `properties/declaration_block.rs`.
use crate::properties_table::{SHORTHANDS, Shorthand, canonical_property, longhand};
use cssparser::{Parser, Token};

/// Parses and serializes property values.
pub trait ValueEngine {
    /// Parse `value` (no `!important`) for the canonical `property`, returning the longhand
    /// values it sets as `(longhand, serialized value)`: one pair for a longhand, every
    /// longhand for a shorthand. `None` if the value is invalid.
    fn parse(&self, property: &str, value: &str) -> Option<Vec<(String, String)>>;
    /// Serialize `shorthand` from its longhands' serialized values (in the shorthand's
    /// longhand order), or `None` if those values cannot be expressed by the shorthand.
    fn serialize_shorthand(&self, shorthand: &str, longhands: &[&str]) -> Option<String>;
}

/// One declaration of a block: a longhand or custom property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockDeclaration {
    pub property: String,
    /// The serialized value; empty when it is pending a shorthand's `var()` substitution.
    pub value: String,
    pub important: bool,
    /// For a longhand set by a shorthand containing `var()`: the shorthand and its value.
    pub pending: Option<(String, String)>,
}

/// An ordered CSSOM declaration block of longhands and custom properties.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclarationBlock {
    pub declarations: Vec<BlockDeclaration>,
}

const CSS_WIDE: [&str; 6] = [
    "initial",
    "inherit",
    "unset",
    "revert",
    "revert-layer",
    "revert-rule",
];

fn css_wide(value: &str) -> bool {
    CSS_WIDE.iter().any(|k| value.eq_ignore_ascii_case(k))
}

/// Whether `value` contains a `var()` (or `env()`/`attr()`) reference at any depth.
fn has_substitution(value: &str) -> bool {
    fn walk(p: &mut Parser<'_>) -> bool {
        while let Ok(token) = p.next() {
            match token.clone() {
                Token::Function(name)
                    if ["var", "env", "attr"]
                        .iter()
                        .any(|f| name.eq_ignore_ascii_case(f)) =>
                {
                    let _ = p.parse_nested_block(|p| {
                        while p.next().is_ok() {}
                        Ok::<_, cssparser::ParseError<()>>(())
                    });
                    return true;
                }
                Token::Function(_)
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::CurlyBracketBlock
                    if p.parse_nested_block(|p| Ok::<_, cssparser::ParseError<()>>(walk(p)))
                        .unwrap_or(false) =>
                {
                    return true;
                }
                _ => {}
            }
        }
        false
    }
    value.to_ascii_lowercase().contains('(') && walk(&mut Parser::new(value))
}

/// `value` without leading or trailing whitespace and comments.
fn trim_comments(mut value: &str) -> &str {
    loop {
        let trimmed = value.trim();
        let trimmed = match trimmed
            .strip_prefix("/*")
            .and_then(|r| r.find("*/").map(|e| &r[e + 2..]))
        {
            Some(rest) => rest,
            None => match trimmed
                .strip_suffix("*/")
                .and_then(|r| r.rfind("/*").map(|s| &r[..s]))
            {
                Some(rest) => rest,
                None => return trimmed,
            },
        };
        value = trimmed;
    }
}

/// Split a declaration value into the value and whether it ends in `!important`.
pub fn split_important(value: &str) -> (&str, bool) {
    let trimmed = value.trim_end();
    if let Some(bang) = trimmed.rfind('!') {
        let rest = trimmed[bang + 1..].trim();
        if rest.eq_ignore_ascii_case("important") {
            return (trimmed[..bang].trim_end(), true);
        }
    }
    (value, false)
}

impl DeclarationBlock {
    /// Parse a declaration list such as a `style` attribute (CSSOM "parse a CSS declaration
    /// block"). Invalid declarations are dropped.
    pub fn parse(css: &str, engine: &dyn ValueEngine) -> Self {
        let mut block = Self::default();
        for decl in crate::raw_declarations(css) {
            block.apply(&decl.property, &decl.value, decl.important, engine, true);
        }
        block
    }

    /// The longhand values for `property: value`, or `None` if invalid.
    fn expand(
        property: &str,
        value: &str,
        engine: &dyn ValueEngine,
    ) -> Option<Vec<BlockDeclaration>> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        if property.starts_with("--") {
            return Some(vec![BlockDeclaration {
                property: property.to_owned(),
                value: trim_comments(value).to_owned(),
                important: false,
                pending: None,
            }]);
        }
        let shorthand = crate::properties_table::shorthand(property);
        if css_wide(value) {
            let value = value.to_ascii_lowercase();
            let names: Vec<&str> = match shorthand {
                Some(s) => s.longhands.to_vec(),
                None => vec![property],
            };
            return Some(
                names
                    .into_iter()
                    .map(|name| BlockDeclaration {
                        property: name.to_owned(),
                        value: value.clone(),
                        important: false,
                        pending: None,
                    })
                    .collect(),
            );
        }
        if has_substitution(value) {
            // Values with substitutions keep their text, comments included, except around
            // the value.
            let raw = trim_comments(value).to_owned();
            if !crate::valid_var_references(&raw) {
                return None;
            }
            return Some(match shorthand {
                Some(s) => s
                    .longhands
                    .iter()
                    .map(|name| BlockDeclaration {
                        property: (*name).to_owned(),
                        value: String::new(),
                        important: false,
                        pending: Some((property.to_owned(), raw.clone())),
                    })
                    .collect(),
                None => vec![BlockDeclaration {
                    property: property.to_owned(),
                    value: raw,
                    important: false,
                    pending: None,
                }],
            });
        }
        let parsed = engine.parse(property, value)?;
        Some(
            parsed
                .into_iter()
                .map(|(property, value)| BlockDeclaration {
                    property,
                    value,
                    important: false,
                    pending: None,
                })
                .collect(),
        )
    }

    /// Add `property: value`. While parsing, a later declaration replaces an earlier one
    /// and moves to the end, except that a normal declaration never replaces an important
    /// one; through `setProperty` an existing declaration keeps its position.
    fn apply(
        &mut self,
        property: &str,
        value: &str,
        important: bool,
        engine: &dyn ValueEngine,
        parsing: bool,
    ) -> bool {
        let Some(property) = canonical_property(property) else {
            return false;
        };
        let Some(declarations) = Self::expand(&property, value, engine) else {
            return false;
        };
        let mut changed = false;
        for mut decl in declarations {
            decl.important = important;
            match self
                .declarations
                .iter()
                .position(|d| d.property == decl.property)
            {
                Some(i) if parsing => {
                    if self.declarations[i].important && !important {
                        continue;
                    }
                    self.declarations.remove(i);
                    self.declarations.push(decl);
                    changed = true;
                }
                Some(i) => {
                    // A later declaration of the same logical group with the other mapping
                    // (physical against flow-relative) must stay before this one, so the
                    // update moves to the end (as Stylo's `prepare_for_update`).
                    let reorder = longhand(&decl.property).is_some_and(|info| {
                        !info.logical_group.is_empty()
                            && self.declarations[i + 1..].iter().any(|d| {
                                longhand(&d.property).is_some_and(|other| {
                                    other.logical_group == info.logical_group
                                        && other.logical != info.logical
                                })
                            })
                    });
                    if reorder {
                        self.declarations.remove(i);
                        self.declarations.push(decl);
                        changed = true;
                    } else if self.declarations[i] != decl {
                        self.declarations[i] = decl;
                        changed = true;
                    }
                }
                None => {
                    self.declarations.push(decl);
                    changed = true;
                }
            }
        }
        changed
    }

    /// `setProperty(property, value, priority)`: an empty value removes the property.
    /// Returns whether the block changed; an invalid value leaves it unchanged.
    pub fn set_property(
        &mut self,
        property: &str,
        value: &str,
        important: bool,
        engine: &dyn ValueEngine,
    ) -> bool {
        if value.trim().is_empty() {
            let before = self.declarations.len();
            self.remove_property(property, engine);
            return self.declarations.len() != before;
        }
        self.apply(property, value, important, engine, false)
    }

    /// `removeProperty(property)`: the previous value, as `getPropertyValue` returned it.
    pub fn remove_property(&mut self, property: &str, engine: &dyn ValueEngine) -> String {
        let Some(property) = canonical_property(property) else {
            return String::new();
        };
        let previous = self.property_value(&property, engine);
        let names: Vec<String> = match crate::properties_table::shorthand(&property) {
            Some(s) => s.longhands.iter().map(|l| (*l).to_owned()).collect(),
            None => vec![property],
        };
        self.declarations.retain(|d| !names.contains(&d.property));
        previous
    }

    fn find(&self, property: &str) -> Option<&BlockDeclaration> {
        self.declarations.iter().find(|d| d.property == property)
    }

    /// `getPropertyValue(property)`. A shorthand has a value only when all its longhands
    /// are set with the same importance and the shorthand can express them.
    pub fn property_value(&self, property: &str, engine: &dyn ValueEngine) -> String {
        let Some(property) = canonical_property(property) else {
            return String::new();
        };
        match crate::properties_table::shorthand(&property) {
            Some(shorthand) => self
                .shorthand_value(shorthand, engine, &[])
                .map(|(value, _)| value)
                .unwrap_or_default(),
            None => self
                .find(&property)
                .map(|d| d.value.clone())
                .unwrap_or_default(),
        }
    }

    /// `getPropertyPriority(property)`: `"important"` or `""`.
    pub fn property_priority(&self, property: &str) -> &'static str {
        let Some(property) = canonical_property(property) else {
            return "";
        };
        let important = match crate::properties_table::shorthand(&property) {
            Some(s) => s
                .longhands
                .iter()
                .all(|l| self.find(l).is_some_and(|d| d.important)),
            None => self.find(&property).is_some_and(|d| d.important),
        };
        if important { "important" } else { "" }
    }

    /// The shorthand's value and importance from the block's longhands, skipping those in
    /// `excluded` (already serialized).
    fn shorthand_value(
        &self,
        shorthand: &Shorthand,
        engine: &dyn ValueEngine,
        excluded: &[&str],
    ) -> Option<(String, bool)> {
        let declarations: Vec<&BlockDeclaration> = shorthand
            .longhands
            .iter()
            .map(|l| {
                if excluded.contains(l) {
                    None
                } else {
                    self.find(l)
                }
            })
            .collect::<Option<_>>()?;
        let important = declarations[0].important;
        if declarations.iter().any(|d| d.important != important) {
            return None;
        }
        // Longhands pending one shorthand's `var()` serialize as that shorthand's value.
        if let Some((from, raw)) = &declarations[0].pending {
            return declarations
                .iter()
                .all(|d| d.pending.as_ref() == Some(&(from.clone(), raw.clone())))
                .then(|| (from == shorthand.name).then(|| (raw.clone(), important)))
                .flatten();
        }
        if declarations.iter().any(|d| d.pending.is_some()) {
            return None;
        }
        let first = &declarations[0].value;
        if css_wide(first) || declarations.iter().any(|d| css_wide(&d.value)) {
            return declarations
                .iter()
                .all(|d| d.value == *first)
                .then(|| (first.clone(), important));
        }
        let values: Vec<&str> = declarations.iter().map(|d| d.value.as_str()).collect();
        engine
            .serialize_shorthand(shorthand.name, &values)
            .map(|v| (v, important))
    }

    /// `cssText`: each declaration once, as the largest shorthand that can express it when
    /// all of that shorthand's longhands are present (CSSOM "serialize a CSS declaration
    /// block").
    pub fn to_css(&self, engine: &dyn ValueEngine) -> String {
        let mut out: Vec<String> = Vec::new();
        let mut done: Vec<&str> = Vec::new();
        for decl in &self.declarations {
            if done.contains(&decl.property.as_str()) {
                continue;
            }
            let mut shorthands: Vec<&Shorthand> = SHORTHANDS
                .iter()
                .filter(|s| s.longhands.contains(&decl.property.as_str()))
                .collect();
            // Prefer shorthands covering more longhands (`border` before `border-top`).
            shorthands.sort_by_key(|s| std::cmp::Reverse(s.longhands.len()));
            let mut serialized = false;
            for shorthand in shorthands {
                if self.mixed_logical_groups(shorthand) {
                    continue;
                }
                if let Some((value, important)) = self.shorthand_value(shorthand, engine, &done) {
                    out.push(declaration_text(shorthand.name, &value, important));
                    done.extend(shorthand.longhands.iter().copied());
                    serialized = true;
                    break;
                }
            }
            if !serialized {
                // A longhand pending its shorthand's `var()` serializes with an empty value.
                out.push(declaration_text(
                    &decl.property,
                    &decl.value,
                    decl.important,
                ));
                done.push(&decl.property);
            }
        }
        out.join(" ")
    }

    /// Whether, between the shorthand's first longhand and the point where all of them have
    /// been seen, a declaration that is not one of them falls in a logical property group
    /// one of them belongs to; the shorthand would then reorder physical and flow-relative
    /// declarations (as Stylo's `PropertyDeclarationBlock::to_css`; `all` is exempt).
    fn mixed_logical_groups(&self, shorthand: &Shorthand) -> bool {
        if shorthand.kind == "all" {
            return false;
        }
        let mut groups: Vec<&str> = Vec::new();
        let mut seen = 0;
        for decl in &self.declarations {
            let Some(info) = longhand(&decl.property) else {
                continue;
            };
            if shorthand.longhands.contains(&decl.property.as_str()) {
                if !info.logical_group.is_empty() {
                    groups.push(info.logical_group);
                }
                seen += 1;
                if seen == shorthand.longhands.len() {
                    return false;
                }
            } else if seen > 0 && groups.contains(&info.logical_group) {
                return true;
            }
        }
        false
    }

    /// The number of declarations (`CSSStyleDeclaration.length`) and their names in order.
    pub fn property_names(&self) -> impl Iterator<Item = &str> {
        self.declarations.iter().map(|d| d.property.as_str())
    }
}

fn declaration_text(property: &str, value: &str, important: bool) -> String {
    if important {
        format!("{property}: {value} !important;")
    } else {
        format!("{property}: {value};")
    }
}

/// Whether `name` is a known longhand (used by engines deciding whether to expand).
pub fn is_longhand(name: &str) -> bool {
    longhand(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Accepts any value; a shorthand sets every longhand to the whole value, and
    /// serializes back when all longhands are equal.
    struct Echo;
    impl ValueEngine for Echo {
        fn parse(&self, property: &str, value: &str) -> Option<Vec<(String, String)>> {
            if value == "bad" {
                return None;
            }
            Some(match crate::properties_table::shorthand(property) {
                Some(s) => s
                    .longhands
                    .iter()
                    .map(|l| ((*l).to_owned(), value.to_owned()))
                    .collect(),
                None => vec![(property.to_owned(), value.to_owned())],
            })
        }
        fn serialize_shorthand(&self, _: &str, longhands: &[&str]) -> Option<String> {
            longhands
                .iter()
                .all(|v| *v == longhands[0])
                .then(|| longhands[0].to_owned())
        }
    }

    #[test]
    fn parsing_keeps_important_and_moves_later_duplicates() {
        let block = DeclarationBlock::parse(
            "color: red !important; width: 1px; color: blue; WIDTH: 2px; bogus: 1; height: bad",
            &Echo,
        );
        assert_eq!(block.to_css(&Echo), "color: red !important; width: 2px;");
        assert_eq!(block.property_priority("color"), "important");
    }

    #[test]
    fn shorthands_recombine_only_when_complete() {
        let mut block = DeclarationBlock::parse("margin: 1px", &Echo);
        assert_eq!(block.to_css(&Echo), "margin: 1px;");
        assert_eq!(block.property_value("margin-top", &Echo), "1px");
        block.set_property("margin-left", "2px", false, &Echo);
        assert_eq!(block.property_value("margin", &Echo), "");
        assert_eq!(
            block.to_css(&Echo),
            "margin-top: 1px; margin-right: 1px; margin-bottom: 1px; margin-left: 2px;"
        );
        assert_eq!(block.remove_property("margin", &Echo), "");
        assert_eq!(block.to_css(&Echo), "");
        // Mixed importance does not recombine.
        let block = DeclarationBlock::parse("margin: 1px; margin-top: 1px !important", &Echo);
        assert_eq!(block.property_value("margin", &Echo), "");
    }

    #[test]
    fn set_property_updates_in_place_and_ignores_invalid_values() {
        let mut block = DeclarationBlock::parse("color: red; width: 1px", &Echo);
        assert!(block.set_property("color", "green", false, &Echo));
        assert!(!block.set_property("color", "bad", false, &Echo));
        assert!(!block.set_property("unknown-property", "1", false, &Echo));
        assert_eq!(block.to_css(&Echo), "color: green; width: 1px;");
        assert!(block.set_property("color", "", false, &Echo));
        assert_eq!(block.to_css(&Echo), "width: 1px;");
    }

    #[test]
    fn var_shorthands_css_wide_keywords_and_custom_properties() {
        let block = DeclarationBlock::parse("margin: var(--m) /* c */  1px; --x:  a  b ", &Echo);
        assert_eq!(
            block.property_value("margin", &Echo),
            "var(--m) /* c */  1px"
        );
        assert_eq!(block.property_value("margin-top", &Echo), "");
        assert_eq!(block.property_value("--x", &Echo), "a  b");
        assert_eq!(
            block.to_css(&Echo),
            "margin: var(--m) /* c */  1px; --x: a  b;"
        );
        // Pending longhands whose shorthand cannot be rebuilt serialize empty.
        let block = DeclarationBlock::parse("margin: var(--m); margin-top: 10px", &Echo);
        assert_eq!(
            block.to_css(&Echo),
            "margin-right: ; margin-bottom: ; margin-left: ; margin-top: 10px;"
        );
        // A flow-relative margin between the physical ones keeps them apart.
        let block = DeclarationBlock::parse(
            "margin-top: 1px; margin-right: 1px; margin-inline-start: 1px; margin-bottom: 1px; margin-left: 1px",
            &Echo,
        );
        assert!(!block.to_css(&Echo).starts_with("margin:"));
        let block = DeclarationBlock::parse(
            "margin: 10px; margin-inline: 10px; margin-block: 10px; margin-inline-end: 10px; margin-bottom: 10px",
            &Echo,
        );
        assert_eq!(
            block.to_css(&Echo),
            "margin-top: 10px; margin-right: 10px; margin-left: 10px; margin-inline-start: 10px; \
             margin-block: 10px; margin-inline-end: 10px; margin-bottom: 10px;"
        );
        let block =
            DeclarationBlock::parse("color: /* a */ var(--p) /* b */ var(--p) /* c */", &Echo);
        assert_eq!(
            block.property_value("color", &Echo),
            "var(--p) /* b */ var(--p)"
        );
        let block = DeclarationBlock::parse("padding: inherit", &Echo);
        assert_eq!(block.to_css(&Echo), "padding: inherit;");
        let block = DeclarationBlock::parse("padding: inherit; padding-top: initial", &Echo);
        assert_eq!(block.property_value("padding", &Echo), "");
    }
}
