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
    let words = idents(value).filter(|w| !w.is_empty())?;
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

/// `flex-wrap`: `nowrap | [ wrap | wrap-reverse ] || balance`, serialized as Stylo's
/// `FlexWrap` bitflags (`wrap balance` is `balance`). Ported from Stylo
/// `values/specified/position.rs`; `balance` is enabled because Wega lays it out.
pub fn flex_wrap(value: &str) -> Option<String> {
    let words = idents(value).filter(|w| !w.is_empty())?;
    if let [w] = words.as_slice()
        && w == "nowrap"
    {
        return Some("nowrap".into());
    }
    let (mut wrap, mut reverse, mut balance) = (false, false, false);
    for w in &words {
        let flag = match w.as_str() {
            "wrap" => &mut wrap,
            "wrap-reverse" => &mut reverse,
            "balance" => &mut balance,
            _ => return None,
        };
        if *flag {
            return None;
        }
        *flag = true;
    }
    if wrap && reverse {
        return None;
    }
    let parts: Vec<&str> = [
        (wrap && !balance, "wrap"),
        (reverse, "wrap-reverse"),
        (balance, "balance"),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, name)| *name)
    .collect();
    Some(parts.join(" "))
}

/// `aspect-ratio`: `auto || <ratio>`, written `auto` first and the ratio as `a / b` (a single
/// number gains `/ 1`). Ported from Stylo `AspectRatio` and `Ratio` (MPL-2.0).
pub fn aspect_ratio(value: &str) -> super::Specified {
    use cssparser::{ParseError, Parser, ToCss, Token};
    let unported = std::cell::Cell::new(false);
    let number = |p: &mut Parser<'_>| -> Result<f32, ParseError<()>> {
        match p.next()?.clone() {
            Token::Number { value, .. } if value >= 0.0 && value.is_finite() => Ok(value),
            Token::Function(_) => {
                unported.set(true);
                Err(ParseError::unexpected_token())
            }
            _ => Err(ParseError::unexpected_token()),
        }
    };
    let ratio = |p: &mut Parser<'_>| -> Result<String, ParseError<()>> {
        let a = number(p)?;
        let b = match p.try_parse(|p| p.expect_delim('/')) {
            Ok(()) => number(p)?,
            Err(_) => 1.0,
        };
        Ok(format!("{} / {}", a.to_css_string(), b.to_css_string()))
    };
    let auto = |p: &mut Parser<'_>| p.try_parse(|p| p.expect_ident_matching("auto")).is_ok();
    let parsed = Parser::new(value).parse_entirely(|p| {
        let mut is_auto = auto(p);
        let ratio = p.try_parse(ratio).ok();
        if !is_auto {
            is_auto = auto(p);
        }
        match (is_auto, ratio) {
            (false, None) => Err(ParseError::<()>::unexpected_token()),
            (true, None) => Ok("auto".to_owned()),
            (false, Some(r)) => Ok(r),
            (true, Some(r)) => Ok(format!("auto {r}")),
        }
    });
    match parsed {
        _ if unported.get() => super::Specified::Unported,
        Ok(css) => super::Specified::Parsed(Some(css)),
        Err(_) => super::Specified::Parsed(None),
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
            ("", None),
        ] {
            assert_eq!(display(input).as_deref(), output, "{input}");
        }
    }
}
