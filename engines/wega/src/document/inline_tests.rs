//! Native CSSOM values against Stylo: for every longhand, each value in a corpus of edge
//! cases must serialize exactly as Stylo serializes it, or be left to Stylo (unported).
use super::*;
use wega_css_parser::{
    ValueEngine,
    properties_table::{KEYWORD_TYPES, LONGHANDS},
};

/// Marks values the native engine does not handle, so only native results are compared.
struct Unported;
const UNPORTED: &str = "\u{0}unported";
impl ValueEngine for Unported {
    fn parse(&self, _: &str, _: &str) -> Option<Vec<(String, String)>> {
        Some(vec![(UNPORTED.into(), String::new())])
    }
    fn serialize_shorthand(&self, _: &str, _: &[&str]) -> Option<String> {
        None
    }
}

const CORPUS: &[&str] = &[
    "0",
    "1",
    "-1",
    "2",
    "1.5",
    "-0.5",
    "1e3",
    "1000",
    "1001",
    "0.0001",
    "10px",
    "-10px",
    "0px",
    "1.25EM",
    "3rem",
    "2vw",
    "1q",
    "5%",
    "-5%",
    "0%",
    "150%",
    "1s",
    "-2ms",
    "0s",
    "1deg",
    "1fr",
    "calc(1px)",
    "calc(1px + 2%)",
    "min(1px, 2px)",
    "auto",
    "none",
    "normal",
    "AUTO",
    "Normal",
    "hidden",
    "solid",
    "dotted",
    "inset",
    "center",
    "left",
    "right",
    "top",
    "bottom",
    "left 10px",
    "right -5%",
    "center center",
    "10px 20px",
    "10px 10px",
    "1px 2px 3px",
    "red",
    "RED",
    "transparent",
    "currentColor",
    "#abc",
    "#abcd",
    "#aabbcc",
    "#aabbcc80",
    "#12345",
    "rgb(1,2,3)",
    "url(a.png)",
    "url('a b.png')",
    "none, none",
    "url(a.png), none",
    "repeat-x",
    "repeat no-repeat",
    "space round",
    "cover",
    "contain",
    "auto auto",
    "10px auto",
    "border-box",
    "padding-box text",
    "text",
    "infinite",
    "2, infinite",
    "1s, 2s",
    "1s,",
    "wrap",
    "nowrap",
    "wrap-reverse",
    "balance",
    "wrap balance",
    "balance wrap-reverse",
    "wrap wrap",
    "flex",
    "inline flex",
    "block flow",
    "list-item",
    "grid",
    "contents",
    "inline-grid",
    "baseline",
    "first baseline",
    "last baseline",
    "safe center",
    "unsafe end",
    "space-between",
    "stretch",
    "legacy left",
    "flat",
    "preserve-3d",
    "static",
    "sticky",
    "nonzero",
    "evenodd",
    "context-fill-opacity",
    "context-stroke-opacity",
    "thin",
    "medium",
    "thick",
    "outset",
    "groove",
    "double",
    "-3px",
    "0.5px",
    "4",
    "8 ",
    "bold",
    "bolder",
    "lighter",
    "400",
    "0.5",
    "foo",
    "foo bar",
    "!",
    "1px !",
    "",
];

/// Deliberate differences: `(longhand, value)` Wega accepts where this Stylo build does not.
const WEGA_ONLY: &[(&str, &str)] = &[
    // Grid layout is behind a preference that is off in Stylo's Servo build; Wega lays it out.
    ("display", "grid"),
    ("display", "inline-grid"),
    // `flex-wrap: balance` is likewise behind a preference; Wega's flex layout balances lines.
    ("flex-wrap", "balance"),
    ("flex-wrap", "wrap balance"),
    ("flex-wrap", "balance wrap-reverse"),
    // Standard keywords Stylo compiles only into Gecko builds (WPT expects them valid).
    ("alignment-baseline", "alphabetic"),
    ("alignment-baseline", "ideographic"),
    ("alignment-baseline", "central"),
    ("alignment-baseline", "mathematical"),
    ("alignment-baseline", "hanging"),
    ("image-rendering", "smooth"),
    ("image-rendering", "optimizespeed"),
    ("image-rendering", "optimizequality"),
    ("pointer-events", "visiblepainted"),
    ("pointer-events", "visiblefill"),
    ("pointer-events", "visiblestroke"),
    ("pointer-events", "visible"),
    ("pointer-events", "painted"),
    ("pointer-events", "fill"),
    ("pointer-events", "stroke"),
    ("pointer-events", "all"),
    ("word-break", "break-word"),
];

#[test]
fn native_values_serialize_as_stylo_does() {
    let stylo = StyloValues {
        url: url::Url::parse("https://example.test/").unwrap().into(),
    };
    let native = NativeFirst(Unported);
    let mut mismatches = Vec::new();
    for longhand in LONGHANDS {
        // Longhands this Stylo build does not support at all are Wega's call, not a mismatch.
        if stylo.property_id(longhand.name).is_none() {
            continue;
        }
        // Every keyword the generated table knows for this longhand, with the corpus.
        let base = longhand.value_type.rsplit("::").next().unwrap_or_default();
        let keywords = KEYWORD_TYPES
            .iter()
            .filter(|(name, _)| *name == base)
            .flat_map(|(_, variants)| variants.iter())
            .flat_map(|(keyword, aliases)| std::iter::once(keyword).chain(aliases.iter()))
            .chain(longhand.keywords.iter())
            .copied();
        for value in keywords.chain(CORPUS.iter().copied()) {
            let value = &value;
            let ours = native.parse(longhand.name, value);
            if ours
                .as_ref()
                .is_some_and(|v| v.iter().any(|(n, _)| n == UNPORTED))
            {
                continue;
            }
            if WEGA_ONLY.contains(&(longhand.name, *value)) {
                continue;
            }
            let theirs = stylo.parse(longhand.name, value);
            if ours != theirs {
                mismatches.push(format!(
                    "{}: {value:?}: native {ours:?}, Stylo {theirs:?}",
                    longhand.name
                ));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}
