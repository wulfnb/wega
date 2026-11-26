//! Native property values for CSSOM declaration blocks, with a fallback [`ValueEngine`] for
//! value types not ported yet. This module maps each longhand to its value parser in
//! `wega_css_values::specified` (keyword properties are handled here, from the property
//! table); each parser validates and serializes specified values as Stylo does, so moving a
//! type here changes no output.
use crate::generated::properties::KEYWORD_TYPES;
use crate::properties_table::longhand;
use crate::{ValueEngine, properties_table::Longhand};
use cssparser::Parser;
use wega_css_values::specified::{self as values, Specified};

/// Native values first, `fallback` for the rest.
pub struct NativeFirst<F: ValueEngine>(pub F);

impl<F: ValueEngine> ValueEngine for NativeFirst<F> {
    fn parse(&self, property: &str, value: &str) -> Option<Vec<(String, String)>> {
        if let Some(info) = longhand(property) {
            match parse_longhand(info, value) {
                Specified::Parsed(parsed) => {
                    return parsed.map(|value| vec![(property.to_owned(), value)]);
                }
                Specified::Unported => {}
            }
        }
        self.0.parse(property, value)
    }

    fn serialize_shorthand(&self, shorthand: &str, longhands: &[&str]) -> Option<String> {
        self.0.serialize_shorthand(shorthand, longhands)
    }
}

fn parse_longhand(info: &Longhand, value: &str) -> Specified {
    use values::*;
    if !info.keywords.is_empty() {
        return Specified::Parsed(keywords(info, value));
    }
    let base = info.value_type.rsplit("::").next().unwrap_or_default();
    if let Ok(i) = KEYWORD_TYPES.binary_search_by(|(name, _)| (*name).cmp(base)) {
        return Specified::Parsed(keyword_enum(info, KEYWORD_TYPES[i].1, value));
    }
    let each = |item: fn(&str) -> Specified| list(info.list, value, item);
    // `(keywords handled here, negative values allowed, percentages allowed)`.
    let (words, negative, percent): (&[&str], bool, bool) = match info.value_type {
        "Margin" | "Inset" => (&["auto"], true, true),
        "Size" => (&["auto"], false, true),
        "MaxSize" => (&["none"], false, true),
        "NonNegativeLengthPercentage" => (&[], false, true),
        "NonNegativeLengthPercentageOrAuto" => (&["auto"], false, true),
        "length::NonNegativeLengthPercentageOrNormal" => (&["normal"], false, true),
        "LengthPercentage" => (&[], true, true),
        "BorderSideWidth" => (&["thin", "medium", "thick"], false, false),
        "length::NonNegativeLengthOrAuto" => (&["auto"], false, false),
        "Color" | "ColorPropertyValue" => return color(value),
        "color::CaretColor" => return caret_color(value),
        "NonNegativeNumber" | "Integer" | "ZIndex" => return number(info.value_type, value),
        "Time" => return list(info.list, value, |v| time(v, info.non_negative)),
        // `auto` needs the scroll-driven animations preference, which is off.
        "AnimationDuration" => return each(|v| time(v, true)),
        "AnimationIterationCount" => return each(animation_iteration_count),
        "ColumnCount" => return column_count(value),
        "Opacity" => return opacity(value),
        "LineHeight" => return line_height(value),
        "FontWeight" => return font_weight(value),
        "BorderCornerRadius" => return corner_radius(value),
        "BorderSpacing" => return border_spacing(value),
        "Display" => return display(value).into(),
        "BackgroundRepeat" => return each(background_repeat),
        "BackgroundClip" => {
            let for_background = info.parse_method == "parse_for_background";
            return list(info.list, value, |v| background_clip(v, for_background));
        }
        "BackgroundSize" => return each(background_size),
        "Image" => return each(image),
        "position::HorizontalPosition" => {
            return each(|v| position_component(v, ["left", "right"]));
        }
        "position::VerticalPosition" => return each(|v| position_component(v, ["top", "bottom"])),
        "ContentDistribution" | "SelfAlignment" | "ItemPlacement" | "JustifyItems" => {
            let grammar = match info.value_type {
                "ContentDistribution" => Grammar::ContentDistribution,
                "SelfAlignment" => Grammar::SelfAlignment,
                _ => Grammar::ItemPlacement,
            };
            let axis = if info.parse_method == "parse_block" {
                Axis::Block
            } else {
                Axis::Inline
            };
            return alignment(value, grammar, axis).into();
        }
        _ => return Specified::Unported,
    };
    length_percentage(value, words, negative, percent)
}

/// A keyword enum value type (generated from Stylo's `#[derive(Parse)]` enums): one of its
/// keywords or their aliases, serialized as the keyword; a comma-separated list for list
/// properties.
fn keyword_enum(
    info: &Longhand,
    variants: &[(&'static str, &'static [&'static str])],
    value: &str,
) -> Option<String> {
    let one = |p: &mut Parser<'_>| -> Result<String, cssparser::ParseError<()>> {
        let ident = p.expect_ident()?.clone();
        variants
            .iter()
            .find(|(keyword, aliases)| {
                ident.eq_ignore_ascii_case(keyword)
                    || aliases.iter().any(|a| ident.eq_ignore_ascii_case(a))
            })
            .map(|(keyword, _)| (*keyword).to_owned())
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
#[path = "native_values_tests.rs"]
mod tests;
