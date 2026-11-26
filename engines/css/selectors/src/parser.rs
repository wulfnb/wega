//! Selector parsing. Unsupported selectors reject their rule rather than broadening it.
use crate::*;
use cssparser::{ParseError, Parser, Token};
use std::cell::RefCell;
use std::collections::HashMap;
type Error = ParseError<()>;

/// The `@namespace` declarations in effect while a stylesheet is parsed (CSS Namespaces 3).
/// Queries from script have none.
#[derive(Debug, Clone, Default)]
pub struct Namespaces {
    pub default: Option<String>,
    pub prefixes: HashMap<String, String>,
}

thread_local! {
    static NAMESPACES: RefCell<Namespaces> = RefCell::new(Namespaces::default());
}

/// Run `parse` with fresh namespace declarations, restoring the previous ones afterwards.
pub fn with_namespaces<R>(parse: impl FnOnce() -> R) -> R {
    let saved = NAMESPACES.with(|n| n.replace(Namespaces::default()));
    let result = parse();
    NAMESPACES.with(|n| *n.borrow_mut() = saved);
    result
}

/// Record an `@namespace` rule for the selectors parsed after it.
pub fn declare_namespace(prefix: Option<&str>, url: &str) {
    NAMESPACES.with(|n| {
        let mut n = n.borrow_mut();
        match prefix {
            Some(prefix) => {
                n.prefixes.insert(prefix.to_owned(), url.to_owned());
            }
            None => n.default = Some(url.to_owned()),
        }
    });
}

fn default_namespace() -> Option<String> {
    NAMESPACES.with(|n| n.borrow().default.clone())
}

fn prefixed_namespace(prefix: &str) -> Option<String> {
    NAMESPACES.with(|n| n.borrow().prefixes.get(prefix).cloned())
}

pub fn selectors(css: &str) -> Vec<ComplexSelector> {
    let mut parser = Parser::new(css);
    selector_list(&mut parser, 0).unwrap_or_default()
}

pub fn selector_list(input: &mut Parser<'_>, depth: usize) -> Result<Vec<ComplexSelector>, Error> {
    if depth > 32 {
        return Err(ParseError::unexpected_token());
    }
    input.parse_comma_separated(|p| complex(p, depth))
}

pub(crate) fn complex(input: &mut Parser<'_>, depth: usize) -> Result<ComplexSelector, Error> {
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
        // `ns|E`: `*|` is any namespace, a bare `|` is no namespace, and other prefixes must
        // be declared with `@namespace`. Without a prefix the default namespace applies.
        if matches!(
            token,
            Token::Delim('|') | Token::Delim('*') | Token::Ident(_)
        ) {
            let state = input.state();
            let piped = matches!(input.next_including_whitespace(), Ok(Token::Delim('|')));
            // `Some(None)`: any namespace; `Some(Some(url))`: that namespace.
            let (namespace, local) = if matches!(token, Token::Delim('|')) {
                input.reset(&state);
                (
                    Some(Some(String::new())),
                    input.next_including_whitespace()?.clone(),
                )
            } else if piped {
                let namespace = match &token {
                    Token::Delim('*') => None,
                    Token::Ident(prefix) => {
                        Some(prefixed_namespace(prefix).ok_or_else(ParseError::unexpected_token)?)
                    }
                    _ => unreachable!(),
                };
                (Some(namespace), input.next_including_whitespace()?.clone())
            } else {
                input.reset(&state);
                (None, token.clone())
            };
            let simple = match local {
                Token::Delim('*') => SimpleSelector::Universal,
                Token::Ident(s) => SimpleSelector::Type(s.to_string()),
                _ => return Err(ParseError::unexpected_token()),
            };
            let namespace = namespace.unwrap_or_else(default_namespace);
            if let Some(url) = namespace {
                components.push(SimpleSelector::Namespace(url));
            }
            {
                components.push(simple);
                if items.len() > 128 {
                    return Err(ParseError::unexpected_token());
                }
                continue;
            }
        }
        let simple = match token {
            Token::Delim('*') => SimpleSelector::Universal,
            Token::Ident(s) => SimpleSelector::Type(s.to_string()),
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
    // A default namespace also applies to compounds without a type selector, except
    // inside `:is()`, `:where()`, `:not()` and `:has()` (Selectors 4 §17.1).
    if depth == 0
        && let Some(url) = default_namespace()
    {
        for (compound, _) in &mut items {
            if !compound.components.iter().any(|c| {
                matches!(
                    c,
                    SimpleSelector::Type(_)
                        | SimpleSelector::Universal
                        | SimpleSelector::Namespace(_)
                )
            }) {
                compound
                    .components
                    .insert(0, SimpleSelector::Namespace(url.clone()));
            }
        }
    }
    Ok(ComplexSelector { items })
}

pub(crate) fn pseudo(input: &mut Parser<'_>, depth: usize) -> Result<SimpleSelector, Error> {
    // `::name` and the legacy single-colon pseudo-elements select generated
    // content, never the element itself. Keep them so the rest of the rule's
    // selector list survives instead of invalidating the whole rule.
    let element = |name: &str| SimpleSelector::PseudoElement(name.to_ascii_lowercase());
    let token = input.next_including_whitespace()?.clone();
    Ok(match token {
        // Unknown pseudo-elements invalidate the selector list (Selectors 4 §3.6.1).
        // `-webkit-` ones are kept, as Chrome does, so the rest of such a list applies.
        Token::Colon => match input.next_including_whitespace()?.clone() {
            Token::Ident(s) if known_pseudo_element(&s) => element(&s),
            Token::Function(s) if known_functional_pseudo_element(&s) => {
                input.parse_nested_block(|p| {
                    while p.next().is_ok() {}
                    Ok::<_, Error>(())
                })?;
                element(&s)
            }
            _ => return Err(ParseError::unexpected_token()),
        },
        Token::Ident(s) => match s.to_ascii_lowercase().as_str() {
            "root" => SimpleSelector::Root,
            "scope" => SimpleSelector::Scope,
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
            "visited"
            | "target"
            | "target-within"
            | "indeterminate"
            | "invalid"
            | "user-invalid"
            | "fullscreen"
            | "modal"
            | "popover-open"
            | "autofill"
            | "-webkit-autofill"
            | "host"
            | "default"
            | "out-of-range"
            | "open"
            | "-moz-meter-optimum"
            | "-moz-meter-sub-optimum"
            | "-moz-meter-sub-sub-optimum" => SimpleSelector::Never,
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
                    let last = s.eq_ignore_ascii_case("nth-last-child");
                    if p.is_exhausted() {
                        SimpleSelector::NthChild(a, b, last)
                    } else {
                        p.expect_ident_matching("of")?;
                        SimpleSelector::NthChildOf(a, b, last, selector_list(p, depth + 1)?)
                    }
                }
                "nth-of-type" | "nth-last-of-type" => {
                    let (a, b) = cssparser::parse_nth(p)?;
                    SimpleSelector::NthOfType(a, b, s.eq_ignore_ascii_case("nth-last-of-type"))
                }
                "lang" => {
                    let tag = p.expect_ident_or_string()?.to_ascii_lowercase();
                    SimpleSelector::Lang(tag)
                }
                // Directionality is not tracked yet: documents are left-to-right.
                "dir" => match p.expect_ident()?.to_ascii_lowercase().as_str() {
                    "ltr" => SimpleSelector::Universal,
                    "rtl" => SimpleSelector::Never,
                    _ => return Err(ParseError::unexpected_token()),
                },
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
pub(crate) fn forgiving_list(input: &mut Parser<'_>, depth: usize) -> Vec<ComplexSelector> {
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
pub(crate) fn relative(
    input: &mut Parser<'_>,
    depth: usize,
) -> Result<(Combinator, ComplexSelector), Error> {
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

/// `[name]`, `[|name]` (no namespace) or `[*|name]` (any namespace), with no whitespace
/// around the `|`. Named namespace prefixes need `@namespace`, which is not supported, so
/// they are invalid like any undeclared prefix.
fn attribute_name(input: &mut Parser<'_>) -> Result<String, Error> {
    input.skip_whitespace();
    let name = |input: &mut Parser<'_>| match input.next_including_whitespace()? {
        Token::Ident(n) => Ok(n.to_string()),
        _ => Err(ParseError::unexpected_token()),
    };
    match input.next_including_whitespace()?.clone() {
        Token::Delim('|') => name(input),
        Token::Delim('*') => match input.next_including_whitespace()? {
            Token::Delim('|') => name(input),
            _ => Err(ParseError::unexpected_token()),
        },
        Token::Ident(n) => {
            let state = input.state();
            if let Ok(Token::Delim('|')) = input.next_including_whitespace() {
                return Err(ParseError::unexpected_token());
            }
            input.reset(&state);
            Ok(n.to_string())
        }
        _ => Err(ParseError::unexpected_token()),
    }
}

fn known_pseudo_element(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("-webkit-")
        || matches!(
            name.as_str(),
            "before"
                | "after"
                | "first-line"
                | "first-letter"
                | "marker"
                | "placeholder"
                | "selection"
                | "backdrop"
                | "file-selector-button"
                | "cue"
                | "grammar-error"
                | "spelling-error"
                | "target-text"
                | "details-content"
                | "view-transition"
                | "picker-icon"
                | "color-swatch"
                | "slider-fill"
                | "slider-thumb"
                | "slider-track"
                | "checkmark"
                | "column"
                | "scroll-marker"
                | "scroll-marker-group"
                | "scroll-button"
        )
}

fn known_functional_pseudo_element(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("-webkit-")
        || matches!(
            name.as_str(),
            "highlight"
                | "picker"
                | "scroll-button"
                | "part"
                | "slotted"
                | "cue"
                | "view-transition-group"
                | "view-transition-image-pair"
                | "view-transition-old"
                | "view-transition-new"
        )
}

pub(crate) fn attribute(input: &mut Parser<'_>) -> Result<SimpleSelector, Error> {
    let name = attribute_name(input)?;
    input.skip_whitespace();
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
        None
    } else {
        let flag = input.expect_ident()?;
        if flag.eq_ignore_ascii_case("i") {
            Some(true)
        } else if flag.eq_ignore_ascii_case("s") {
            Some(false)
        } else {
            return Err(ParseError::unexpected_token());
        }
    };
    Ok(SimpleSelector::AttrMatch(name, value, op, insensitive))
}

pub fn parse_selector_list(s: &str) -> Vec<ComplexSelector> {
    selectors(s)
}

pub fn parse_complex_selector(s: &str) -> Option<ComplexSelector> {
    let mut list = parse_selector_list(s);
    (list.len() == 1).then(|| list.remove(0))
}

pub fn parse_compound_selector(s: &str) -> Option<CompoundSelector> {
    let mut sel = parse_complex_selector(s)?;
    (sel.items.len() == 1).then(|| sel.items.remove(0).0)
}

// ---------------------------------------------------------------------------
// DEFAULT USER-AGENT STYLESHEET
// ---------------------------------------------------------------------------

#[cfg(test)]
mod validity_tests {
    use super::selectors;

    #[test]
    fn invalid_selectors_reject_the_list() {
        for invalid in [
            "::x-something-nobody-would-think-of, p",
            "div:dir(ltrr)",
            "[ | data-test-4]",
            "[ns|data-test]",
            "[* | a]",
            "svg|rect",
            "li:nth-child(2n of)",
        ] {
            assert!(selectors(invalid).is_empty(), "{invalid}");
        }
        for valid in [
            "[ data-test-1]",
            "[data-test-2 ]",
            "[data-test-3 = x]",
            "[ |data-test-4]",
            "[*|a]",
            "[lang|=en]",
            "p::before, p::marker",
            "input::-webkit-input-placeholder",
            "div:dir(ltr)",
            "a:after",
            "*|div",
            "|div",
            "*|*",
            ":scope > p",
            "li:nth-child(2n+1 of .item)",
            "input:default",
        ] {
            assert!(!selectors(valid).is_empty(), "{valid}");
        }
    }
}
