//! At-rules and qualified rules: `@media`, `@supports`, `@import`, `@font-face`, `@layer` and nesting.
use crate::*;
use cssparser::{
    AtRuleParser, CowRcStr, ParseError, Parser, ParserState, QualifiedRuleParser, RuleBodyParser,
    StyleSheetParser, Token,
};
use wega_css_selectors::*;
type Error = ParseError<()>;

#[derive(Debug)]
pub(crate) enum AtRulePrelude {
    Media(Vec<MediaQuery>),
    FontFace,
    Import(ImportRule),
    /// `@layer a.b, c` names; an empty list is an anonymous layer block.
    Layer(Vec<String>),
    /// Whether an `@supports` condition holds.
    Supports(bool),
    /// `@property --name`.
    Property(String),
}

#[derive(Debug, Clone)]
pub(crate) enum ParsedRule {
    Style(Rule),
    Media { rules: Vec<Rule> },
    FontFace(FontFaceRule),
    Import(ImportRule),
}

pub(crate) fn push_token(s: &mut String, token: &Token) {
    match token {
        Token::Ident(id) => {
            s.push_str(id);
            s.push(' ');
        }
        Token::Colon => s.push(':'),
        Token::Comma => s.push(','),
        // Range comparisons (`width >= 48rem`) and ratios (`16/9`).
        Token::Delim(c) => s.push(*c),
        Token::Percentage { unit_value, .. } => {
            s.push_str(&format!("{}% ", unit_value * 100.0));
        }
        Token::Dimension { value, unit, .. } => {
            s.push_str(&format!("{value}{unit} "));
        }
        Token::Number { value, .. } => {
            s.push_str(&format!("{value} "));
        }
        Token::QuotedString(q) => {
            s.push('"');
            s.push_str(q);
            s.push('"');
            s.push(' ');
        }
        _ => {}
    }
}

pub(crate) fn parse_import_url(input: &mut Parser<'_>) -> Result<String, Error> {
    let token = input.next().map_err(|_| ParseError::unexpected_token())?;
    match token {
        Token::QuotedString(s) => Ok(s.to_string()),
        Token::UnquotedUrl(s) => Ok(s.to_string()),
        Token::Function(name) if name.eq_ignore_ascii_case("url") => {
            input.parse_nested_block(|p| {
                let t = p.next().map_err(|_| ParseError::unexpected_token())?;
                match t {
                    Token::QuotedString(s) | Token::UnquotedUrl(s) | Token::Ident(s) => {
                        Ok(s.to_string())
                    }
                    _ => Err(ParseError::unexpected_token()),
                }
            })
        }
        _ => Err(ParseError::unexpected_token()),
    }
}

pub(crate) fn unquote(s: &str) -> &str {
    let s = s.trim();
    if ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with("'") && s.ends_with("'")))
        && s.len() >= 2
    {
        return &s[1..s.len() - 1];
    }
    s
}

pub(crate) fn parse_font_face_rule(decls: Vec<Declaration>) -> FontFaceRule {
    let mut font_family = String::new();
    let mut src = Vec::new();
    let mut font_weight = None;
    let mut font_style = None;

    for d in decls {
        match d.property.to_ascii_lowercase().as_str() {
            "font-family" => {
                let name = unquote(d.value.trim());
                font_family = name.to_string();
            }
            "src" => {
                for part in d.value.split(',') {
                    let p = part.trim();
                    let mut url = String::new();
                    let mut format = None;
                    if let Some(start) = p.find("url(") {
                        let rest = &p[start + 4..];
                        if let Some(end) = rest.find(')') {
                            url = unquote(rest[..end].trim()).to_string();
                        }
                    }
                    if let Some(start) = p.find("format(") {
                        let rest = &p[start + 7..];
                        if let Some(end) = rest.find(')') {
                            format = Some(unquote(rest[..end].trim()).to_string());
                        }
                    }
                    if !url.is_empty() {
                        src.push(FontSourceSpec { url, format });
                    }
                }
            }
            "font-weight" => {
                font_weight = match d.value.trim().to_ascii_lowercase().as_str() {
                    "bold" => Some(700),
                    "normal" => Some(400),
                    other => other.parse::<u16>().ok(),
                };
            }
            "font-style" => {
                font_style = Some(d.value.trim().to_string());
            }
            _ => {}
        }
    }

    FontFaceRule {
        font_family,
        src,
        font_weight,
        font_style,
    }
}

pub(crate) struct Rules {
    pub(crate) unsupported: Vec<String>,
    depth: usize,
    /// Cascade layer names in order of first declaration.
    layers: Vec<String>,
    current_layer: Option<String>,
    anonymous_layers: usize,
    /// `@property` registrations: (name, initial value, inherits).
    pub(crate) registered: Vec<(String, String, bool)>,
}

impl Rules {
    pub(crate) fn new() -> Self {
        Self {
            unsupported: Vec::new(),
            depth: 0,
            layers: Vec::new(),
            current_layer: None,
            anonymous_layers: 0,
            registered: Vec::new(),
        }
    }
    fn layer_index(&mut self, name: &str) -> u32 {
        // A nested layer is declared after (and so ranks above) its parent.
        if let Some(i) = self.layers.iter().position(|l| l == name) {
            return i as u32;
        }
        self.layers.push(name.to_owned());
        (self.layers.len() - 1) as u32
    }
    fn qualified(&self, name: &str) -> String {
        match &self.current_layer {
            Some(parent) => format!("{parent}.{name}"),
            None => name.to_owned(),
        }
    }
    /// Parse a block of rules (inside `@media`, `@supports`, `@layer`).
    fn nested(
        &mut self,
        input: &mut Parser<'_>,
        layer: Option<String>,
        query: Option<&Vec<MediaQuery>>,
    ) -> Result<Vec<Rule>, Error> {
        if self.depth >= 32 {
            return Err(ParseError::unexpected_token());
        }
        let mut nested = Rules {
            unsupported: Vec::new(),
            depth: self.depth + 1,
            layers: std::mem::take(&mut self.layers),
            current_layer: layer.or_else(|| self.current_layer.clone()),
            anonymous_layers: self.anonymous_layers,
            registered: std::mem::take(&mut self.registered),
        };
        let parsed: Vec<ParsedRule> = StyleSheetParser::new(input, &mut nested)
            .filter_map(Result::ok)
            .collect();
        self.layers = nested.layers;
        self.registered = nested.registered;
        self.anonymous_layers = nested.anonymous_layers;
        self.unsupported.extend(nested.unsupported);
        let mut out = Vec::new();
        for p in parsed {
            match p {
                ParsedRule::Style(r) => out.push(r),
                ParsedRule::Media { rules } => out.extend(rules),
                _ => {}
            }
        }
        if let Some(q) = query {
            for r in &mut out {
                r.media.push(q.clone());
            }
        }
        Ok(out)
    }
}

/// Evaluate an `@supports` condition against what this engine implements.
///
/// `Err` is a syntax error, which drops the whole rule (CSS Conditional 3 §2.1):
/// leftover tokens, or `and` and `or` mixed without parentheses.
pub(crate) fn supports_condition(input: &mut Parser<'_>, depth: usize) -> Result<bool, ()> {
    if depth > 16 {
        return Err(());
    }
    if input.try_parse(|p| p.expect_ident_matching("not")).is_ok() {
        let result = !supports_in_parens(input, depth)?;
        return input.is_exhausted().then_some(result).ok_or(());
    }
    let mut result = supports_in_parens(input, depth)?;
    let mut combinator: Option<bool> = None;
    while !input.is_exhausted() {
        let is_and = match input.next() {
            Ok(Token::Ident(word)) if word.eq_ignore_ascii_case("and") => true,
            Ok(Token::Ident(word)) if word.eq_ignore_ascii_case("or") => false,
            _ => return Err(()),
        };
        if combinator.is_some_and(|c| c != is_and) {
            return Err(());
        }
        combinator = Some(is_and);
        let next = supports_in_parens(input, depth)?;
        result = if is_and {
            result && next
        } else {
            result || next
        };
    }
    Ok(result)
}

pub(crate) fn supports_in_parens(input: &mut Parser<'_>, depth: usize) -> Result<bool, ()> {
    match input.next().cloned() {
        Ok(Token::ParenthesisBlock) => input
            .parse_nested_block(|p| {
                let state = p.state();
                // A declaration test `(property: value)`.
                if let Ok(name) = p.expect_ident().map(|n| n.to_string())
                    && p.expect_colon().is_ok()
                {
                    let start = p.position();
                    let mut well_formed = true;
                    while let Ok(token) = p.next() {
                        well_formed &= !token.is_parse_error();
                    }
                    let value = p.slice_from(start).trim();
                    // A declaration test may end in `!important` (CSS Conditional 3 §2.2).
                    let value = match value.rfind('!') {
                        Some(bang)
                            if value[bang + 1..].trim().eq_ignore_ascii_case("important") =>
                        {
                            value[..bang].trim()
                        }
                        _ => value,
                    }
                    .to_owned();
                    let ok = well_formed && !value.is_empty() && {
                        if name.starts_with("--") {
                            true
                        } else {
                            let name = name.to_ascii_lowercase();
                            supported_property(&name)
                                && !value.contains("from ")
                                && PropertyDeclaration::parse(&name, &value) != Parsed::Invalid
                        }
                    };
                    return Ok::<bool, Error>(ok);
                }
                p.reset(&state);
                // Anything else in parentheses is `<general-enclosed>`: false.
                Ok(p.parse_entirely(|p| {
                    supports_condition(p, depth + 1).map_err(|_| p.new_error_for_next_token::<()>())
                })
                .unwrap_or(false))
            })
            .map_err(|_| ()),
        Ok(Token::Function(f)) if f.eq_ignore_ascii_case("selector") => input
            .parse_nested_block(|p| {
                let start = p.position();
                while p.next().is_ok() {}
                Ok::<bool, Error>(!selectors(p.slice_from(start)).is_empty())
            })
            .map_err(|_| ()),
        // Other functions are `<general-enclosed>`: false.
        Ok(Token::Function(_)) => input
            .parse_nested_block(|p| {
                while p.next().is_ok() {}
                Ok::<bool, Error>(false)
            })
            .map_err(|_| ()),
        _ => Err(()),
    }
}

impl<'i> AtRuleParser<'i> for Rules {
    type Prelude = AtRulePrelude;
    type AtRule = ParsedRule;
    type Error = ();

    fn parse_prelude(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
    ) -> Result<Self::Prelude, Error> {
        match name.to_ascii_lowercase().as_str() {
            "media" => {
                let mut s = String::new();
                while let Ok(token) = input.next() {
                    match token {
                        Token::ParenthesisBlock => {
                            s.push('(');
                            input.parse_nested_block(|p| {
                                while let Ok(t) = p.next() {
                                    push_token(&mut s, t);
                                }
                                Ok::<(), ParseError<()>>(())
                            })?;
                            s.push(')');
                        }
                        other => push_token(&mut s, other),
                    }
                }
                let queries = parse_media_query_list(&s);
                Ok(AtRulePrelude::Media(queries))
            }
            "font-face" => Ok(AtRulePrelude::FontFace),
            "layer" => {
                let mut names = Vec::new();
                let mut current = String::new();
                while let Ok(token) = input.next() {
                    match token {
                        Token::Ident(i) => current.push_str(i),
                        Token::Delim('.') => current.push('.'),
                        Token::Comma => names.push(std::mem::take(&mut current)),
                        _ => return Err(ParseError::unexpected_token()),
                    }
                }
                if !current.is_empty() {
                    names.push(current);
                }
                Ok(AtRulePrelude::Layer(names))
            }
            "supports" => supports_condition(input, 0)
                .map(AtRulePrelude::Supports)
                .map_err(|_| input.new_error_for_next_token()),
            "property" => {
                let name = input.expect_ident()?.to_string();
                if !name.starts_with("--") {
                    return Err(ParseError::unexpected_token());
                }
                Ok(AtRulePrelude::Property(name))
            }
            "import" => {
                let url = parse_import_url(input)?;
                let mut media_s = String::new();
                while let Ok(token) = input.next() {
                    match token {
                        Token::ParenthesisBlock => {
                            media_s.push('(');
                            input.parse_nested_block(|p| {
                                while let Ok(t) = p.next() {
                                    push_token(&mut media_s, t);
                                }
                                Ok::<(), ParseError<()>>(())
                            })?;
                            media_s.push(')');
                        }
                        other => push_token(&mut media_s, other),
                    }
                }
                let media = if media_s.trim().is_empty() {
                    Vec::new()
                } else {
                    parse_media_query_list(&media_s)
                };
                Ok(AtRulePrelude::Import(ImportRule { url, media }))
            }
            _ => {
                self.unsupported.push(format!("@{name}"));
                Err(ParseError::unexpected_token())
            }
        }
    }

    fn parse_block(
        &mut self,
        prelude: Self::Prelude,
        _: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<Self::AtRule, Error> {
        match prelude {
            AtRulePrelude::Media(queries) => Ok(ParsedRule::Media {
                rules: self.nested(input, None, Some(&queries))?,
            }),
            AtRulePrelude::Property(name) => {
                let decls: Vec<Declaration> = RuleBodyParser::new(input, &mut Declarations)
                    .filter_map(Result::ok)
                    .collect();
                let initial = decls
                    .iter()
                    .find(|d| d.property == "initial-value")
                    .map(|d| d.value.trim().to_owned());
                let inherits = decls
                    .iter()
                    .find(|d| d.property == "inherits")
                    .is_some_and(|d| d.value.trim().eq_ignore_ascii_case("true"));
                if let Some(initial) = initial
                    && self.registered.len() < 1024
                {
                    self.registered.push((name, initial, inherits));
                }
                Ok(ParsedRule::Media { rules: Vec::new() })
            }
            AtRulePrelude::Supports(holds) => {
                let rules = self.nested(input, None, None)?;
                Ok(ParsedRule::Media {
                    rules: if holds { rules } else { Vec::new() },
                })
            }
            AtRulePrelude::Layer(names) => {
                if names.len() > 1 {
                    return Err(ParseError::unexpected_token());
                }
                let name = match names.into_iter().next() {
                    Some(n) => self.qualified(&n),
                    None => {
                        self.anonymous_layers += 1;
                        self.qualified(&format!("\u{0}anonymous{}", self.anonymous_layers))
                    }
                };
                // Register parents first so a nested layer ranks above them.
                let mut prefix = String::new();
                for part in name.split('.') {
                    if !prefix.is_empty() {
                        prefix.push('.');
                    }
                    prefix.push_str(part);
                    self.layer_index(&prefix);
                }
                let index = self.layer_index(&name);
                let mut rules = self.nested(input, Some(name), None)?;
                for r in &mut rules {
                    r.layer.get_or_insert(index);
                }
                Ok(ParsedRule::Media { rules })
            }
            AtRulePrelude::FontFace => {
                let decls: Vec<Declaration> = RuleBodyParser::new(input, &mut Declarations)
                    .filter_map(Result::ok)
                    .collect();
                Ok(ParsedRule::FontFace(parse_font_face_rule(decls)))
            }
            _ => Err(ParseError::unexpected_token()),
        }
    }

    fn rule_without_block(
        &mut self,
        prelude: Self::Prelude,
        _: &ParserState,
    ) -> Result<Self::AtRule, ()> {
        match prelude {
            AtRulePrelude::Import(imp) => Ok(ParsedRule::Import(imp)),
            AtRulePrelude::Layer(names) => {
                // `@layer a, b;` fixes the layers' order.
                for name in names {
                    let name = self.qualified(&name);
                    self.layer_index(&name);
                }
                Ok(ParsedRule::Media { rules: Vec::new() })
            }
            _ => Err(()),
        }
    }
}

impl<'i> QualifiedRuleParser<'i> for Rules {
    type Prelude = Vec<ComplexSelector>;
    type QualifiedRule = ParsedRule;
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
    ) -> Result<Self::QualifiedRule, Error> {
        Ok(ParsedRule::Style(Rule {
            selectors,
            declarations: RuleBodyParser::new(input, &mut Declarations)
                .filter_map(Result::ok)
                .flat_map(expand_shorthand)
                .collect(),
            media: Vec::new(),
            layer: self
                .current_layer
                .clone()
                .map(|name| self.layer_index(&name)),
        }))
    }
}

#[cfg(test)]
mod supports_tests {
    fn applies(condition: &str) -> bool {
        !crate::stylesheet(&format!("@supports {condition} {{ p {{ color: red }} }}"))
            .rules
            .is_empty()
    }

    #[test]
    fn syntax_errors_drop_the_rule() {
        for invalid in [
            "(margin: 2px) )",
            "(color: green) and (color: green) or (color: green)",
            "(color: green) or (color: green) and (color: green)",
            "(color: green) or(color: blue)",
            "not (color: green) and (color: green)",
            "(margin: 0])",
            "(margin-top: wide)",
        ] {
            assert!(!applies(invalid), "{invalid}");
        }
        for valid in [
            "(margin: 0)",
            "(color: green !important)",
            "((margin: 0) and (display:inline-block !important))",
            "(color: green) and (color: green)",
            "((color: green) or (color: red)) and (color: green)",
            "not (unknown-property: 1)",
            "not (foo bar)",
            "(--custom: anything)",
        ] {
            assert!(applies(valid), "{valid}");
        }
    }
}
