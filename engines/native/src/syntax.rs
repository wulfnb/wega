//! Token-based CSS syntax with cssparser. Unsupported selectors reject their
//! rule rather than broadening it (e.g. accidentally applying :hover always).
use super::css::*;
use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, Parser, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, StyleSheetParser, Token,
};
type Error = ParseError<()>;

#[derive(Debug)]
enum AtRulePrelude {
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
enum ParsedRule {
    Style(Rule),
    Media { rules: Vec<Rule> },
    FontFace(FontFaceRule),
    Import(ImportRule),
}

fn push_token(s: &mut String, token: &Token) {
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

fn parse_import_url(input: &mut Parser<'_>) -> Result<String, Error> {
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

fn unquote(s: &str) -> &str {
    let s = s.trim();
    if ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with("'") && s.ends_with("'")))
        && s.len() >= 2
    {
        return &s[1..s.len() - 1];
    }
    s
}
fn parse_font_face_rule(decls: Vec<Declaration>) -> FontFaceRule {
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

struct Rules {
    unsupported: Vec<String>,
    depth: usize,
    /// Cascade layer names in order of first declaration.
    layers: Vec<String>,
    current_layer: Option<String>,
    anonymous_layers: usize,
    /// `@property` registrations: (name, initial value, inherits).
    registered: Vec<(String, String, bool)>,
}
impl Rules {
    fn new() -> Self {
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
fn supports_condition(input: &mut Parser<'_>, depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    if input.try_parse(|p| p.expect_ident_matching("not")).is_ok() {
        return !supports_in_parens(input, depth);
    }
    let mut result = supports_in_parens(input, depth);
    loop {
        if input.try_parse(|p| p.expect_ident_matching("and")).is_ok() {
            let next = supports_in_parens(input, depth);
            result = result && next;
        } else if input.try_parse(|p| p.expect_ident_matching("or")).is_ok() {
            let next = supports_in_parens(input, depth);
            result = result || next;
        } else {
            break;
        }
    }
    while input.next().is_ok() {}
    result
}
fn supports_in_parens(input: &mut Parser<'_>, depth: usize) -> bool {
    match input.next().cloned() {
        Ok(Token::ParenthesisBlock) => input
            .parse_nested_block(|p| {
                let state = p.state();
                // A declaration test `(property: value)`.
                if let Ok(name) = p.expect_ident().map(|n| n.to_ascii_lowercase())
                    && p.expect_colon().is_ok()
                {
                    let start = p.position();
                    while p.next().is_ok() {}
                    let value = p.slice_from(start).trim().to_owned();
                    let ok = !value.is_empty()
                        && (name.starts_with("--")
                            || (super::css::supported_property(&name) && !value.contains("from ")));
                    return Ok::<bool, Error>(ok);
                }
                p.reset(&state);
                Ok(supports_condition(p, depth + 1))
            })
            .unwrap_or(false),
        Ok(Token::Function(f)) if f.eq_ignore_ascii_case("selector") => input
            .parse_nested_block(|p| {
                let start = p.position();
                while p.next().is_ok() {}
                Ok::<bool, Error>(!selectors(p.slice_from(start)).is_empty())
            })
            .unwrap_or(false),
        Ok(Token::Function(_)) => {
            let _ = input.parse_nested_block(|p| {
                while p.next().is_ok() {}
                Ok::<(), Error>(())
            });
            false
        }
        _ => false,
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
            "supports" => Ok(AtRulePrelude::Supports(supports_condition(input, 0))),
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
pub fn stylesheet(css: &str) -> StyleSheet {
    if css.len() > 8 * 1024 * 1024 {
        return StyleSheet {
            unsupported: vec!["stylesheet byte limit".into()],
            ..Default::default()
        };
    }
    let mut parser = Parser::new(css);
    let mut rules = Rules::new();
    let parsed: Vec<ParsedRule> = StyleSheetParser::new(&mut parser, &mut rules)
        .filter_map(Result::ok)
        .collect();

    let mut sheet_rules = Vec::new();
    let mut font_faces = Vec::new();
    let mut imports = Vec::new();

    for p in parsed {
        match p {
            ParsedRule::Style(r) => sheet_rules.push(r),
            ParsedRule::Media { rules, .. } => sheet_rules.extend(rules),
            ParsedRule::FontFace(f) => font_faces.push(f),
            ParsedRule::Import(i) => imports.push(i),
        }
    }

    for rule in &sheet_rules {
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
        rules: sheet_rules,
        font_faces,
        imports,
        unsupported: rules.unsupported,
        registered_properties: rules.registered,
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
            Token::Colon => pseudo(input, depth)?,
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
fn pseudo(input: &mut Parser<'_>, depth: usize) -> Result<SimpleSelector, Error> {
    // `::name` and the legacy single-colon pseudo-elements select generated
    // content, never the element itself. Keep them so the rest of the rule's
    // selector list survives instead of invalidating the whole rule.
    let element = |name: &str| SimpleSelector::PseudoElement(name.to_ascii_lowercase());
    let token = input.next_including_whitespace()?.clone();
    Ok(match token {
        Token::Colon => match input.next_including_whitespace()?.clone() {
            Token::Ident(s) => element(&s),
            Token::Function(s) => {
                input.parse_nested_block(|p| {
                    while p.next().is_ok() {}
                    Ok::<_, Error>(())
                })?;
                element(&s)
            }
            _ => return Err(ParseError::unexpected_token()),
        },
        Token::Ident(s) => match s.to_ascii_lowercase().as_str() {
            "root" | "scope" => SimpleSelector::Root,
            "empty" => SimpleSelector::Empty,
            "hover" => SimpleSelector::Hover,
            "focus" | "focus-visible" => SimpleSelector::Focus,
            "focus-within" => SimpleSelector::FocusWithin,
            "active" => SimpleSelector::Active,
            "checked" => SimpleSelector::Checked,
            "disabled" => SimpleSelector::Disabled,
            "enabled" => SimpleSelector::Enabled,
            "link" | "any-link" => SimpleSelector::Link,
            // History is not exposed to style; Chrome reports links unvisited.
            "visited" | "target" | "target-within" | "indeterminate" | "invalid"
            | "user-invalid" | "fullscreen" | "modal" | "popover-open" | "autofill"
            | "-webkit-autofill" | "host" => SimpleSelector::Never,
            "defined" | "valid" | "user-valid" | "in-range" => SimpleSelector::Universal,
            "required" => SimpleSelector::AttrPresent("required".into()),
            "optional" => SimpleSelector::Optional,
            "read-only" => SimpleSelector::ReadOnly(true),
            "read-write" => SimpleSelector::ReadOnly(false),
            "placeholder-shown" => SimpleSelector::PlaceholderShown,
            "first-child" => SimpleSelector::FirstChild,
            "last-child" => SimpleSelector::LastChild,
            "only-child" => SimpleSelector::OnlyChild,
            "first-of-type" => SimpleSelector::NthOfType(0, 1, false),
            "last-of-type" => SimpleSelector::NthOfType(0, 1, true),
            "only-of-type" => SimpleSelector::Is(vec![ComplexSelector {
                items: vec![(
                    CompoundSelector {
                        components: vec![
                            SimpleSelector::NthOfType(0, 1, false),
                            SimpleSelector::NthOfType(0, 1, true),
                        ],
                    },
                    None,
                )],
            }]),
            "before" | "after" | "first-line" | "first-letter" => element(&s),
            _ => return Err(ParseError::unexpected_token()),
        },
        Token::Function(s) => input.parse_nested_block(|p| {
            Ok(match s.to_ascii_lowercase().as_str() {
                "is" | "matches" | "-webkit-any" => {
                    SimpleSelector::Is(forgiving_list(p, depth + 1))
                }
                "where" => SimpleSelector::Where(forgiving_list(p, depth + 1)),
                "not" => SimpleSelector::Not(selector_list(p, depth + 1)?),
                "has" => SimpleSelector::Has(p.parse_comma_separated(|p| relative(p, depth + 1))?),
                "nth-child" | "nth-last-child" => {
                    let (a, b) = cssparser::parse_nth(p)?;
                    if !p.is_exhausted() {
                        // `of S` filters are not supported; refuse rather than guess.
                        return Err(ParseError::unexpected_token());
                    }
                    SimpleSelector::NthChild(a, b, s.eq_ignore_ascii_case("nth-last-child"))
                }
                "nth-of-type" | "nth-last-of-type" => {
                    let (a, b) = cssparser::parse_nth(p)?;
                    SimpleSelector::NthOfType(a, b, s.eq_ignore_ascii_case("nth-last-of-type"))
                }
                "lang" => {
                    let tag = p.expect_ident_or_string()?.to_ascii_lowercase();
                    SimpleSelector::Lang(tag)
                }
                "dir" => {
                    p.expect_ident()?;
                    SimpleSelector::Universal
                }
                // Shadow-tree selectors never match light-DOM elements.
                "host" | "host-context" => {
                    while p.next().is_ok() {}
                    SimpleSelector::Never
                }
                _ => return Err(ParseError::unexpected_token()),
            })
        })?,
        _ => return Err(ParseError::unexpected_token()),
    })
}
/// `:is()` and `:where()` take forgiving selector lists: invalid entries are
/// dropped rather than invalidating the enclosing rule.
fn forgiving_list(input: &mut Parser<'_>, depth: usize) -> Vec<ComplexSelector> {
    let mut out = Vec::new();
    while !input.is_exhausted() {
        let item = input.parse_until_before(cssparser::Delimiter::Comma, |p| complex(p, depth));
        if let Ok(item) = item {
            out.push(item);
        }
        if input.next().is_err() {
            break;
        }
    }
    out
}
/// A relative selector inside `:has()`, such as `> img` or `.card`.
fn relative(input: &mut Parser<'_>, depth: usize) -> Result<(Combinator, ComplexSelector), Error> {
    if depth > 32 {
        return Err(ParseError::unexpected_token());
    }
    input.skip_whitespace();
    let combinator = if input.try_parse(|p| p.expect_delim('>')).is_ok() {
        Combinator::Child
    } else if input.try_parse(|p| p.expect_delim('+')).is_ok() {
        Combinator::Adjacent
    } else if input.try_parse(|p| p.expect_delim('~')).is_ok() {
        Combinator::Sibling
    } else {
        Combinator::Descendant
    };
    input.skip_whitespace();
    Ok((combinator, complex(input, depth)?))
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
/// A unitless `calc()`/`min()`/`max()`/`clamp()` result, e.g. `calc(10 * -1)`.
pub fn calc_number(css: &str) -> Option<f64> {
    let mut p = Parser::new(css);
    p.parse_entirely(|p| {
        let name = p.expect_function()?.to_ascii_lowercase();
        p.parse_nested_block(|p| {
            let number = |q: Quantity| match q {
                Quantity::Number(n) => Ok(n),
                _ => Err(ParseError::unexpected_token()),
            };
            if name == "calc" {
                return number(expression(p, 0)?);
            }
            let args = p.parse_comma_separated(|p| expression(p, 0).and_then(number))?;
            Ok(match (name.as_str(), args.as_slice()) {
                ("min", [_, ..]) => args.iter().copied().fold(f64::INFINITY, f64::min),
                ("max", [_, ..]) => args.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                ("clamp", [lo, v, hi]) => v.min(*hi).max(*lo),
                _ => return Err(ParseError::unexpected_token()),
            })
        })
    })
    .ok()
    .filter(|n: &f64| n.is_finite())
}
fn narrow(v: [f64; 4]) -> [f32; 4] {
    v.map(|x| x as f32)
}
pub fn calc_length(css: &str) -> Option<Length> {
    let mut p = Parser::new(css);
    p.parse_entirely(|p| {
        let name = p.expect_function()?.to_ascii_lowercase();
        p.parse_nested_block(|p| {
            let linear = |q: Quantity| match q {
                Quantity::Length(v) => Ok(v),
                Quantity::Number(0.0) => Ok([0.0; 4]),
                _ => Err(ParseError::unexpected_token()),
            };
            let pure = |v: &[f64; 4]| v[1] == 0.0 && v[2] == 0.0 && v[3] == 0.0;
            if name == "calc" {
                return Ok(Length::Calc(linear(expression(p, 0)?)?));
            }
            let args = p.parse_comma_separated(|p| expression(p, 0).and_then(linear))?;
            let (inf, neg) = (
                [f64::INFINITY, 0.0, 0.0, 0.0],
                [f64::NEG_INFINITY, 0.0, 0.0, 0.0],
            );
            Ok(match (name.as_str(), args.as_slice()) {
                ("clamp", [lo, value, hi]) => Length::Clamp([*lo, *value, *hi].map(narrow)),
                ("min" | "max", [only]) => Length::Calc(*only),
                ("min" | "max", many) if many.iter().all(pure) => {
                    let values = many.iter().map(|v| v[0]);
                    Length::Px(if name == "min" {
                        values.fold(f64::INFINITY, f64::min)
                    } else {
                        values.fold(f64::NEG_INFINITY, f64::max)
                    })
                }
                // Mixed-unit lists beyond two arguments keep the first pair.
                ("min", [a, b, ..]) => Length::Clamp([neg, *a, *b].map(narrow)),
                ("max", [a, b, ..]) => Length::Clamp([*a, *b, inf].map(narrow)),
                _ => return Err(ParseError::unexpected_token()),
            })
        })
    })
    .ok()
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
            match Length::parse(&format!("{value}{unit}")).map(viewport_px) {
                Some(Length::Px(v)) => terms[0] = v,
                Some(Length::Em(v)) => terms[2] = v,
                Some(Length::Rem(v)) => terms[3] = v,
                _ => return Err(ParseError::unexpected_token()),
            }
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
            Token::SquareBracketBlock => continue,
            Token::Ident(ref i)
                if matches!(
                    i.to_ascii_lowercase().as_str(),
                    "min-content" | "max-content" | "auto"
                ) =>
            {
                out.push(GridTrack::Length(Length::Auto))
            }
            Token::Function(f) if f.eq_ignore_ascii_case("fit-content") => {
                p.parse_nested_block(consume)?;
                out.push(GridTrack::Length(Length::Auto));
            }
            Token::Function(f)
                if f.eq_ignore_ascii_case("repeat")
                    && p.try_parse(|p| {
                        p.parse_nested_block(|p| {
                            let i = p.expect_ident()?.to_ascii_lowercase();
                            if i == "auto-fill" || i == "auto-fit" {
                                while p.next().is_ok() {}
                                Ok::<(), Error>(())
                            } else {
                                Err(ParseError::unexpected_token())
                            }
                        })
                    })
                    .is_ok() =>
            {
                // `try_parse` above consumed the block on success; re-read it.
                let mut inner = Parser::new(p.slice_from(start));
                let items = inner.parse_entirely(|p| {
                    p.expect_function()?;
                    p.parse_nested_block(|p| {
                        p.expect_ident()?;
                        p.expect_comma()?;
                        track_list(p, depth + 1)
                    })
                })?;
                out.push(GridTrack::AutoRepeat(items));
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
            token => {
                if matches!(token, Token::Function(_)) {
                    p.parse_nested_block(consume)?;
                }
                out.push(GridTrack::Length(
                    Length::parse(p.slice_from(start).trim())
                        .ok_or_else(ParseError::unexpected_token)?,
                ))
            }
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

pub(super) fn shorthand_properties(property: &str) -> &'static [&'static str] {
    match property {
        "flex" => &["flex-grow", "flex-shrink", "flex-basis"],
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
pub(super) fn expand_shorthand(decl: Declaration) -> Vec<Declaration> {
    let names = shorthand_properties(&decl.property);
    if names.is_empty() || decl.value.contains("var(") {
        // Substitution precedes shorthand parsing. Keep the pending shorthand
        // at its authored cascade position until custom properties are known.
        return vec![decl];
    }
    if matches!(decl.value.trim(), "initial" | "inherit" | "unset") {
        return names
            .iter()
            .map(|p| Declaration {
                property: (*p).into(),
                value: decl.value.clone(),
                important: decl.important,
            })
            .collect();
    }
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
