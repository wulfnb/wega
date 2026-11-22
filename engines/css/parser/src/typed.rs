//! Typed declarations, parsed once when the stylesheet is parsed.
//!
//! CSS engine modularization plan, Phase 2. Ported longhands parse into a
//! [`PropertyDeclaration`] here, and the cascade applies them without re-parsing.
//! Values that depend on the element (`em`, `rem`) stay specified and are resolved when
//! applied. Shorthands, logical properties, stateful longhands (`border-radius`,
//! `grid-row`/`grid-column`), `font-size`, background and mask images, and
//! `var()`-dependent values stay strings and go through `wega_style`'s
//! `apply_declaration`.
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

/// A specified `line-height`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    Normal,
    Factor(f64),
    Length(Length),
}

/// A longhand declaration with its specified value parsed.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyDeclaration {
    // Box and position.
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
    // Color and background.
    Color(Color),
    BackgroundColor(Color),
    BackgroundPosition((Length, Length)),
    BackgroundSize(BackgroundSize),
    /// `(x, y)`.
    BackgroundRepeat((bool, bool)),
    BackgroundClipText(bool),
    ClipPath(Option<Box<ClipPath>>),
    Opacity(f32),
    // Border.
    /// A border width in px or a length resolved against the font size when applied.
    BorderWidth(Side, Length),
    BorderColor(Side, Color),
    /// Whether the side's style hides it (`none` or `hidden`).
    BorderStyleHidden(Side, bool),
    BorderSpacing(Length),
    // Font and text.
    FontFamily(String),
    FontWeight(f32),
    FontItalic(bool),
    LineHeight(LineHeight),
    WhiteSpace(WhiteSpace),
    TextAlign(TextAlign),
    VerticalAlign(VerticalAlign),
    Cursor(String),
    // Flex and grid.
    FlexDirection(FlexDirection),
    JustifyContent(JustifyContent),
    AlignItems(AlignItems),
    FlexGrow(f64),
    FlexShrink(f64),
    FlexBasis(Length),
    FlexWrap(bool),
    /// `(row, column)`, resolved against the font size when applied.
    Gap(Length, Length),
    RowGap(Length),
    ColumnGap(Length),
    GridTemplateColumns(Vec<GridTrack>),
    GridTemplateRows(Vec<GridTrack>),
    GridTemplateAreas(Vec<Vec<String>>),
    // Transforms and effects.
    Transform(Option<Box<Transform>>),
    TransformOrigin((Length, Length)),
    ObjectFit(ObjectFit),
    ObjectPosition((Length, Length)),
    BoxShadow(Option<Box<BoxShadow>>),
}

/// The outcome of parsing one declaration's value.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Parsed {
    Typed(PropertyDeclaration),
    /// A ported property with a value that does not parse. Drop the declaration.
    Invalid,
    /// Not ported, or not typed before computed-value time: keep the string.
    #[default]
    Deferred,
}

const CURSORS: &[&str] = &[
    "auto",
    "default",
    "none",
    "context-menu",
    "help",
    "pointer",
    "progress",
    "wait",
    "cell",
    "crosshair",
    "text",
    "vertical-text",
    "alias",
    "copy",
    "move",
    "no-drop",
    "not-allowed",
    "grab",
    "grabbing",
    "all-scroll",
    "col-resize",
    "row-resize",
    "n-resize",
    "e-resize",
    "s-resize",
    "w-resize",
    "ne-resize",
    "nw-resize",
    "se-resize",
    "sw-resize",
    "ew-resize",
    "ns-resize",
    "nesw-resize",
    "nwse-resize",
    "zoom-in",
    "zoom-out",
];

/// `transform-origin` and `object-position`: keywords or lengths, y defaults to center.
fn origin(value: &str) -> Option<(Length, Length)> {
    let p: Vec<_> = value.split_ascii_whitespace().collect();
    let axis = |v: &str| match v {
        "center" => Some(Length::Percent(50.0)),
        "left" | "top" => Some(Length::Percent(0.0)),
        "right" | "bottom" => Some(Length::Percent(100.0)),
        _ => Length::parse(v),
    };
    let x = p.first().and_then(|v| axis(v))?;
    Some((
        x,
        p.get(1)
            .and_then(|v| axis(v))
            .unwrap_or(Length::Percent(50.0)),
    ))
}

/// A border width keyword or a non-percentage length.
fn border_width(value: &str) -> Option<Length> {
    match value.to_ascii_lowercase().as_str() {
        "thin" => Some(Length::Px(1.0)),
        "medium" => Some(Length::Px(3.0)),
        "thick" => Some(Length::Px(5.0)),
        _ => match Length::parse(value)? {
            Length::Auto | Length::Percent(_) => None,
            l => Some(l),
        },
    }
}

fn border_side(property: &str) -> Option<(Side, &str)> {
    [
        ("border-top-", Side::Top),
        ("border-right-", Side::Right),
        ("border-bottom-", Side::Bottom),
        ("border-left-", Side::Left),
    ]
    .into_iter()
    .find_map(|(prefix, side)| property.strip_prefix(prefix).map(|rest| (side, rest)))
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
        Self::parse_value(property, value)
    }

    /// Parse `value` once the cascade has substituted `var()` and `currentcolor` and the
    /// style pass's viewport is known.
    pub fn parse_value(property: &str, value: &str) -> Parsed {
        let lower = value.trim().to_ascii_lowercase();
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
                    css_number(value).map(|n| {
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
            "color" => Color::parse(value).map(P::Color),
            "background-color" => Color::parse(value).map(P::BackgroundColor),
            "background-position" => position(value).map(P::BackgroundPosition),
            "background-size" => BackgroundSize::parse(value).map(P::BackgroundSize),
            "background-repeat" => background_repeat(&components(value)).map(P::BackgroundRepeat),
            "background-clip" | "-webkit-background-clip" => {
                Some(P::BackgroundClipText(lower == "text"))
            }
            // `None` also covers valid shapes that are not painted yet (`url()`, `circle()`),
            // which must still override an earlier clip; full validation comes with full
            // parsers.
            "clip-path" => Some(P::ClipPath(ClipPath::parse(value).map(Box::new))),
            "opacity" => {
                let v = value.trim();
                v.strip_suffix('%')
                    .and_then(|p| p.trim().parse::<f64>().ok().map(|p| p / 100.0))
                    .or_else(|| css_number(v))
                    .map(|op| P::Opacity(op.clamp(0.0, 1.0) as f32))
            }
            "border-spacing" => length(P::BorderSpacing),
            p if border_side(p).is_some() => {
                let (side, rest) = border_side(p).unwrap();
                match rest {
                    "width" => border_width(value).map(|w| P::BorderWidth(side, w)),
                    "color" => Color::parse(value).map(|c| P::BorderColor(side, c)),
                    "style" => Some(P::BorderStyleHidden(
                        side,
                        matches!(value, "none" | "hidden"),
                    )),
                    _ => return Parsed::Deferred,
                }
            }
            "font-family" => Some(P::FontFamily(value.to_owned())),
            "font-weight" => match value {
                "bold" => Some(P::FontWeight(700.0)),
                "normal" => Some(P::FontWeight(400.0)),
                _ => value
                    .parse::<f32>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .map(|n| P::FontWeight(n.clamp(1.0, 1000.0))),
            },
            "font-style" => Some(P::FontItalic(matches!(value, "italic" | "oblique"))),
            "line-height" => {
                if value == "normal" {
                    Some(P::LineHeight(LineHeight::Normal))
                } else if let Ok(factor) = value.parse::<f64>() {
                    (factor >= 0.0 && factor.is_finite())
                        .then_some(P::LineHeight(LineHeight::Factor(factor)))
                } else {
                    Length::parse(value)
                        .filter(|l| !matches!(l, Length::Vw(_) | Length::Vh(_)))
                        .map(|l| P::LineHeight(LineHeight::Length(l)))
                }
            }
            "white-space" => WhiteSpace::parse(value).map(P::WhiteSpace),
            "text-align" => TextAlign::parse(value).map(P::TextAlign),
            "vertical-align" => VerticalAlign::parse(value).map(P::VerticalAlign),
            "cursor" => CURSORS
                .contains(&value)
                .then(|| P::Cursor(value.to_owned())),
            "flex-direction" => FlexDirection::parse(value).map(P::FlexDirection),
            "justify-content" => JustifyContent::parse(value).map(P::JustifyContent),
            "align-items" => AlignItems::parse(value).map(P::AlignItems),
            "flex-grow" => css_number(value).filter(|g| *g >= 0.0).map(P::FlexGrow),
            "flex-shrink" => css_number(value).filter(|s| *s >= 0.0).map(P::FlexShrink),
            "flex-basis" => length(P::FlexBasis),
            "flex-wrap" => Some(P::FlexWrap(value == "wrap")),
            "gap" => {
                let values: Vec<_> = components(value)
                    .into_iter()
                    .filter_map(Length::parse)
                    .collect();
                values
                    .first()
                    .map(|row| P::Gap(*row, *values.get(1).unwrap_or(row)))
            }
            "row-gap" => length(P::RowGap),
            "column-gap" => length(P::ColumnGap),
            "grid-template-columns" => tracks(value).map(P::GridTemplateColumns),
            "grid-template-rows" => tracks(value).map(P::GridTemplateRows),
            "grid-template-areas" => {
                Some(P::GridTemplateAreas(grid_areas(value).unwrap_or_default()))
            }
            "transform" => Some(P::Transform(Transform::parse(value).map(Box::new))),
            "transform-origin" => origin(value).map(P::TransformOrigin),
            "object-fit" => ObjectFit::parse(value).map(P::ObjectFit),
            "object-position" => origin(value).map(P::ObjectPosition),
            // Inset-only shadows parse to `None` (not painted yet) and still override.
            "box-shadow" => Some(P::BoxShadow(BoxShadow::parse(value).map(Box::new))),
            _ => return Parsed::Deferred,
        };
        typed.map_or(Parsed::Invalid, Parsed::Typed)
    }
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
        assert_eq!(
            PropertyDeclaration::parse("font-size", "12px"),
            Parsed::Deferred
        );
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

    #[test]
    fn every_family_parses_specified_values() {
        use PropertyDeclaration as P;
        let typed = |p, v| match PropertyDeclaration::parse(p, v) {
            Parsed::Typed(t) => t,
            other => panic!("{p}: {v} -> {other:?}"),
        };
        assert_eq!(
            typed("color", "#ff0000"),
            P::Color(Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            })
        );
        assert_eq!(
            typed("border-left-width", "thick"),
            P::BorderWidth(Side::Left, Length::Px(5.0))
        );
        assert_eq!(
            typed("border-top-width", "2em"),
            P::BorderWidth(Side::Top, Length::Em(2.0))
        );
        assert_eq!(
            typed("border-bottom-style", "hidden"),
            P::BorderStyleHidden(Side::Bottom, true)
        );
        assert_eq!(typed("font-weight", "bold"), P::FontWeight(700.0));
        assert_eq!(
            typed("line-height", "1.5"),
            P::LineHeight(LineHeight::Factor(1.5))
        );
        assert_eq!(typed("opacity", "50%"), P::Opacity(0.5));
        assert_eq!(
            typed("gap", "4px 8px"),
            P::Gap(Length::Px(4.0), Length::Px(8.0))
        );
        assert_eq!(typed("flex-grow", "2"), P::FlexGrow(2.0));
        assert_eq!(
            typed("transform-origin", "left"),
            P::TransformOrigin((Length::Percent(0.0), Length::Percent(50.0)))
        );
        assert!(
            matches!(typed("grid-template-columns", "1fr 2fr"), P::GridTemplateColumns(t) if t.len() == 2)
        );
        assert_eq!(
            PropertyDeclaration::parse("border-top-left-radius", "4px"),
            Parsed::Deferred
        );
        assert_eq!(
            PropertyDeclaration::parse("flex-grow", "-1"),
            Parsed::Invalid
        );
        assert_eq!(
            PropertyDeclaration::parse("cursor", "banana"),
            Parsed::Invalid
        );
    }

    #[test]
    fn viewport_math_is_typed_once_the_cascade_knows_the_viewport() {
        assert_eq!(
            PropertyDeclaration::parse("width", "calc(100vw - 20px)"),
            Parsed::Deferred
        );
        assert!(matches!(
            PropertyDeclaration::parse_value("width", "calc(100vw - 20px)"),
            Parsed::Typed(PropertyDeclaration::Width(_))
        ));
    }
}
