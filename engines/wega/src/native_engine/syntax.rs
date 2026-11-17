//! Token-based CSS syntax with cssparser. Unsupported selectors reject their
//! rule rather than broadening it (e.g. accidentally applying :hover always).
use super::css::*;
use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, Parser, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, StyleSheetParser, Token,
};
type Error = ParseError<()>;

struct Rules {
    unsupported: Vec<String>,
}
impl<'i> AtRuleParser<'i> for Rules {
    type Prelude = ();
    type AtRule = Rule;
    type Error = ();
    fn parse_prelude(&mut self, name: CowRcStr<'i>, _: &mut Parser<'i>) -> Result<(), Error> {
        self.unsupported.push(format!("@{name}"));
        Err(ParseError::unexpected_token())
    }
}
fn consume(input: &mut Parser<'_>) -> Result<(), Error> {
    consume_at(input, 0)
}
fn consume_at(input: &mut Parser<'_>, depth: usize) -> Result<(), Error> {
    if depth > 64 {
        return Err(ParseError::unexpected_token());
    }
    while !input.is_exhausted() {
        let token = input.next()?.clone();
        if token.is_parse_error() {
            return Err(ParseError::unexpected_token());
        }
        if matches!(
            token,
            Token::Function(_)
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::CurlyBracketBlock
        ) {
            input.parse_nested_block(|p| consume_at(p, depth + 1))?;
        }
    }
    Ok(())
}
impl<'i> QualifiedRuleParser<'i> for Rules {
    type Prelude = Vec<ComplexSelector>;
    type QualifiedRule = Rule;
    type Error = ();
    fn parse_prelude(&mut self, input: &mut Parser<'i>) -> Result<Self::Prelude, Error> {
        match selector_list(input, 0) {
            Ok(s) => Ok(s),
            Err(e) => {
                self.unsupported.push("selector".into());
                Err(e)
            }
        }
    }
    fn parse_block(
        &mut self,
        selectors: Self::Prelude,
        _: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<Rule, Error> {
        Ok(Rule {
            selectors,
            declarations: RuleBodyParser::new(input, &mut Declarations)
                .filter_map(Result::ok)
                .flat_map(expand_shorthand)
                .collect(),
        })
    }
}
pub fn stylesheet(css: &str) -> StyleSheet {
    if css.len() > 8 * 1024 * 1024 {
        return StyleSheet {
            rules: Vec::new(),
            unsupported: vec!["stylesheet byte limit".into()],
        };
    }
    let mut parser = Parser::new(css);
    let mut rules = Rules {
        unsupported: Vec::new(),
    };
    let parsed: Vec<Rule> = StyleSheetParser::new(&mut parser, &mut rules)
        .filter_map(Result::ok)
        .collect();
    for rule in &parsed {
        for decl in &rule.declarations {
            if matches!(decl.property.as_str(), "font-size" | "line-height")
                && (decl.value.contains("vw") || decl.value.contains("vh"))
                && rules.unsupported.len() < 256
            {
                rules
                    .unsupported
                    .push(format!("viewport-relative typography:{}", decl.property));
            }
            if !super::css::supported_property(&decl.property) && rules.unsupported.len() < 256 {
                rules
                    .unsupported
                    .push(format!("property:{}", decl.property));
            }
        }
    }
    rules.unsupported.sort();
    rules.unsupported.dedup();
    StyleSheet {
        rules: parsed,
        unsupported: rules.unsupported,
    }
}
struct Declarations;
impl<'i> DeclarationParser<'i> for Declarations {
    type Declaration = Declaration;
    type Error = ();
    fn parse_value(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
        _: &ParserState,
    ) -> Result<Declaration, Error> {
        let start = input.position();
        let mut end = start;
        let mut important = false;
        while !input.is_exhausted() {
            let state = input.state();
            if input.try_parse(cssparser::parse_important).is_ok() {
                input.expect_exhausted()?;
                important = true;
                break;
            }
            input.reset(&state);
            let token = input.next()?.clone();
            if token.is_parse_error() {
                return Err(ParseError::unexpected_token());
            }
            if matches!(
                token,
                Token::Function(_)
                    | Token::ParenthesisBlock
                    | Token::SquareBracketBlock
                    | Token::CurlyBracketBlock
            ) {
                input.parse_nested_block(consume)?;
            }
            end = input.position();
        }
        let property = if name.starts_with("--") {
            name.to_string()
        } else {
            name.to_ascii_lowercase()
        };
        let value = input.slice(start..end).trim().to_owned();
        if value.is_empty() {
            return Err(ParseError::unexpected_token());
        }
        Ok(Declaration {
            property,
            value,
            important,
        })
    }
}
impl<'i> AtRuleParser<'i> for Declarations {
    type Prelude = ();
    type AtRule = Declaration;
    type Error = ();
}
impl<'i> QualifiedRuleParser<'i> for Declarations {
    type Prelude = ();
    type QualifiedRule = Declaration;
    type Error = ();
}
impl<'i> RuleBodyItemParser<'i, Declaration, ()> for Declarations {
    fn parse_declarations(&self) -> bool {
        true
    }
    fn parse_qualified(&self) -> bool {
        false
    }
}
pub fn declarations(css: &str) -> Vec<Declaration> {
    let mut parser = Parser::new(css);
    RuleBodyParser::new(&mut parser, &mut Declarations)
        .filter_map(Result::ok)
        .flat_map(expand_shorthand)
        .collect()
}
pub fn selectors(css: &str) -> Vec<ComplexSelector> {
    let mut parser = Parser::new(css);
    selector_list(&mut parser, 0).unwrap_or_default()
}
fn selector_list(input: &mut Parser<'_>, depth: usize) -> Result<Vec<ComplexSelector>, Error> {
    if depth > 32 {
        return Err(ParseError::unexpected_token());
    }
    input.parse_comma_separated(|p| complex(p, depth))
}
fn complex(input: &mut Parser<'_>, depth: usize) -> Result<ComplexSelector, Error> {
    let mut items: Vec<(CompoundSelector, Option<Combinator>)> = Vec::new();
    let mut components = Vec::new();
    let mut spaced = false;
    while !input.is_exhausted() {
        let token = input.next_including_whitespace()?.clone();
        if matches!(token, Token::WhiteSpace(_)) {
            spaced = true;
            continue;
        }
        let combinator = match token {
            Token::Delim('>') => Some(Combinator::Child),
            Token::Delim('+') => Some(Combinator::Adjacent),
            Token::Delim('~') => Some(Combinator::Sibling),
            _ => None,
        };
        if let Some(c) = combinator {
            if !components.is_empty() {
                items.push((
                    CompoundSelector {
                        components: std::mem::take(&mut components),
                    },
                    Some(c),
                ));
            } else if let Some(last) = items.last_mut() {
                if last.1 != Some(Combinator::Descendant) {
                    return Err(ParseError::unexpected_token());
                }
                last.1 = Some(c);
            } else {
                return Err(ParseError::unexpected_token());
            }
            spaced = false;
            continue;
        }
        if spaced && !components.is_empty() {
            items.push((
                CompoundSelector {
                    components: std::mem::take(&mut components),
                },
                Some(Combinator::Descendant),
            ));
        }
        spaced = false;
        let simple = match token {
            Token::Delim('*') => SimpleSelector::Universal,
            Token::Ident(s) => SimpleSelector::Type(s.to_ascii_lowercase()),
            Token::IDHash(s) => SimpleSelector::Id(s.to_string()),
            Token::Delim('.') => SimpleSelector::Class(input.expect_ident()?.to_string()),
            Token::SquareBracketBlock => input.parse_nested_block(attribute)?,
            Token::Colon => match input.next()?.clone() {
                Token::Ident(s) => match s.to_ascii_lowercase().as_str() {
                    "root" => SimpleSelector::Root,
                    "empty" => SimpleSelector::Empty,
                    "first-child" => SimpleSelector::FirstChild,
                    "last-child" => SimpleSelector::LastChild,
                    "only-child" => SimpleSelector::OnlyChild,
                    _ => return Err(ParseError::unexpected_token()),
                },
                Token::Function(s) => input.parse_nested_block(|p| {
                    Ok(match s.to_ascii_lowercase().as_str() {
                        "is" => SimpleSelector::Is(selector_list(p, depth + 1)?),
                        "where" => SimpleSelector::Where(selector_list(p, depth + 1)?),
                        "not" => SimpleSelector::Not(selector_list(p, depth + 1)?),
                        "nth-child" | "nth-last-child" => {
                            let (a, b) = cssparser::parse_nth(p)?;
                            SimpleSelector::NthChild(a, b, s.eq_ignore_ascii_case("nth-last-child"))
                        }
                        _ => return Err(ParseError::unexpected_token()),
                    })
                })?,
                _ => return Err(ParseError::unexpected_token()),
            },
            _ => return Err(ParseError::unexpected_token()),
        };
        components.push(simple);
        if items.len() > 128 {
            return Err(ParseError::unexpected_token());
        }
    }
    if components.is_empty() {
        return Err(ParseError::unexpected_token());
    }
    items.push((CompoundSelector { components }, None));
    Ok(ComplexSelector { items })
}
fn attribute(input: &mut Parser<'_>) -> Result<SimpleSelector, Error> {
    let name = input.expect_ident()?.to_ascii_lowercase();
    if input.is_exhausted() {
        return Ok(SimpleSelector::AttrPresent(name));
    }
    let op = match input.next()? {
        Token::Delim('=') => '=',
        Token::IncludeMatch => '~',
        Token::DashMatch => '|',
        Token::PrefixMatch => '^',
        Token::SuffixMatch => '$',
        Token::SubstringMatch => '*',
        _ => return Err(ParseError::unexpected_token()),
    };
    let value = input.expect_ident_or_string()?.to_string();
    let insensitive = if input.is_exhausted() {
        false
    } else {
        let flag = input.expect_ident()?;
        if flag.eq_ignore_ascii_case("i") {
            true
        } else if flag.eq_ignore_ascii_case("s") {
            false
        } else {
            return Err(ParseError::unexpected_token());
        }
    };
    Ok(SimpleSelector::AttrMatch(name, value, op, insensitive))
}

// Length expressions retain dimensional terms until the containing block is known.
pub fn calc_length(css: &str) -> Option<Length> {
    let mut p = Parser::new(css);
    let result = p
        .parse_entirely(|p| {
            p.expect_function_matching("calc")?;
            p.parse_nested_block(|p| expression(p, 0))
        })
        .ok()?;
    match result {
        Quantity::Length(v) => Some(Length::Calc(v)),
        Quantity::Number(0.0) => Some(Length::Px(0.0)),
        _ => None,
    }
}
#[derive(Clone, Copy)]
enum Quantity {
    Number(f64),
    Length([f64; 4]),
}
fn expression(p: &mut Parser<'_>, depth: usize) -> Result<Quantity, Error> {
    let mut a = product(p, depth)?;
    while !p.is_exhausted() {
        let op = match p.next()? {
            Token::Delim('+') => 1.0,
            Token::Delim('-') => -1.0,
            _ => return Err(ParseError::unexpected_token()),
        };
        let b = product(p, depth)?;
        a = match (a, b) {
            (Quantity::Number(a), Quantity::Number(b)) => Quantity::Number(a + op * b),
            (Quantity::Length(mut a), Quantity::Length(b)) => {
                for i in 0..4 {
                    a[i] += op * b[i];
                }
                Quantity::Length(a)
            }
            _ => return Err(ParseError::unexpected_token()),
        };
    }
    Ok(a)
}
fn product(p: &mut Parser<'_>, depth: usize) -> Result<Quantity, Error> {
    let mut a = atom(p, depth)?;
    loop {
        let state = p.state();
        let op = match p.next() {
            Ok(Token::Delim('*')) => '*',
            Ok(Token::Delim('/')) => '/',
            _ => {
                p.reset(&state);
                break;
            }
        };
        let b = atom(p, depth)?;
        let (quantity, scale) = match (a, b, op) {
            (a, Quantity::Number(b), '*') => (a, b),
            (Quantity::Number(a), b, '*') => (b, a),
            (a, Quantity::Number(b), '/') if b != 0.0 => (a, 1.0 / b),
            _ => return Err(ParseError::unexpected_token()),
        };
        a = match quantity {
            Quantity::Number(v) => Quantity::Number(v * scale),
            Quantity::Length(mut v) => {
                for x in &mut v {
                    *x *= scale;
                }
                Quantity::Length(v)
            }
        };
    }
    Ok(a)
}
fn atom(p: &mut Parser<'_>, depth: usize) -> Result<Quantity, Error> {
    if depth > 32 {
        return Err(ParseError::unexpected_token());
    }
    let mut terms = [0.0; 4];
    match p.next()?.clone() {
        Token::Number { value, .. } if value.is_finite() => {
            return Ok(Quantity::Number(value as f64));
        }
        Token::Percentage { unit_value, .. } if unit_value.is_finite() => {
            terms[1] = unit_value as f64 * 100.0
        }
        Token::Dimension { value, unit, .. } if value.is_finite() => {
            let i = match unit.to_ascii_lowercase().as_str() {
                "px" => 0,
                "em" => 2,
                "rem" => 3,
                _ => return Err(ParseError::unexpected_token()),
            };
            terms[i] = value as f64;
        }
        Token::ParenthesisBlock => return p.parse_nested_block(|p| expression(p, depth + 1)),
        Token::Function(f) if f.eq_ignore_ascii_case("calc") => {
            return p.parse_nested_block(|p| expression(p, depth + 1));
        }
        _ => return Err(ParseError::unexpected_token()),
    }
    Ok(Quantity::Length(terms))
}

pub fn substitute(css: &str, vars: &std::collections::BTreeMap<String, String>) -> Option<String> {
    expand(&mut Parser::new(css), vars, &mut Vec::new(), 0).ok()
}
fn expand(
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

pub fn tracks(css: &str) -> Option<Vec<GridTrack>> {
    if css.trim() == "none" {
        return Some(Vec::new());
    }
    track_list(&mut Parser::new(css), 0).ok()
}
fn track_list(p: &mut Parser<'_>, depth: usize) -> Result<Vec<GridTrack>, Error> {
    if depth > 16 {
        return Err(ParseError::unexpected_token());
    }
    let mut out = Vec::new();
    while !p.is_exhausted() {
        let start = p.position();
        match p.next()?.clone() {
            Token::Function(f) if f.eq_ignore_ascii_case("minmax") => {
                out.push(p.parse_nested_block(|p| {
                    let start = p.position();
                    p.parse_until_before(cssparser::Delimiter::Comma, consume)?;
                    let min = Length::parse(p.slice_from(start).trim())
                        .ok_or_else(ParseError::unexpected_token)?;
                    p.expect_comma()?;
                    let mut max = track_list(p, depth + 1)?;
                    if max.len() != 1 {
                        return Err(ParseError::unexpected_token());
                    }
                    Ok(GridTrack::MinMax(min, Box::new(max.remove(0))))
                })?)
            }
            Token::Function(f) if f.eq_ignore_ascii_case("repeat") => {
                let repeated = p.parse_nested_block(|p| {
                    let count = p.expect_integer()?;
                    if !(1..=256).contains(&count) {
                        return Err(ParseError::unexpected_token());
                    }
                    p.expect_comma()?;
                    let items = track_list(p, depth + 1)?;
                    if items.len() * count as usize > 256 {
                        return Err(ParseError::unexpected_token());
                    }
                    Ok((0..count).flat_map(|_| items.clone()).collect::<Vec<_>>())
                })?;
                out.extend(repeated);
            }
            Token::Dimension { value, unit, .. }
                if unit.eq_ignore_ascii_case("fr") && value >= 0.0 && value.is_finite() =>
            {
                out.push(GridTrack::Fraction(value as f64))
            }
            _ => out.push(GridTrack::Length(
                Length::parse(p.slice_from(start).trim())
                    .ok_or_else(ParseError::unexpected_token)?,
            )),
        }
        if out.len() > 256 {
            return Err(ParseError::unexpected_token());
        }
    }
    if out.is_empty() {
        return Err(ParseError::unexpected_token());
    }
    Ok(out)
}

fn expand_shorthand(decl: Declaration) -> Vec<Declaration> {
    let properties = if decl.property == "font" {
        font_shorthand(&decl.value)
    } else if decl.property == "flex" {
        flex_shorthand(&decl.value)
    } else {
        return vec![decl];
    };
    properties
        .unwrap_or_default()
        .into_iter()
        .map(|(p, v)| Declaration {
            property: p.to_owned(),
            value: v,
            important: decl.important,
        })
        .collect()
}
fn font_shorthand(css: &str) -> Option<Vec<(&'static str, String)>> {
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
fn flex_shorthand(css: &str) -> Option<Vec<(&'static str, String)>> {
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
