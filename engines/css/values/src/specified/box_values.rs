//! `display`, ported from Stylo `values/specified/box.rs` (MPL-2.0; `Display`,
//! `DisplayKeyword`, Servo build): outside, inside and list-item parts, serialized in
//! Stylo's canonical form (`block flex` is `flex`, `inline flow-root` is `inline-block`).
use super::idents;

/// Parse and serialize a `display` value, or `None` if invalid.
pub fn display(value: &str) -> Option<String> {
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
