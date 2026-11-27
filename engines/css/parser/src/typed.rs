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
    FlexWrap(FlexWrapMode),
    AlignContent(AlignContent),
    WritingMode(WritingMode),
    /// `direction: rtl`.
    DirectionRtl(bool),
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

/// A length that is not negative where the property forbids negative values.
/// Math functions clamp at computed time, so they are accepted.
fn non_negative(l: &Length) -> bool {
    match l {
        Length::Px(v)
        | Length::Percent(v)
        | Length::Em(v)
        | Length::Rem(v)
        | Length::Vw(v)
        | Length::Vh(v) => *v >= 0.0,
        _ => true,
    }
}

/// A `<length-percentage [0,∞]>` or `normal` for the gap longhands.
fn gap_value(value: &str) -> Option<Length> {
    if value.trim().eq_ignore_ascii_case("normal") {
        return Some(Length::Px(0.0));
    }
    Length::parse(value).filter(|l| *l != Length::Auto && non_negative(l))
}

/// Whether every comma-separated `background-image` layer is `none`, a `url()`, an image
/// function, or a gradient whose arguments are well formed. Invalid values are dropped
/// at parse time instead of clearing an earlier image.
pub fn valid_background_image(value: &str) -> bool {
    use cssparser::{Parser, Token};
    let mut parser = Parser::new(value);
    parser
        .parse_entirely(|p| {
            p.parse_comma_separated(|p| {
                let bad = |p: &mut Parser<'_>| Err(p.new_error_for_next_token::<()>());
                match p.next()?.clone() {
                    Token::Ident(i) if i.eq_ignore_ascii_case("none") => Ok(()),
                    Token::UnquotedUrl(_) => Ok(()),
                    Token::Function(f) => {
                        let f = f.to_ascii_lowercase();
                        p.parse_nested_block(|p| {
                            let ok = match f.as_str() {
                                "linear-gradient"
                                | "repeating-linear-gradient"
                                | "-webkit-linear-gradient" => linear_gradient_args(p),
                                "url"
                                | "image-set"
                                | "-webkit-image-set"
                                | "cross-fade"
                                | "radial-gradient"
                                | "repeating-radial-gradient"
                                | "conic-gradient"
                                | "repeating-conic-gradient"
                                | "-webkit-radial-gradient"
                                | "-webkit-gradient" => {
                                    let mut ok = true;
                                    while let Ok(t) = p.next() {
                                        ok &= !t.is_parse_error();
                                    }
                                    ok
                                }
                                _ => false,
                            };
                            if ok { Ok(()) } else { bad(p) }
                        })
                    }
                    _ => bad(p),
                }
            })
        })
        .is_ok()
}

/// `[ <angle> | to <side-or-corner> ]? [in <colorspace>]?, <color-stop-list>`: the
/// leading argument and that every stop starts with a color or is a lone hint.
fn linear_gradient_args(p: &mut cssparser::Parser<'_>) -> bool {
    use cssparser::Token;
    let args: Result<Vec<String>, _> = p.parse_comma_separated(|p| {
        let start = p.position();
        while p.next().is_ok() {}
        Ok::<_, cssparser::ParseError<()>>(p.slice_from(start).trim().to_owned())
    });
    let Ok(args) = args else { return false };
    let mut stops = &args[..];
    if let Some(first) = args.first() {
        let mut head = cssparser::Parser::new(first);
        let leading = match head.next() {
            Ok(Token::Ident(w)) => w.eq_ignore_ascii_case("to") || w.eq_ignore_ascii_case("in"),
            Ok(Token::Dimension { value, unit, .. }) => {
                if angle_degrees(*value, unit).is_none() {
                    return false;
                }
                true
            }
            Ok(Token::Number { value, .. }) => *value == 0.0,
            _ => false,
        };
        if leading {
            stops = &args[1..];
        }
    }
    stops.len() >= 2
        && stops.iter().all(|stop| {
            let words = components(stop);
            match words.split_first() {
                Some((color, rest)) if Color::parse(color).is_some() => {
                    rest.len() <= 2 && rest.iter().all(|w| Length::parse(w).is_some())
                }
                // A color hint: one length or percentage.
                Some((hint, [])) => Length::parse(hint).is_some(),
                _ => false,
            }
        })
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
            "inherit" | "initial" | "unset" | "revert" | "revert-layer" | "revert-rule"
        ) || lower.contains("var(")
            || lower.contains("attr(")
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
            "flex-basis" => Length::parse(value).filter(non_negative).map(P::FlexBasis),
            "flex-wrap" => FlexWrapMode::parse(value).map(P::FlexWrap),
            "align-content" => AlignContent::parse(value).map(P::AlignContent),
            "writing-mode" => WritingMode::parse(value).map(P::WritingMode),
            "direction" => match lower.as_str() {
                "ltr" => Some(P::DirectionRtl(false)),
                "rtl" => Some(P::DirectionRtl(true)),
                _ => None,
            },
            "gap" => {
                let values: Option<Vec<_>> = components(value).into_iter().map(gap_value).collect();
                match values.as_deref() {
                    Some([row]) => Some(P::Gap(*row, *row)),
                    Some([row, column]) => Some(P::Gap(*row, *column)),
                    _ => None,
                }
            }
            "row-gap" => gap_value(value).map(P::RowGap),
            "column-gap" => gap_value(value).map(P::ColumnGap),
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
            "background-image" if !valid_background_image(value) => return Parsed::Invalid,
            _ => return Parsed::Deferred,
        };
        typed.map_or(Parsed::Invalid, Parsed::Typed)
    }
}

#[cfg(test)]
#[path = "typed_tests.rs"]
mod tests;
