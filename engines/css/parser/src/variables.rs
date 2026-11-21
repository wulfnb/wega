//! Custom properties: `var()` substitution and cycle detection.
use cssparser::{ParseError, Parser, Token};
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
