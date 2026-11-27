//! Shorthand expansion.
use crate::*;
use cssparser::{Parser, Token};
use wega_css_values::*;

pub fn shorthand_properties(property: &str) -> &'static [&'static str] {
    match property {
        "flex" => &["flex-grow", "flex-shrink", "flex-basis"],
        "flex-flow" => &["flex-direction", "flex-wrap"],
        "font" => &[
            "font-weight",
            "font-style",
            "font-size",
            "line-height",
            "font-family",
        ],
        _ => &[],
    }
}

pub fn expand_shorthand(decl: Declaration) -> Vec<Declaration> {
    let names = shorthand_properties(&decl.property);
    if names.is_empty() || decl.value.contains("var(") {
        // Substitution precedes shorthand parsing. Keep the pending shorthand
        // at its authored cascade position until custom properties are known.
        return vec![decl];
    }
    if matches!(
        decl.value.trim().to_ascii_lowercase().as_str(),
        "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "revert-rule"
    ) {
        return names
            .iter()
            .map(|p| Declaration::new(*p, decl.value.clone(), decl.important))
            .collect();
    }
    let properties = if decl.property == "font" {
        font_shorthand(&decl.value)
    } else if decl.property == "flex" {
        flex_shorthand(&decl.value)
    } else if decl.property == "flex-flow" {
        flex_flow_shorthand(&decl.value)
    } else {
        return vec![decl];
    };
    properties
        .unwrap_or_default()
        .into_iter()
        .map(|(p, v)| Declaration::new(p, v, decl.important))
        .collect()
}

pub(crate) fn font_shorthand(css: &str) -> Option<Vec<(&'static str, String)>> {
    use cssparser::ToCss;
    let mut p = Parser::new(css);
    let mut weight = "normal".to_owned();
    let mut italic = "normal".to_owned();
    let size = loop {
        let token = p.next().ok()?.clone();
        match &token {
            Token::Ident(s) if s.eq_ignore_ascii_case("normal") => {}
            Token::Ident(s)
                if s.eq_ignore_ascii_case("italic") || s.eq_ignore_ascii_case("oblique") =>
            {
                italic = s.to_string()
            }
            Token::Ident(s) if s.eq_ignore_ascii_case("bold") => weight = s.to_string(),
            Token::Number { value, .. } if *value >= 1.0 && *value <= 1000.0 => {
                weight = value.to_string()
            }
            Token::Dimension { .. } | Token::Percentage { .. } => break token.to_css_string(),
            _ => return None,
        }
    };
    let line = if p.try_parse(|p| p.expect_delim('/')).is_ok() {
        p.next().ok()?.to_css_string()
    } else {
        "normal".into()
    };
    let start = p.position();
    consume(&mut p).ok()?;
    let family = p.slice_from(start).trim().to_owned();
    if family.is_empty() {
        return None;
    }
    Some(vec![
        ("font-weight", weight),
        ("font-style", italic),
        ("font-size", size),
        ("line-height", line),
        ("font-family", family),
    ])
}

pub(crate) fn flex_shorthand(css: &str) -> Option<Vec<(&'static str, String)>> {
    let values: Vec<_> = css.split_ascii_whitespace().collect();
    let (grow, shrink, basis) = match values.as_slice() {
        ["none"] => (0.0, 0.0, "auto".to_owned()),
        ["auto"] => (1.0, 1.0, "auto".to_owned()),
        ["initial"] => (0.0, 1.0, "auto".to_owned()),
        [grow] => {
            if let Ok(g) = grow.parse::<f64>() {
                (g, 1.0, "0%".into())
            } else {
                Length::parse(grow)?;
                (1.0, 1.0, grow.to_string())
            }
        }
        [grow, second] => {
            let g = grow.parse::<f64>().ok()?;
            if let Ok(s) = second.parse::<f64>() {
                (g, s, "0%".into())
            } else {
                Length::parse(second)?;
                (g, 1.0, second.to_string())
            }
        }
        [grow, shrink, basis] => {
            Length::parse(basis)?;
            (
                grow.parse::<f64>().ok()?,
                shrink.parse::<f64>().ok()?,
                basis.to_string(),
            )
        }
        _ => return None,
    };
    if !grow.is_finite() || !shrink.is_finite() || grow < 0.0 || shrink < 0.0 {
        return None;
    }
    Some(vec![
        ("flex-grow", grow.to_string()),
        ("flex-shrink", shrink.to_string()),
        ("flex-basis", basis),
    ])
}

/// `flex-flow: <flex-direction> || <flex-wrap>`; an omitted part is reset to its initial
/// value.
pub(crate) fn flex_flow_shorthand(css: &str) -> Option<Vec<(&'static str, String)>> {
    let mut direction = None;
    let mut wrap = Vec::new();
    for word in css.split_ascii_whitespace() {
        let word = word.to_ascii_lowercase();
        match word.as_str() {
            "row" | "row-reverse" | "column" | "column-reverse" if direction.is_none() => {
                direction = Some(word)
            }
            _ => wrap.push(word),
        }
    }
    let wrap = if wrap.is_empty() {
        "nowrap".to_owned()
    } else {
        let wrap = wrap.join(" ");
        FlexWrapMode::parse(&wrap)?;
        wrap
    };
    if direction.is_none() && wrap == "nowrap" && !css.to_ascii_lowercase().contains("nowrap") {
        return None;
    }
    Some(vec![
        ("flex-direction", direction.unwrap_or_else(|| "row".into())),
        ("flex-wrap", wrap),
    ])
}

#[cfg(test)]
mod flex_flow_tests {
    use super::*;

    #[test]
    fn flex_flow_expands_to_direction_and_wrap() {
        let expand = |v: &str| {
            expand_shorthand(Declaration::new("flex-flow", v, false))
                .into_iter()
                .map(|d| (d.property, d.value))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            expand("wrap column"),
            [
                ("flex-direction".to_owned(), "column".to_owned()),
                ("flex-wrap".to_owned(), "wrap".to_owned())
            ]
        );
        assert_eq!(expand("row-reverse")[1].1, "nowrap");
        assert_eq!(expand("column balance")[1].1, "balance");
        assert!(expand("column sideways").is_empty());
        assert!(expand("row column").is_empty());
    }
}
