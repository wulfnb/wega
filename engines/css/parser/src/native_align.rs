//! CSS Box Alignment values (`align-content`, `justify-content`, `align-self`,
//! `justify-self`, `align-items`, `justify-items`): parsing and specified-value
//! `justify-self`, `align-items`, `justify-items`) and `display`: parsing and specified-value
//!
//! Ported from Stylo `values/specified/align.rs` (MPL-2.0): the same grammar per axis and
//! the same serialization as its `AlignFlags`. `anchor-center` is behind a preference that
//! is off in Servo builds, so it is rejected here as there.

/// The axis a property aligns in: `align-*` is the block axis, `justify-*` the inline axis.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    Block,
    Inline,
}

/// Which alignment grammar a value type uses.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grammar {
    /// `align-content`, `justify-content`.
    ContentDistribution,
    /// `align-self`, `justify-self`.
    SelfAlignment,
    /// `align-items`, `justify-items`.
    ItemPlacement,
}

/// The idents of `value`, lowercased, or `None` if it holds anything else.
fn idents(value: &str) -> Option<Vec<String>> {
    let mut parser = cssparser::Parser::new(value);
    let mut out = Vec::new();
    while !parser.is_exhausted() {
        out.push(parser.expect_ident().ok()?.to_ascii_lowercase());
    }
    Some(out)
}

/// `<baseline-position>`: `baseline`, `first baseline` or `last baseline`, serialized as
/// `baseline` or `last baseline`.
fn baseline(words: &[String]) -> Option<String> {
    match words
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["baseline"] | ["first", "baseline"] => Some("baseline".into()),
        ["last", "baseline"] => Some("last baseline".into()),
        _ => None,
    }
}

/// `<overflow-position>? <position>`, where `positions` lists the allowed positions.
fn positioned(words: &[String], positions: &[&str]) -> Option<String> {
    let (overflow, rest) = match words.first().map(String::as_str) {
        Some(o @ ("safe" | "unsafe")) => (Some(o), &words[1..]),
        _ => (None, words),
    };
    let [position] = rest else { return None };
    let position = positions.iter().find(|p| **p == position.as_str())?;
    Some(match overflow {
        Some(o) => format!("{o} {position}"),
        None => (*position).to_owned(),
    })
}

/// `<self-position>`, with `left` and `right` on the inline axis.
fn self_positions(axis: Axis) -> &'static [&'static str] {
    match axis {
        Axis::Block => &[
            "start",
            "end",
            "flex-start",
            "flex-end",
            "center",
            "self-start",
            "self-end",
        ],
        Axis::Inline => &[
            "start",
            "end",
            "flex-start",
            "flex-end",
            "center",
            "self-start",
            "self-end",
            "left",
            "right",
        ],
    }
}

/// `legacy | [ legacy && [ left | right | center ] ]`, serialized `legacy <position>`.
fn legacy(words: &[String]) -> Option<String> {
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let side = |w: &str| matches!(w, "left" | "right" | "center");
    match words.as_slice() {
        ["legacy"] => Some("legacy".into()),
        ["legacy", s] | [s, "legacy"] if side(s) => Some(format!("legacy {s}")),
        _ => None,
    }
}

/// Parse and serialize an alignment value, or `None` if invalid.
pub(crate) fn alignment(value: &str, grammar: Grammar, axis: Axis) -> Option<String> {
    let words = idents(value)?;
    let single = |options: &[&str]| -> Option<String> {
        match words.as_slice() {
            [w] => options
                .iter()
                .find(|o| **o == w.as_str())
                .map(|o| (*o).to_owned()),
            _ => None,
        }
    };
    match grammar {
        Grammar::ContentDistribution => single(&["normal"])
            .or_else(|| (axis == Axis::Block).then(|| baseline(&words)).flatten())
            .or_else(|| single(&["stretch", "space-between", "space-around", "space-evenly"]))
            .or_else(|| {
                let positions: &[&str] = match axis {
                    Axis::Block => &["start", "end", "flex-start", "flex-end", "center"],
                    Axis::Inline => &[
                        "start",
                        "end",
                        "flex-start",
                        "flex-end",
                        "center",
                        "left",
                        "right",
                    ],
                };
                positioned(&words, positions)
            }),
        Grammar::SelfAlignment => baseline(&words)
            .or_else(|| single(&["auto", "normal", "stretch"]))
            .or_else(|| positioned(&words, self_positions(axis))),
        Grammar::ItemPlacement => baseline(&words)
            .or_else(|| single(&["normal", "stretch"]))
            .or_else(|| (axis == Axis::Inline).then(|| legacy(&words)).flatten())
            .or_else(|| positioned(&words, self_positions(axis))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment_follows_stylo() {
        use Axis::*;
        use Grammar::*;
        let ok = |v: &str, g, a| alignment(v, g, a);
        assert_eq!(
            ok("Normal", ContentDistribution, Block).as_deref(),
            Some("normal")
        );
        assert_eq!(
            ok("first baseline", ContentDistribution, Block).as_deref(),
            Some("baseline")
        );
        assert_eq!(ok("baseline", ContentDistribution, Inline), None);
        assert_eq!(
            ok("safe  CENTER", ContentDistribution, Block).as_deref(),
            Some("safe center")
        );
        assert_eq!(ok("left", ContentDistribution, Block), None);
        assert_eq!(
            ok("left", ContentDistribution, Inline).as_deref(),
            Some("left")
        );
        assert_eq!(
            ok("space-evenly", ContentDistribution, Inline).as_deref(),
            Some("space-evenly")
        );
        assert_eq!(ok("safe space-evenly", ContentDistribution, Inline), None);
        assert_eq!(ok("auto", SelfAlignment, Block).as_deref(), Some("auto"));
        assert_eq!(
            ok("last baseline", SelfAlignment, Inline).as_deref(),
            Some("last baseline")
        );
        assert_eq!(
            ok("unsafe self-end", SelfAlignment, Block).as_deref(),
            Some("unsafe self-end")
        );
        assert_eq!(ok("anchor-center", SelfAlignment, Block), None);
        assert_eq!(ok("auto", ItemPlacement, Block), None);
        assert_eq!(
            ok("center legacy", ItemPlacement, Inline).as_deref(),
            Some("legacy center")
        );
        assert_eq!(ok("legacy", ItemPlacement, Block), None);
        assert_eq!(
            ok("legacy", ItemPlacement, Inline).as_deref(),
            Some("legacy")
        );
    }
}

/// `display`, ported from Stylo `values/specified/box.rs` (`Display`, `DisplayKeyword`,
/// Servo build): outside, inside and list-item parts, serialized in Stylo's canonical form
/// (`block flex` is `flex`, `inline flow-root` is `inline-block`).
pub(crate) fn display(value: &str) -> Option<String> {
    #[derive(Clone, Copy, PartialEq)]
    enum Out {
        None,
        Inline,
        Block,
        TableCaption,
        InternalTable,
    }
    let words = idents(value)?;
    let full = |w: &str| -> Option<(Out, &'static str)> {
        Some(match w {
            "none" => (Out::None, "none"),
            "contents" => (Out::None, "contents"),
            "inline-block" => (Out::Inline, "flow-root"),
            "inline-table" => (Out::Inline, "table"),
            "-webkit-flex" => (Out::Block, "flex"),
            "inline-flex" | "-webkit-inline-flex" => (Out::Inline, "flex"),
            "inline-grid" => (Out::Inline, "grid"),
            "table-caption" => (Out::TableCaption, "flow"),
            "table-row-group" | "table-header-group" | "table-footer-group" | "table-column"
            | "table-column-group" | "table-row" | "table-cell" => {
                (Out::InternalTable, table_inside(w))
            }
            _ => return None,
        })
    };
    let (outside, inside, list_item) = match words.as_slice() {
        [w] if full(w).is_some() => {
            let (o, i) = full(w)?;
            (o, i, false)
        }
        _ => {
            let (mut outside, mut inside, mut list_item) = (None, None, false);
            for w in &words {
                match w.as_str() {
                    "block" | "inline" if outside.is_none() => {
                        outside = Some(if w == "block" {
                            Out::Block
                        } else {
                            Out::Inline
                        })
                    }
                    "list-item" if !list_item => list_item = true,
                    "flow" | "flex" | "flow-root" | "table" | "grid" if inside.is_none() => {
                        inside = Some(match w.as_str() {
                            "flow" => "flow",
                            "flex" => "flex",
                            "flow-root" => "flow-root",
                            "table" => "table",
                            _ => "grid",
                        })
                    }
                    _ => return None,
                }
            }
            let inside = inside.unwrap_or("flow");
            if list_item && inside != "flow" {
                return None;
            }
            (outside.unwrap_or(Out::Block), inside, list_item)
        }
    };
    let outside_name = |o: Out| match o {
        Out::Inline => "inline",
        _ => "block",
    };
    Some(match (outside, inside, list_item) {
        (Out::Block | Out::Inline, "flow", false) => outside_name(outside).to_owned(),
        (Out::Inline, "flow-root", false) => "inline-block".into(),
        (Out::TableCaption, _, _) => "table-caption".into(),
        (Out::Inline, "grid", _) => "inline-grid".into(),
        (Out::Inline, "flex", _) => "inline-flex".into(),
        (Out::Inline, "table", _) => "inline-table".into(),
        (_, inside, true) => {
            let mut out = String::new();
            if outside != Out::Block {
                out.push_str(outside_name(outside));
                out.push(' ');
            }
            if inside != "flow" {
                out.push_str(inside);
                out.push(' ');
            }
            out.push_str("list-item");
            out
        }
        (_, inside, false) => inside.to_owned(),
    })
}

fn table_inside(word: &str) -> &'static str {
    match word {
        "table-row-group" => "table-row-group",
        "table-header-group" => "table-header-group",
        "table-footer-group" => "table-footer-group",
        "table-column" => "table-column",
        "table-column-group" => "table-column-group",
        "table-row" => "table-row",
        _ => "table-cell",
    }
}

#[cfg(test)]
mod display_tests {
    use super::display;

    #[test]
    fn display_serializes_canonically() {
        for (input, output) in [
            ("BLOCK", Some("block")),
            ("block flow", Some("block")),
            ("flow", Some("block")),
            ("inline flow-root", Some("inline-block")),
            ("block flex", Some("flex")),
            ("inline flex", Some("inline-flex")),
            ("-webkit-flex", Some("flex")),
            ("flow-root", Some("flow-root")),
            ("list-item", Some("list-item")),
            ("list-item block flow", Some("list-item")),
            ("inline list-item", Some("inline list-item")),
            ("table-cell", Some("table-cell")),
            ("none", Some("none")),
            ("contents", Some("contents")),
            ("flex list-item", None),
            ("block block", None),
            ("none block", None),
            ("ruby", None),
        ] {
            assert_eq!(display(input).as_deref(), output, "{input}");
        }
    }
}
