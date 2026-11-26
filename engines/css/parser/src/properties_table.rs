//! The CSS property table: every property the engine accepts, with its value type, keyword
//! values, inheritance, aliases and shorthand structure.
//!
//! The data is generated from Stylo's property metadata (`stylo/properties/longhands.toml`,
//! `shorthands.toml`, MPL-2.0) by `tools/css-properties/generate.py`.
pub use crate::generated::properties::{ALIASES, LONGHANDS, SHORTHANDS};

/// A longhand property.
#[derive(Debug)]
pub struct Longhand {
    pub name: &'static str,
    /// Stylo's specified value type (`Color`, `Size`, ...), or `keyword`.
    pub value_type: &'static str,
    /// The keyword values, for keyword properties.
    pub keywords: &'static [&'static str],
    pub inherited: bool,
    /// A flow-relative property (`margin-inline-start`).
    pub logical: bool,
    /// A comma-separated list of values (`background-image`).
    pub list: bool,
    /// The logical property group (`margin` for `margin-top` and `margin-inline-start`).
    pub logical_group: &'static str,
}

/// A shorthand property.
#[derive(Debug)]
pub struct Shorthand {
    pub name: &'static str,
    pub longhands: &'static [&'static str],
    /// Stylo's serialization kind: `four_sides`, `two_properties`, `single_border`, `all`
    /// or ``.
    pub kind: &'static str,
}

/// The longhand named `name` (already canonical and lowercase).
pub fn longhand(name: &str) -> Option<&'static Longhand> {
    LONGHANDS
        .binary_search_by(|l| l.name.cmp(name))
        .ok()
        .map(|i| &LONGHANDS[i])
}

/// The shorthand named `name` (already canonical and lowercase).
pub fn shorthand(name: &str) -> Option<&'static Shorthand> {
    SHORTHANDS
        .binary_search_by(|s| s.name.cmp(name))
        .ok()
        .map(|i| &SHORTHANDS[i])
}

/// The canonical name of a property, resolving case and aliases (`-webkit-flex` is `flex`),
/// or `None` for an unknown property. Custom properties are returned as written.
pub fn canonical_property(name: &str) -> Option<String> {
    if name.starts_with("--") {
        return (name.len() > 2).then(|| name.to_owned());
    }
    let lower = name.to_ascii_lowercase();
    if longhand(&lower).is_some() || shorthand(&lower).is_some() {
        return Some(lower);
    }
    ALIASES
        .binary_search_by(|(alias, _)| (*alias).cmp(lower.as_str()))
        .ok()
        .map(|i| ALIASES[i].1.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_resolves_aliases() {
        assert!(LONGHANDS.windows(2).all(|w| w[0].name < w[1].name));
        assert!(SHORTHANDS.windows(2).all(|w| w[0].name < w[1].name));
        assert!(ALIASES.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(canonical_property("Display").as_deref(), Some("display"));
        assert_eq!(canonical_property("-webkit-flex").as_deref(), Some("flex"));
        assert_eq!(
            canonical_property("word-wrap").as_deref(),
            Some("overflow-wrap")
        );
        assert_eq!(canonical_property("grid-gap").as_deref(), Some("gap"));
        assert_eq!(canonical_property("--x").as_deref(), Some("--x"));
        assert_eq!(canonical_property("--"), None);
        assert_eq!(canonical_property("not-a-property"), None);
        assert!(longhand("color").unwrap().inherited);
        assert!(!longhand("margin-top").unwrap().inherited);
        assert_eq!(shorthand("margin").unwrap().kind, "four_sides");
        assert_eq!(
            longhand("background-attachment").unwrap().keywords,
            ["scroll", "fixed"]
        );
    }
}
