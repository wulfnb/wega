//! Selector parsing. Unsupported selectors reject their rule rather than broadening it.
use crate::*;
use cssparser::{ParseError, Parser, Token};
type Error = ParseError<()>;

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
        Token::Ident(n) => Ok(n.to_ascii_lowercase()),
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
            Ok(n.to_ascii_lowercase())
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
        ] {
            assert!(!selectors(valid).is_empty(), "{valid}");
        }
    }
}
