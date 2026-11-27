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
    /// `@namespace [prefix] url`.
    Namespace(Option<String>, String),
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
    /// Whether `@namespace` may still appear: only at the top level, before any rule other
    /// than `@charset`, `@import` and `@layer` statements (CSS Namespaces 3 §2).
    namespaces_open: bool,
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
            namespaces_open: true,
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
            namespaces_open: false,
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

impl<'i> AtRuleParser<'i> for Rules {
    type Prelude = AtRulePrelude;
    type AtRule = ParsedRule;
    type Error = ();

    fn parse_prelude(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
    ) -> Result<Self::Prelude, Error> {
        let name = name.to_ascii_lowercase();
        if !matches!(name.as_str(), "namespace" | "import" | "charset" | "layer") {
            self.namespaces_open = false;
        }
        match name.as_str() {
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
            "namespace" if self.namespaces_open => {
                let prefix = input
                    .try_parse(|p| p.expect_ident().map(|i| i.to_string()))
                    .ok();
                let url = match input.next()?.clone() {
                    Token::UnquotedUrl(u) | Token::QuotedString(u) => u.to_string(),
                    Token::Function(f) if f.eq_ignore_ascii_case("url") => input
                        .parse_nested_block(|p| {
                            p.expect_string().map(|s| s.to_string()).map_err(Into::into)
                        })?,
                    _ => return Err(ParseError::unexpected_token()),
                };
                input.expect_exhausted()?;
                Ok(AtRulePrelude::Namespace(prefix, url))
            }
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
                self.namespaces_open = false;
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
            AtRulePrelude::Namespace(prefix, url) => {
                wega_css_selectors::declare_namespace(prefix.as_deref(), &url);
                Ok(ParsedRule::Media { rules: Vec::new() })
            }
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
        self.namespaces_open = false;
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
mod namespace_tests {
    use wega_css_selectors::SimpleSelector;

    fn first_compound(css: &str) -> Vec<SimpleSelector> {
        crate::stylesheet(css).rules[0].selectors[0].items[0]
            .0
            .components
            .clone()
    }

    #[test]
    fn namespace_rules_scope_type_selectors() {
        const SVG: &str = "http://www.w3.org/2000/svg";
        let svg = || SimpleSelector::Namespace(SVG.into());
        // The default namespace applies to type selectors and to bare compounds.
        assert_eq!(
            first_compound(&format!("@namespace url({SVG}); rect {{ color: red }}")),
            [svg(), SimpleSelector::Type("rect".into())]
        );
        assert_eq!(
            first_compound(&format!("@namespace \"{SVG}\"; .a {{ color: red }}"))[0],
            svg()
        );
        // Inside :is() it applies only to the explicit type selector.
        let is = first_compound(&format!(
            "@namespace url({SVG}); *|*:is(div) {{ color: red }}"
        ));
        assert_eq!(is[0], SimpleSelector::Universal);
        // Declared prefixes resolve; undeclared ones drop the rule.
        assert_eq!(
            first_compound(&format!("@namespace s url({SVG}); s|rect {{ color: red }}"))[0],
            svg()
        );
        assert!(crate::stylesheet("x|rect { color: red }").rules.is_empty());
        // Declarations do not leak into the next stylesheet.
        assert_eq!(
            first_compound("rect { color: red }"),
            [SimpleSelector::Type("rect".into())]
        );
    }
}

#[cfg(test)]
mod namespace_position_tests {
    #[test]
    fn namespace_rules_only_count_at_the_top() {
        let not_div = |css: &str| crate::stylesheet(css).rules.len();
        // Inside @media, after a style rule, or after a @layer block, @namespace is ignored,
        // so `foo|div` stays an undeclared prefix and its rule is dropped.
        assert_eq!(
            not_div("@media all { @namespace foo url(x); foo|div { color: red } }"),
            0
        );
        assert_eq!(
            not_div("p {} @namespace foo url(x); foo|div { color: red }"),
            1
        );
        assert_eq!(
            not_div("@layer a {} @namespace foo url(x); foo|div { color: red }"),
            0
        );
        assert_eq!(
            not_div("@import url(a.css); @namespace foo url(x); foo|div { color: red }"),
            1
        );
    }
}
