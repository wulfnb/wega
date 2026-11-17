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
            input.parse_nested_block(consume)?;
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
                .collect(),
        })
    }
}
pub fn stylesheet(css: &str) -> StyleSheet {
    let mut parser = Parser::new(css);
    let mut rules = Rules {
        unsupported: Vec::new(),
    };
    let parsed = StyleSheetParser::new(&mut parser, &mut rules)
        .filter_map(Result::ok)
        .collect();
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
    expand(&mut Parser::new(css), vars, &mut Vec::new()).ok()
}
fn expand(
    p: &mut Parser<'_>,
    vars: &std::collections::BTreeMap<String, String>,
    stack: &mut Vec<String>,
) -> Result<String, Error> {
    if stack.len() > 32 {
        return Err(ParseError::unexpected_token());
    }
    let mut out = String::new();
    while !p.is_exhausted() {
        let start = p.position();
        let token = p.next_including_whitespace()?.clone();
        match token {
            Token::Function(f) if f.eq_ignore_ascii_case("var") => {
                let value = p.parse_nested_block(|p| {
                    let name = p.expect_ident()?.to_string();
                    if !name.starts_with("--") {
                        return Err(ParseError::unexpected_token());
                    }
                    let fallback = if !p.is_exhausted() {
                        p.expect_comma()?;
                        Some(expand(p, vars, stack)?)
                    } else {
                        None
                    };
                    let resolved = if stack.contains(&name) {
                        None
                    } else if let Some(raw) = vars.get(&name) {
                        stack.push(name);
                        let result = expand(&mut Parser::new(raw), vars, stack).ok();
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
                let value = p.parse_nested_block(|p| expand(p, vars, stack))?;
                out.push_str(&f);
                out.push('(');
                out.push_str(&value);
                out.push(')');
            }
            Token::ParenthesisBlock => {
                let value = p.parse_nested_block(|p| expand(p, vars, stack))?;
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
