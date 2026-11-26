//! Stylesheet and declaration-block entry points.
use crate::*;
use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, Parser, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, StyleSheetParser, Token,
};
use wega_css_values::*;
type Error = ParseError<()>;

pub fn stylesheet(css: &str) -> StyleSheet {
    // `@namespace` declarations hold for this stylesheet only.
    wega_css_selectors::with_namespaces(|| parse_stylesheet(css))
}

fn parse_stylesheet(css: &str) -> StyleSheet {
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
            if decl.typed == Parsed::Invalid && rules.unsupported.len() < 256 {
                rules
                    .unsupported
                    .push(format!("invalid value:{}: {}", decl.property, decl.value));
            }
            if !supported_property(&decl.property) && rules.unsupported.len() < 256 {
                rules
                    .unsupported
                    .push(format!("property:{}", decl.property));
            }
        }
    }
    rules.unsupported.sort();
    rules.unsupported.dedup();
    let mut sheet = StyleSheet {
        rules: sheet_rules,
        font_faces,
        imports,
        unsupported: rules.unsupported,
        registered_properties: rules.registered,
        index: Default::default(),
    };
    sheet.reindex();
    sheet
}

pub(crate) struct Declarations;

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
        if value.is_empty() || !valid_var_references(&value) {
            return Err(ParseError::unexpected_token());
        }
        Ok(Declaration::new(property, value, important))
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

/// A declaration list as written: names lowercased (custom properties kept), values with
/// `!important` split off, shorthands not expanded.
pub fn raw_declarations(css: &str) -> Vec<Declaration> {
    let mut parser = Parser::new(css);
    RuleBodyParser::new(&mut parser, &mut Declarations)
        .filter_map(Result::ok)
        .collect()
}

/// Whether every `var()` in `value` is `var(<custom-property-name> [, <declaration-value>?]?)`.
/// A fallback may not contain a top-level `;` or `!` (CSS Variables 1 §3), so
/// `var(--x, !important)` makes the whole declaration invalid at parse time.
pub(crate) fn valid_var_references(value: &str) -> bool {
    fn walk(input: &mut Parser<'_>, depth: usize) -> Result<(), Error> {
        if depth > 32 {
            return Err(ParseError::unexpected_token());
        }
        while let Ok(token) = input.next() {
            match token.clone() {
                Token::Function(name) if name.eq_ignore_ascii_case("var") => {
                    input.parse_nested_block(|p| {
                        if !p.expect_ident()?.starts_with("--") {
                            return Err(ParseError::unexpected_token());
                        }
                        if p.is_exhausted() {
                            return Ok(());
                        }
                        p.expect_comma()?;
                        // The fallback: no top-level `;` or `!`.
                        while let Ok(token) = p.next() {
                            match token.clone() {
                                Token::Semicolon | Token::Delim('!') => {
                                    return Err(ParseError::unexpected_token());
                                }
                                Token::Function(_)
                                | Token::ParenthesisBlock
                                | Token::SquareBracketBlock
                                | Token::CurlyBracketBlock => {
                                    p.parse_nested_block(|p| walk(p, depth + 1))?
                                }
                                t if t.is_parse_error() => {
                                    return Err(ParseError::unexpected_token());
                                }
                                _ => {}
                            }
                        }
                        Ok(())
                    })?;
                }
                Token::Function(_)
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::CurlyBracketBlock => input.parse_nested_block(|p| walk(p, depth + 1))?,
                _ => {}
            }
        }
        Ok(())
    }
    if !value.to_ascii_lowercase().contains("var(") {
        return true;
    }
    let mut parser = Parser::new(value);
    walk(&mut parser, 0).is_ok()
}

#[cfg(test)]
mod var_tests {
    use super::valid_var_references;

    #[test]
    fn var_fallbacks_reject_top_level_semicolons_and_bangs() {
        for invalid in [
            "var(--b,!)",
            "var(--b,;)",
            "var(--b,!important)",
            "var(b)",
            "calc(var(--b,!) + 1px)",
        ] {
            assert!(!valid_var_references(invalid), "{invalid}");
        }
        for valid in [
            "var(--b)",
            "var(--b,)",
            "var(--b, red)",
            "var(--b, var(--c, 1px))",
            "var(--b, f(;))",
            "calc(var(--b) + 1px)",
            "plain",
        ] {
            assert!(valid_var_references(valid), "{valid}");
        }
    }
}
