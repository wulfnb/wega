//! Native property values for CSSOM declaration blocks, with a fallback [`ValueEngine`] for
//! value types not ported yet. Each ported type validates and serializes specified values
//! as Stylo does (`stylo/values/specified/*`), so moving a type here changes no output.
use crate::properties_table::longhand;
use crate::{ValueEngine, properties_table::Longhand};
use cssparser::Parser;

/// Native values first, `fallback` for the rest.
pub struct NativeFirst<F: ValueEngine>(pub F);

/// The result of parsing one longhand natively.
enum Native {
    /// Parsed: the serialized specified value, or `None` if invalid.
    Parsed(Option<String>),
    /// Not ported: ask the fallback engine.
    Unported,
}

impl<F: ValueEngine> ValueEngine for NativeFirst<F> {
    fn parse(&self, property: &str, value: &str) -> Option<Vec<(String, String)>> {
        if let Some(info) = longhand(property) {
            match parse_longhand(info, value) {
                Native::Parsed(parsed) => {
                    return parsed.map(|value| vec![(property.to_owned(), value)]);
                }
                Native::Unported => {}
            }
        }
        self.0.parse(property, value)
    }

    fn serialize_shorthand(&self, shorthand: &str, longhands: &[&str]) -> Option<String> {
        self.0.serialize_shorthand(shorthand, longhands)
    }
}

fn parse_longhand(info: &Longhand, value: &str) -> Native {
    if !info.keywords.is_empty() {
        return Native::Parsed(keywords(info, value));
    }
    Native::Unported
}

/// A keyword property (Stylo `single_keyword`): one of its keywords, ASCII
/// case-insensitively, serialized in lowercase; a comma-separated list for list properties.
fn keywords(info: &Longhand, value: &str) -> Option<String> {
    let one = |p: &mut Parser<'_>| -> Result<String, cssparser::ParseError<()>> {
        let ident = p.expect_ident()?.clone();
        info.keywords
            .iter()
            .find(|k| ident.eq_ignore_ascii_case(k))
            .map(|k| (*k).to_owned())
            .ok_or_else(cssparser::ParseError::unexpected_token)
    };
    let mut parser = Parser::new(value);
    let values = parser
        .parse_entirely(|p| {
            if info.list {
                p.parse_comma_separated(one)
            } else {
                one(p).map(|k| vec![k])
            }
        })
        .ok()?;
    Some(values.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Never;
    impl ValueEngine for Never {
        fn parse(&self, _: &str, _: &str) -> Option<Vec<(String, String)>> {
            Some(vec![("fallback".into(), String::new())])
        }
        fn serialize_shorthand(&self, _: &str, _: &[&str]) -> Option<String> {
            None
        }
    }

    #[test]
    fn keyword_properties_are_native() {
        let engine = NativeFirst(Never);
        let one = |p: &str, v: &str| engine.parse(p, v).map(|v| v[0].1.clone());
        assert_eq!(
            one("background-attachment", "FIXED").as_deref(),
            Some("fixed")
        );
        assert_eq!(
            one("background-attachment", "scroll ,  fixed").as_deref(),
            Some("scroll, fixed")
        );
        assert_eq!(one("background-attachment", "sideways"), None);
        assert_eq!(one("background-attachment", "fixed fixed"), None);
        // Not ported yet: the fallback answers.
        assert_eq!(engine.parse("width", "1px").unwrap()[0].0, "fallback");
    }
}
