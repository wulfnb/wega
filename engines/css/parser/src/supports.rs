//! `@supports` conditions and `CSS.supports()` (CSS Conditional 3 §2 and §6.1).
use crate::*;
use cssparser::{ParseError, Parser, Token};
use wega_css_selectors::selectors;
type Error = ParseError<()>;

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

/// `CSS.supports(conditionText)` (CSS Conditional 3 §6.1): the text as a
/// `<supports-condition>`, or failing that, wrapped in parentheses, so that a bare
/// declaration such as `display: grid` works.
pub fn supports_text(condition: &str) -> bool {
    let evaluate = |text: &str| {
        Parser::new(text)
            .parse_entirely(|p| {
                supports_condition(p, 0).map_err(|_| p.new_error_for_next_token::<()>())
            })
            .ok()
    };
    evaluate(condition) == Some(true) || evaluate(&format!("({condition})")) == Some(true)
}

/// `CSS.supports(property, value)`: whether `value` is valid for the property, which is
/// matched literally (no `!important`, no extra syntax around the name).
pub fn supports_property_value(property: &str, value: &str) -> bool {
    let well_formed_name = !property.is_empty()
        && property
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii());
    let mut bang = false;
    let mut input = Parser::new(value);
    while let Ok(token) = input.next() {
        bang |= matches!(token, Token::Delim('!'));
    }
    well_formed_name && !bang && supports_text(&format!("({property}: {value})"))
}

#[cfg(test)]
mod css_supports_tests {
    use super::*;

    #[test]
    fn css_supports_accepts_conditions_and_bare_declarations() {
        assert!(supports_text("(display: flex)"));
        assert!(supports_text("display: flex"));
        assert!(supports_text("(color: red) and (margin: 0)"));
        assert!(!supports_text("display: nonsense"));
        assert!(!supports_text(
            "(color: red) and (margin: 0) or (padding: 0)"
        ));
        assert!(supports_property_value("color", "red"));
        assert!(supports_property_value("--anything", "x"));
        assert!(!supports_property_value("color", "red !important"));
        assert!(!supports_property_value("color:", "red"));
        assert!(!supports_property_value("unknown-property", "1"));
    }
}
