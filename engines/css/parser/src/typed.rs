//! Typed declarations, parsed once when the stylesheet is parsed.
//!
//! CSS engine modularization plan, Phase 2. Ported property families parse into a
//! [`PropertyDeclaration`] here, and the cascade applies them without re-parsing. Families
//! not ported yet, shorthands, logical properties and `var()`-dependent values stay
//! strings and go through `wega_style`'s `apply_declaration`.
//!
//! Ported: box and position (`display`, `position`, `box-sizing`, `visibility`,
//! `pointer-events`, sizes, margin and padding longhands, insets, `z-index`, `float`,
//! `clear`, `overflow-x`, `overflow-y`).
//!
//! Reference (concepts, not code): Stylo `properties/declaration_block.rs` and the
//! generated `PropertyDeclaration` enum.
use wega_css_values::*;

/// One physical box side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

/// A longhand declaration with its value parsed into the computed-value type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PropertyDeclaration {
    Display(Display),
    Position(Position),
    BoxSizing(BoxSizing),
    Visibility(bool),
    PointerEvents(bool),
    Width(Length),
    Height(Length),
    MinWidth(Length),
    MaxWidth(Length),
    MinHeight(Length),
    MaxHeight(Length),
    Margin(Side, Length),
    Padding(Side, Length),
    Inset(Side, Length),
    ZIndex(Option<i32>),
    Float(Float),
    /// `(left, right)`.
    Clear(bool, bool),
    OverflowX(Overflow),
    OverflowY(Overflow),
}

/// The outcome of parsing one declaration's value.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Parsed {
    Typed(PropertyDeclaration),
    /// A ported property with a value that does not parse. Drop the declaration.
    Invalid,
    /// Not ported yet, or not typed before computed-value time: keep the string.
    #[default]
    Deferred,
}

impl PropertyDeclaration {
    /// Parse `value` for the longhand `property`.
    ///
    /// CSS-wide keywords, `var()` and `currentcolor` are resolved by the cascade, so they
    /// return [`Parsed::Deferred`].
    pub fn parse(property: &str, value: &str) -> Parsed {
        let lower = value.trim().to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "inherit" | "initial" | "unset" | "revert" | "revert-layer"
        ) || lower.contains("var(")
            || lower.contains("currentcolor")
            // Math functions resolve viewport units against the style pass's viewport,
            // which is only known at cascade time.
            || (lower.contains('(') && ["vw", "vh", "vmin", "vmax"].iter().any(|u| lower.contains(u)))
        {
            return Parsed::Deferred;
        }
        let length = |make: fn(Length) -> Self| Length::parse(value).map(make);
        let side =
            |make: fn(Side, Length) -> Self, side| Length::parse(value).map(|l| make(side, l));
        use PropertyDeclaration as P;
        let typed = match property {
            "display" => Display::parse(value).map(P::Display),
            "position" => Position::parse(value).map(P::Position),
            "box-sizing" => BoxSizing::parse(value).map(P::BoxSizing),
            "visibility" => matches!(value, "visible" | "hidden" | "collapse")
                .then(|| P::Visibility(value == "visible")),
            "pointer-events" => {
                matches!(value, "auto" | "none").then(|| P::PointerEvents(value == "auto"))
            }
            "width" => length(P::Width),
            "height" => length(P::Height),
            "min-width" => length(P::MinWidth),
            "max-width" => length(P::MaxWidth),
            "min-height" => length(P::MinHeight),
            "max-height" => length(P::MaxHeight),
            "margin-top" => side(P::Margin, Side::Top),
            "margin-right" => side(P::Margin, Side::Right),
            "margin-bottom" => side(P::Margin, Side::Bottom),
            "margin-left" => side(P::Margin, Side::Left),
            "padding-top" => side(P::Padding, Side::Top),
            "padding-right" => side(P::Padding, Side::Right),
            "padding-bottom" => side(P::Padding, Side::Bottom),
            "padding-left" => side(P::Padding, Side::Left),
            "top" => side(P::Inset, Side::Top),
            "right" => side(P::Inset, Side::Right),
            "bottom" => side(P::Inset, Side::Bottom),
            "left" => side(P::Inset, Side::Left),
            "z-index" => {
                if lower == "auto" {
                    Some(P::ZIndex(None))
                } else {
                    number(value).map(|n| {
                        P::ZIndex(Some(
                            n.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
                        ))
                    })
                }
            }
            "float" => match lower.as_str() {
                "left" | "inline-start" => Some(P::Float(Float::Left)),
                "right" | "inline-end" => Some(P::Float(Float::Right)),
                "none" => Some(P::Float(Float::None)),
                _ => None,
            },
            "clear" => match lower.as_str() {
                "left" | "inline-start" => Some(P::Clear(true, false)),
                "right" | "inline-end" => Some(P::Clear(false, true)),
                "both" => Some(P::Clear(true, true)),
                "none" => Some(P::Clear(false, false)),
                _ => None,
            },
            "overflow-x" => Overflow::parse(value).map(P::OverflowX),
            "overflow-y" => Overflow::parse(value).map(P::OverflowY),
            _ => return Parsed::Deferred,
        };
        typed.map_or(Parsed::Invalid, Parsed::Typed)
    }
}

/// A finite number, or a `calc()` that resolves to one.
fn number(value: &str) -> Option<f64> {
    let v = value.trim();
    v.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .or_else(|| calc_number(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ported_longhands_parse_to_typed_values() {
        use PropertyDeclaration as P;
        assert_eq!(
            PropertyDeclaration::parse("display", "flex"),
            Parsed::Typed(P::Display(Display::Flex))
        );
        assert_eq!(
            PropertyDeclaration::parse("margin-left", "12px"),
            Parsed::Typed(P::Margin(Side::Left, Length::Px(12.0)))
        );
        assert_eq!(
            PropertyDeclaration::parse("z-index", "calc(2 + 1)"),
            Parsed::Typed(P::ZIndex(Some(3)))
        );
        assert_eq!(
            PropertyDeclaration::parse("z-index", "auto"),
            Parsed::Typed(P::ZIndex(None))
        );
        assert_eq!(
            PropertyDeclaration::parse("clear", "both"),
            Parsed::Typed(P::Clear(true, true))
        );
    }

    #[test]
    fn invalid_values_are_reported_and_cascade_keywords_are_deferred() {
        assert_eq!(PropertyDeclaration::parse("width", "wide"), Parsed::Invalid);
        assert_eq!(
            PropertyDeclaration::parse("float", "middle"),
            Parsed::Invalid
        );
        assert_eq!(
            PropertyDeclaration::parse("z-index", "high"),
            Parsed::Invalid
        );
        for value in [
            "inherit",
            "initial",
            "unset",
            "var(--w)",
            "calc(var(--w) * 2)",
            "calc(100vw - 20px)",
            "min(50vh, 300px)",
        ] {
            assert_eq!(PropertyDeclaration::parse("width", value), Parsed::Deferred);
        }
        assert_eq!(PropertyDeclaration::parse("color", "red"), Parsed::Deferred);
        assert_eq!(
            PropertyDeclaration::parse("margin", "1px"),
            Parsed::Deferred
        );
    }

    #[test]
    fn stylesheets_report_invalid_declarations() {
        let sheet = crate::StyleSheet::parse("p { width: wide; float: middle; height: 4px }");
        assert!(
            sheet
                .unsupported
                .iter()
                .any(|u| u == "invalid value:width: wide")
        );
        assert!(
            sheet
                .unsupported
                .iter()
                .any(|u| u == "invalid value:float: middle")
        );
        assert_eq!(
            sheet.rules[0].declarations[2].typed,
            Parsed::Typed(PropertyDeclaration::Height(Length::Px(4.0)))
        );
    }
}
