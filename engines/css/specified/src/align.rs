//! CSS Box Alignment values (`align-content`, `justify-content`, `align-self`,
//! `justify-self`, `align-items`, `justify-items`): parsing and specified-value
//! serialization.
//!
//! Ported from Stylo `values/specified/align.rs` (MPL-2.0): the same grammar per axis and
//! the same serialization as its `AlignFlags`. `anchor-center` is behind a preference that
//! is off in Servo builds, so it is rejected here as there.
use super::idents;

/// The axis a property aligns in: `align-*` is the block axis, `justify-*` the inline axis.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Block,
    Inline,
}

/// Which alignment grammar a value type uses.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Grammar {
    /// `align-content`, `justify-content`.
    ContentDistribution,
    /// `align-self`, `justify-self`.
    SelfAlignment,
    /// `align-items`, `justify-items`.
    ItemPlacement,
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
pub fn alignment(value: &str, grammar: Grammar, axis: Axis) -> Option<String> {
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
