//! Custom properties: `var()` substitution and cycle detection.
use cssparser::{ParseError, Parser, Token};
use wega_css_values::{Color, Length};
type Error = ParseError<()>;

pub fn substitute(css: &str, vars: &std::collections::BTreeMap<String, String>) -> Option<String> {
    expand(&mut Parser::new(css), vars, &mut Vec::new(), 0).ok()
}

pub(crate) fn expand(
    p: &mut Parser<'_>,
    vars: &std::collections::BTreeMap<String, String>,
    stack: &mut Vec<String>,
    depth: usize,
) -> Result<String, Error> {
    if stack.len() > 32 || depth > 64 {
        return Err(ParseError::unexpected_token());
    }
    let mut out = String::new();
    while !p.is_exhausted() {
        let start = p.position();
        let token = p.next_including_whitespace_and_comments()?.clone();
        match token {
            Token::Comment(_) => out.push(' '),
            Token::Function(f) if f.eq_ignore_ascii_case("var") => {
                let value = p.parse_nested_block(|p| {
                    let name = p.expect_ident()?.to_string();
                    if !name.starts_with("--") {
                        return Err(ParseError::unexpected_token());
                    }
                    let fallback = if !p.is_exhausted() {
                        p.expect_comma()?;
                        Some(expand(p, vars, stack, depth + 1)?)
                    } else {
                        None
                    };
                    let resolved = if stack.contains(&name) {
                        None
                    } else if let Some(raw) = vars.get(&name) {
                        stack.push(name);
                        let result = expand(&mut Parser::new(raw), vars, stack, depth + 1).ok();
                        stack.pop();
                        result
                    } else {
                        None
                    };
                    resolved
                        .or(fallback)
                        .ok_or_else(ParseError::unexpected_token)
                })?;
                out.push_str(&value);
            }
            Token::Function(f) => {
                let value = p.parse_nested_block(|p| expand(p, vars, stack, depth + 1))?;
                out.push_str(&f);
                out.push('(');
                out.push_str(&value);
                out.push(')');
            }
            Token::ParenthesisBlock => {
                let value = p.parse_nested_block(|p| expand(p, vars, stack, depth + 1))?;
                out.push('(');
                out.push_str(&value);
                out.push(')');
            }
            _ => out.push_str(p.slice_from(start)),
        }
        if out.len() > 1024 * 1024 {
            return Err(ParseError::unexpected_token());
        }
    }
    Ok(out)
}

pub fn resolve_variables(
    raw: &std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<String, String> {
    fn dependencies(p: &mut Parser<'_>, out: &mut Vec<String>, depth: usize) -> Result<(), Error> {
        if depth > 64 {
            return Err(ParseError::unexpected_token());
        }
        while !p.is_exhausted() {
            match p.next()?.clone() {
                Token::Function(name) => p.parse_nested_block(|p| {
                    if name.eq_ignore_ascii_case("var") {
                        out.push(p.expect_ident()?.to_string());
                        if !p.is_exhausted() {
                            p.expect_comma()?;
                        }
                    }
                    dependencies(p, out, depth + 1)
                })?,
                Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                    p.parse_nested_block(|p| dependencies(p, out, depth + 1))?
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn visit(
        key: &str,
        graph: &std::collections::BTreeMap<String, Vec<String>>,
        states: &mut std::collections::HashMap<String, u8>,
        path: &mut Vec<String>,
        invalid: &mut std::collections::HashSet<String>,
    ) {
        if states.get(key) == Some(&1) {
            if let Some(i) = path.iter().position(|k| k == key) {
                invalid.extend(path[i..].iter().cloned());
            }
            return;
        }
        if states.get(key) == Some(&2) {
            return;
        }
        if path.len() > 64 {
            invalid.insert(key.to_owned());
            return;
        }
        states.insert(key.to_owned(), 1);
        path.push(key.to_owned());
        if let Some(edges) = graph.get(key) {
            for edge in edges {
                if graph.contains_key(edge) {
                    visit(edge, graph, states, path, invalid);
                }
            }
        }
        path.pop();
        states.insert(key.to_owned(), 2);
    }
    let mut graph = std::collections::BTreeMap::new();
    let mut invalid = std::collections::HashSet::new();
    for (k, v) in raw {
        let mut deps = Vec::new();
        if dependencies(&mut Parser::new(v), &mut deps, 0).is_err() {
            invalid.insert(k.clone());
        }
        graph.insert(k.clone(), deps);
    }
    let mut states = std::collections::HashMap::new();
    for key in raw.keys() {
        visit(key, &graph, &mut states, &mut Vec::new(), &mut invalid);
    }
    let valid: std::collections::BTreeMap<_, _> = raw
        .iter()
        .filter(|(k, _)| !invalid.contains(*k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    valid
        .iter()
        .filter_map(|(k, v)| substitute(v, &valid).map(|v| (k.clone(), v)))
        .collect()
}

/// Substitute `attr()` (CSS Values 5 §7.7) using `attribute` to read the element's
/// attributes. `None` means a substitution failed with no fallback, which makes the
/// declaration invalid at computed-value time.
///
/// `attr(<name> <type>? [, <fallback>]?)` where `<type>` is `type(<syntax>)`,
/// `raw-string` or a dimension unit. Without a type the value is a CSS string. Values
/// containing `url()` or `image-set()` are never substituted (attr-taint).
pub fn substitute_attr(css: &str, attribute: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let lower = css.to_ascii_lowercase();
    if !lower.contains("attr(") {
        return Some(css.to_owned());
    }
    if lower.contains("url(") || lower.contains("image-set(") {
        return None;
    }
    let mut out = String::new();
    let mut rest = css;
    while let Some(at) = find_function(rest, "attr(") {
        out.push_str(&rest[..at]);
        let open = at + "attr(".len();
        let close = open + matching_paren(&rest[open..])?;
        out.push_str(&one_attr(&rest[open..close], attribute)?);
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// The start of `name` as a function, not as the tail of another identifier.
fn find_function(css: &str, name: &str) -> Option<usize> {
    let lower = css.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name).map(|i| i + from) {
        let before = lower[..i].chars().next_back();
        if !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Some(i);
        }
        from = i + name.len();
    }
    None
}

/// Index of the `)` closing an already-open parenthesis.
fn matching_paren(css: &str) -> Option<usize> {
    let mut depth = 1usize;
    for (i, c) in css.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn one_attr(args: &str, attribute: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    use cssparser::{Parser, Token};
    let (head, fallback) = match args.find(',') {
        Some(i) => (&args[..i], Some(args[i + 1..].trim())),
        None => (args, None),
    };
    let mut p = Parser::new(head);
    let name = p.expect_ident().ok()?.to_string();
    let mut syntax: Option<String> = None;
    let mut unit: Option<String> = None;
    match p.next().cloned() {
        Err(_) => {}
        Ok(Token::Function(f)) if f.eq_ignore_ascii_case("type") => {
            let inner = p
                .parse_nested_block(|p| {
                    let start = p.position();
                    while p.next().is_ok() {}
                    Ok::<_, cssparser::ParseError<()>>(p.slice_from(start).trim().to_owned())
                })
                .ok()?;
            syntax = Some(inner.to_ascii_lowercase());
        }
        Ok(Token::Ident(i)) if i.eq_ignore_ascii_case("raw-string") => {}
        Ok(Token::Ident(i)) => unit = Some(i.to_ascii_lowercase()),
        Ok(Token::Percentage { .. }) | Ok(Token::Delim('%')) => unit = Some("%".into()),
        _ => return None,
    }
    if !p.is_exhausted() {
        return None;
    }
    let typed = |raw: &str| -> Option<String> {
        let raw = raw.trim();
        if let Some(unit) = &unit {
            let n: f64 = raw.parse().ok().filter(|n: &f64| n.is_finite())?;
            return Some(format!("{n}{unit}"));
        }
        match syntax.as_deref() {
            None => Some(css_string(raw)),
            Some("*") => Some(raw.to_owned()),
            Some("<color>") => Color::parse(raw).map(|_| raw.to_owned()),
            Some("<number>") => raw.parse::<f64>().ok().map(|_| raw.to_owned()),
            Some("<integer>") => raw.parse::<i64>().ok().map(|_| raw.to_owned()),
            Some("<length>") => Length::parse(raw)
                .filter(|l| !matches!(l, Length::Auto | Length::Percent(_)))
                .map(|_| raw.to_owned()),
            Some("<percentage>") => {
                matches!(Length::parse(raw), Some(Length::Percent(_))).then(|| raw.to_owned())
            }
            Some("<length-percentage>") => Length::parse(raw)
                .filter(|l| *l != Length::Auto)
                .map(|_| raw.to_owned()),
            Some("<custom-ident>") => (!raw.is_empty()
                && raw
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_'))
            .then(|| raw.to_owned()),
            _ => None,
        }
    };
    attribute(&name)
        .and_then(|raw| typed(&raw))
        .or_else(|| fallback.map(str::to_owned))
}

/// A CSS string token for `value`.
fn css_string(value: &str) -> String {
    let mut s = String::from("\"");
    for c in value.chars() {
        match c {
            '"' | '\\' => {
                s.push('\\');
                s.push(c);
            }
            '\n' => s.push_str("\\a "),
            c if c.is_control() => s.push_str(&format!("\\{:x} ", c as u32)),
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

#[cfg(test)]
mod attr_tests {
    use super::substitute_attr;

    #[test]
    fn attr_substitutes_typed_values_and_fails_otherwise() {
        let attrs = |name: &str| match name {
            "data-color" => Some("333".to_owned()),
            "data-green" => Some("green".to_owned()),
            "data-n" => Some("12".to_owned()),
            "data-q" => Some("a\"b".to_owned()),
            _ => None,
        };
        let sub = |css: &str| substitute_attr(css, &attrs);
        assert_eq!(sub("attr(data-color type(<color>))"), None);
        assert_eq!(
            sub("attr(data-color type(<color>), red)").as_deref(),
            Some("red")
        );
        assert_eq!(
            sub("attr(data-green type(<color>))").as_deref(),
            Some("green")
        );
        assert_eq!(sub("attr(data-n px)").as_deref(), Some("12px"));
        assert_eq!(sub("attr(data-q)").as_deref(), Some("\"a\\\"b\""));
        assert_eq!(sub("attr(missing)"), None);
        assert_eq!(
            sub("1px solid attr(data-green type(<color>))").as_deref(),
            Some("1px solid green")
        );
        assert_eq!(sub("url(attr(data-q))"), None);
        assert_eq!(sub("no-attr-here").as_deref(), Some("no-attr-here"));
    }
}
