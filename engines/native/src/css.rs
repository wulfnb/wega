//! Wega Native CSS Engine: Rule representation, Selectors, Specificity, Cascade, and Computed Styles.
use wega_dom::{DocumentView, NodeKey, NodeKind, NodeView};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };

    pub fn parse(s: &str) -> Option<Self> {
        let mut parser = cssparser::Parser::new(s.trim());
        parser
            .parse_entirely(|p| {
                Self::parse_token(p).map_err(|_| p.new_error_for_next_token::<()>())
            })
            .ok()
    }

    /// CSS Color 4 sRGB syntax: names, hex, `rgb()`/`hsl()`/`hwb()` in both
    /// legacy comma and modern space/slash forms. `currentcolor` and the
    /// wide-gamut functions are left to the caller.
    pub fn parse_css(p: &mut cssparser::Parser<'_>) -> Option<Self> {
        Self::parse_token(p).ok()
    }
    fn parse_token(p: &mut cssparser::Parser<'_>) -> Result<Self, ()> {
        use cssparser::Token;
        let rgba = |(r, g, b): (u8, u8, u8), a: f32| Self {
            r,
            g,
            b,
            a: (a.clamp(0.0, 1.0) * 255.0).round() as u8,
        };
        match p.next().map_err(|_| ())?.clone() {
            Token::Ident(name) => {
                if name.eq_ignore_ascii_case("transparent") {
                    return Ok(Self::TRANSPARENT);
                }
                cssparser::color::parse_named_color(&name.to_ascii_lowercase())
                    .map(|c| rgba(c, 1.0))
            }
            Token::Hash(h) | Token::IDHash(h) => cssparser::color::parse_hash_color(h.as_bytes())
                .map(|(r, g, b, a)| rgba((r, g, b), a)),
            Token::Function(name) => {
                let name = name.to_ascii_lowercase();
                p.parse_nested_block(|p| {
                    if name == "color-mix" {
                        color_mix(p)
                    } else {
                        color_function(&name, p)
                    }
                    .map_err(|_| p.new_error_for_next_token::<()>())
                })
                .map_err(|_| ())
            }
            _ => Err(()),
        }
    }
}

fn gamma(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
fn linear(v: f32) -> f32 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
/// OKLab to gamma-encoded sRGB (0..1, unclamped).
fn oklab_to_srgb(l: f32, a: f32, b: f32) -> [f32; 3] {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        gamma(4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_),
        gamma(-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_),
        gamma(-0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_),
    ]
}
/// CIE Lab (D50) to gamma-encoded sRGB via Bradford-adapted XYZ D65.
fn lab_to_srgb(l: f32, a: f32, b: f32) -> [f32; 3] {
    let fy = (l + 16.0) / 116.0;
    let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
    let inverse = |t: f32| {
        if t > 6.0 / 29.0 {
            t.powi(3)
        } else {
            3.0 * (6.0f32 / 29.0).powi(2) * (t - 4.0 / 29.0)
        }
    };
    let (x, y, z) = (0.964_22 * inverse(fx), inverse(fy), 0.825_21 * inverse(fz));
    // D50 -> D65
    let (x, y, z) = (
        0.955_576_6 * x - 0.023_039_3 * y + 0.063_163_6 * z,
        -0.028_289_5 * x + 1.009_941_6 * y + 0.021_007_7 * z,
        0.012_298_2 * x - 0.020_483 * y + 1.329_909_8 * z,
    );
    [
        gamma(3.240_97 * x - 1.537_383_2 * y - 0.498_610_76 * z),
        gamma(-0.969_243_6 * x + 1.875_967_5 * y + 0.041_555_06 * z),
        gamma(0.055_630_08 * x - 0.203_976_96 * y + 1.056_971_5 * z),
    ]
}
/// `color-mix(in <space>, <color> [p%], <color> [p%])`, mixed with
/// premultiplied alpha in linear-light sRGB (close to the OKLab results that
/// authoring tools such as Tailwind request for opacity variants).
fn color_mix(p: &mut cssparser::Parser<'_>) -> Result<Color, ()> {
    // The interpolation clause is optional here: Stylo's serialization omits
    // its default (`in oklab`) along with default 50% weights.
    if p.try_parse(|p| p.expect_ident_matching("in")).is_ok() {
        p.expect_ident().map_err(|_| ())?;
        // Optional hue interpolation method, e.g. `shorter hue`.
        let _ = p.try_parse(|p| {
            p.expect_ident()?;
            p.expect_ident_matching("hue")
        });
        p.expect_comma().map_err(|_| ())?;
    }
    let item = |p: &mut cssparser::Parser<'_>| -> Result<(Color, Option<f32>), ()> {
        let mut percent = p.try_parse(|p| p.expect_percentage()).ok();
        let color = Color::parse_token(p)?;
        if percent.is_none() {
            percent = p.try_parse(|p| p.expect_percentage()).ok();
        }
        Ok((color, percent))
    };
    let (a, pa) = item(p)?;
    p.expect_comma().map_err(|_| ())?;
    let (b, pb) = item(p)?;
    let (pa, pb) = match (pa, pb) {
        (Some(x), Some(y)) => (x, y),
        (Some(x), None) => (x, 1.0 - x),
        (None, Some(y)) => (1.0 - y, y),
        (None, None) => (0.5, 0.5),
    };
    let total = pa + pb;
    if total <= 0.0 {
        return Err(());
    }
    let (wa, wb) = (pa / total, pb / total);
    let alpha = |c: Color| c.a as f32 / 255.0;
    let mixed_alpha = alpha(a) * wa + alpha(b) * wb;
    let channel = |x: u8, y: u8| {
        let premultiplied =
            linear(x as f32 / 255.0) * alpha(a) * wa + linear(y as f32 / 255.0) * alpha(b) * wb;
        let v = if mixed_alpha > 0.0 {
            premultiplied / mixed_alpha
        } else {
            0.0
        };
        (gamma(v).clamp(0.0, 1.0) * 255.0).round() as u8
    };
    let scale = total.min(1.0);
    Ok(Color {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: ((mixed_alpha * scale).clamp(0.0, 1.0) * 255.0).round() as u8,
    })
}
/// One numeric channel: (value, was_percentage). `none` reads as zero.
fn channel(p: &mut cssparser::Parser<'_>) -> Result<(f32, bool), ()> {
    use cssparser::Token;
    match p.next().map_err(|_| ())?.clone() {
        Token::Number { value, .. } => Ok((value, false)),
        Token::Percentage { unit_value, .. } => Ok((unit_value, true)),
        Token::Dimension { value, unit, .. } => Ok((
            match unit.to_ascii_lowercase().as_str() {
                "deg" => value,
                "rad" => value.to_degrees(),
                "grad" => value * 0.9,
                "turn" => value * 360.0,
                _ => return Err(()),
            },
            false,
        )),
        Token::Ident(ref i) if i.eq_ignore_ascii_case("none") => Ok((0.0, false)),
        _ => Err(()),
    }
}
fn color_function(name: &str, p: &mut cssparser::Parser<'_>) -> Result<Color, ()> {
    let first = channel(p)?;
    let legacy = p.try_parse(|p| p.expect_comma()).is_ok();
    let second = channel(p)?;
    if legacy {
        p.expect_comma().map_err(|_| ())?;
    }
    let third = channel(p)?;
    let alpha = if legacy {
        p.try_parse(|p| p.expect_comma())
            .ok()
            .map(|_| channel(p))
            .transpose()?
    } else {
        p.try_parse(|p| p.expect_delim('/'))
            .ok()
            .map(|_| channel(p))
            .transpose()?
    }
    .map_or(1.0, |(v, _)| v);
    if !p.is_exhausted() {
        return Err(());
    }
    let unit = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (r, g, b) = match name {
        "rgb" | "rgba" => {
            let c = |(v, percent): (f32, bool)| {
                if percent {
                    unit(v)
                } else {
                    v.clamp(0.0, 255.0).round() as u8
                }
            };
            (c(first), c(second), c(third))
        }
        "hsl" | "hsla" => {
            let fraction = |(v, percent): (f32, bool)| if percent { v } else { v / 100.0 };
            let (h, s, l) = (first.0.rem_euclid(360.0), fraction(second), fraction(third));
            let (s, l) = (s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
            let f = |n: f32| {
                let k = (n + h / 30.0) % 12.0;
                let a = s * l.min(1.0 - l);
                l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
            };
            (unit(f(0.0)), unit(f(8.0)), unit(f(4.0)))
        }
        "hwb" => {
            let fraction = |(v, percent): (f32, bool)| if percent { v } else { v / 100.0 };
            let (h, mut w, mut bl) = (first.0.rem_euclid(360.0), fraction(second), fraction(third));
            if w + bl >= 1.0 {
                let gray = w / (w + bl);
                (unit(gray), unit(gray), unit(gray))
            } else {
                w = w.max(0.0);
                bl = bl.max(0.0);
                let f = |n: f32| {
                    let k = (n + h / 30.0) % 12.0;
                    let base = 0.5 - 0.5 * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0);
                    base * (1.0 - w - bl) + w
                };
                (unit(f(0.0)), unit(f(8.0)), unit(f(4.0)))
            }
        }
        "oklab" | "oklch" | "lab" | "lch" => {
            let lightness = if name.starts_with("ok") {
                // OKLab L is 0..1; percentages map 100% to 1.
                first.0
            } else if first.1 {
                first.0 * 100.0
            } else {
                first.0
            };
            let scale = match name {
                "oklab" | "oklch" => 0.4,
                "lab" => 125.0,
                _ => 150.0,
            };
            let axis = |(v, percent): (f32, bool)| if percent { v * scale } else { v };
            let (a, b) = if name.ends_with("ch") {
                let (chroma, hue) = (axis(second).max(0.0), third.0.to_radians());
                (chroma * hue.cos(), chroma * hue.sin())
            } else {
                (axis(second), axis(third))
            };
            let [r, g, b] = if name.starts_with("ok") {
                oklab_to_srgb(lightness, a, b)
            } else {
                lab_to_srgb(lightness, a, b)
            };
            (unit(r), unit(g), unit(b))
        }
        _ => return Err(()),
    };
    Ok(Color {
        r,
        g,
        b,
        a: unit(alpha),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
    Px(f64),
    Percent(f64),
    Em(f64),
    Rem(f64),
    Calc([f64; 4]),
    /// `clamp(lo, value, hi)` over linear `[px, %, em, rem]` terms; `min()` and
    /// `max()` use an infinite bound.
    Clamp([[f32; 4]; 3]),
    Vw(f64),
    Vh(f64),
    #[default]
    Auto,
}

impl Length {
    pub fn parse(s: &str) -> Option<Self> {
        let lower = s.trim().to_ascii_lowercase();
        let s = lower.as_str();
        if s.eq_ignore_ascii_case("auto") {
            return Some(Self::Auto);
        }
        if s == "0" || s == "0px" {
            return Some(Self::Px(0.0));
        }
        let split = s
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
            .map(|i| {
                // An exponent `e` followed by a unit letter belongs to the unit (`em`, `ex`).
                if i > 0 && s[..i].ends_with(['e', 'E']) && !s[i..].starts_with(['+', '-']) {
                    i - 1
                } else {
                    i
                }
            })
            .unwrap_or(s.len());
        let (number, unit) = s.split_at(split);
        if let Ok(n) = number.trim().parse::<f64>()
            && n.is_finite()
        {
            // Font-relative `ex`/`ch` approximate half an em; small/large/dynamic
            // viewport units read the current viewport.
            return match unit.trim() {
                "px" => Some(Self::Px(n)),
                "%" => Some(Self::Percent(n)),
                "em" => Some(Self::Em(n)),
                "rem" => Some(Self::Rem(n)),
                "ex" | "ch" | "cap" | "ic" => Some(Self::Em(n * 0.5)),
                "lh" => Some(Self::Em(n * 1.25)),
                "rlh" => Some(Self::Rem(n * 1.25)),
                "vw" | "svw" | "lvw" | "dvw" | "vi" | "svi" | "lvi" | "dvi" | "vmax" | "svmax"
                | "lvmax" | "dvmax" => Some(Self::Vw(n)),
                "vh" | "svh" | "lvh" | "dvh" | "vb" | "svb" | "lvb" | "dvb" | "vmin" | "svmin"
                | "lvmin" | "dvmin" => Some(Self::Vh(n)),
                "pt" => Some(Self::Px(n * 96.0 / 72.0)),
                "pc" => Some(Self::Px(n * 16.0)),
                "in" => Some(Self::Px(n * 96.0)),
                "cm" => Some(Self::Px(n * 96.0 / 2.54)),
                "mm" => Some(Self::Px(n * 96.0 / 25.4)),
                "q" => Some(Self::Px(n * 96.0 / 101.6)),
                _ => None,
            };
        }
        if s.starts_with("calc(")
            || s.starts_with("min(")
            || s.starts_with("max(")
            || s.starts_with("clamp(")
        {
            return super::syntax::calc_length(s);
        }
        None
    }

    pub fn resolve(&self, parent_len: f64, font_size: f64, root_font_size: f64) -> f64 {
        let value = match *self {
            Self::Px(v) => v,
            Self::Percent(pct) => parent_len * (pct / 100.0),
            Self::Em(em) => em * font_size,
            Self::Rem(rem) => rem * root_font_size,
            Self::Calc(v) => {
                v[0] + v[1] * parent_len / 100.0 + v[2] * font_size + v[3] * root_font_size
            }
            Self::Clamp([lo, value, hi]) => {
                let linear = |v: [f32; 4]| {
                    let v = v.map(f64::from);
                    v[0] + v[1] * parent_len / 100.0 + v[2] * font_size + v[3] * root_font_size
                };
                linear(value).min(linear(hi)).max(linear(lo))
            }
            Self::Vw(_) | Self::Vh(_) => 0.0, // converted to px before layout
            Self::Auto => 0.0,
        };
        if value.is_nan() {
            0.0
        } else {
            value.clamp(-16_777_216.0, 16_777_216.0)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Display {
    None,
    #[default]
    Inline,
    Block,
    InlineBlock,
    Flex,
    Grid,
    Table,
    TableRow,
    TableCell,
    InlineFlex,
    InlineGrid,
    InlineTable,
    ListItem,
    FlowRoot,
    /// The element generates no box; its children take its place.
    Contents,
    TableRowGroup,
    TableHeaderGroup,
    TableFooterGroup,
    TableCaption,
    TableColumn,
    TableColumnGroup,
}

impl Display {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "inline" => Some(Self::Inline),
            "block" => Some(Self::Block),
            "inline-block" => Some(Self::InlineBlock),
            "flex" => Some(Self::Flex),
            "grid" => Some(Self::Grid),
            "table" => Some(Self::Table),
            "table-row" => Some(Self::TableRow),
            "table-cell" => Some(Self::TableCell),
            "inline-flex" => Some(Self::InlineFlex),
            "inline-grid" => Some(Self::InlineGrid),
            "inline-table" => Some(Self::InlineTable),
            "list-item" | "block list-item" => Some(Self::ListItem),
            "flow-root" | "block flow-root" => Some(Self::FlowRoot),
            "contents" => Some(Self::Contents),
            "table-row-group" => Some(Self::TableRowGroup),
            "table-header-group" => Some(Self::TableHeaderGroup),
            "table-footer-group" => Some(Self::TableFooterGroup),
            "table-caption" => Some(Self::TableCaption),
            "table-column" => Some(Self::TableColumn),
            "table-column-group" => Some(Self::TableColumnGroup),
            "block flow" => Some(Self::Block),
            "inline flow" => Some(Self::Inline),
            "inline flow-root" => Some(Self::InlineBlock),
            "block flex" => Some(Self::Flex),
            "inline flex" => Some(Self::InlineFlex),
            "block grid" => Some(Self::Grid),
            "inline grid" => Some(Self::InlineGrid),
            _ => None,
        }
    }

    pub fn is_block_level(&self) -> bool {
        matches!(
            self,
            Self::Block
                | Self::Flex
                | Self::Grid
                | Self::Table
                | Self::ListItem
                | Self::FlowRoot
                | Self::TableCaption
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

impl Position {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "static" => Some(Self::Static),
            "relative" => Some(Self::Relative),
            "absolute" => Some(Self::Absolute),
            "fixed" => Some(Self::Fixed),
            "sticky" => Some(Self::Sticky),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoxSizing {
    #[default]
    ContentBox,
    BorderBox,
}

impl BoxSizing {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "content-box" => Some(Self::ContentBox),
            "border-box" => Some(Self::BorderBox),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overflow {
    #[default]
    Visible,
    Hidden,
    Clip,
    Scroll,
    Auto,
}

impl Overflow {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "visible" => Some(Self::Visible),
            "hidden" => Some(Self::Hidden),
            "clip" => Some(Self::Clip),
            "scroll" => Some(Self::Scroll),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Right,
    Center,
    Justify,
}

impl TextAlign {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "left" | "start" => Some(Self::Left),
            "right" | "end" => Some(Self::Right),
            "center" => Some(Self::Center),
            "justify" => Some(Self::Justify),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ObjectFit {
    #[default]
    Fill,
    Contain,
    Cover,
    None,
    ScaleDown,
}

impl ObjectFit {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fill" => Some(Self::Fill),
            "contain" => Some(Self::Contain),
            "cover" => Some(Self::Cover),
            "none" => Some(Self::None),
            "scale-down" => Some(Self::ScaleDown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Top,
    Middle,
    Bottom,
    TextTop,
    TextBottom,
    Sub,
    Super,
}

impl VerticalAlign {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "baseline" => Some(Self::Baseline),
            "top" => Some(Self::Top),
            "middle" => Some(Self::Middle),
            "bottom" => Some(Self::Bottom),
            "text-top" => Some(Self::TextTop),
            "text-bottom" => Some(Self::TextBottom),
            "sub" => Some(Self::Sub),
            "super" => Some(Self::Super),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlexDirection {
    #[default]
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

impl FlexDirection {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "row" => Some(Self::Row),
            "row-reverse" => Some(Self::RowReverse),
            "column" => Some(Self::Column),
            "column-reverse" => Some(Self::ColumnReverse),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JustifyContent {
    #[default]
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

impl JustifyContent {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "flex-start" | "start" => Some(Self::FlexStart),
            "flex-end" | "end" => Some(Self::FlexEnd),
            "center" => Some(Self::Center),
            "space-between" => Some(Self::SpaceBetween),
            "space-around" => Some(Self::SpaceAround),
            "space-evenly" => Some(Self::SpaceEvenly),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlignItems {
    #[default]
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
}

impl AlignItems {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stretch" => Some(Self::Stretch),
            "flex-start" | "start" => Some(Self::FlexStart),
            "flex-end" | "end" => Some(Self::FlexEnd),
            "center" => Some(Self::Center),
            "baseline" => Some(Self::Baseline),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectLengths {
    pub top: Length,
    pub right: Length,
    pub bottom: Length,
    pub left: Length,
}

impl Default for RectLengths {
    fn default() -> Self {
        Self {
            top: Length::Px(0.0),
            right: Length::Px(0.0),
            bottom: Length::Px(0.0),
            left: Length::Px(0.0),
        }
    }
}
impl RectLengths {
    pub fn parse_quad(s: &str) -> Self {
        let parts: Vec<&str> = s.split_whitespace().collect();
        match parts.len() {
            1 => {
                let v = Length::parse(parts[0]).unwrap_or_default();
                Self {
                    top: v,
                    right: v,
                    bottom: v,
                    left: v,
                }
            }
            2 => {
                let tb = Length::parse(parts[0]).unwrap_or_default();
                let lr = Length::parse(parts[1]).unwrap_or_default();
                Self {
                    top: tb,
                    right: lr,
                    bottom: tb,
                    left: lr,
                }
            }
            3 => {
                let top = Length::parse(parts[0]).unwrap_or_default();
                let lr = Length::parse(parts[1]).unwrap_or_default();
                let bottom = Length::parse(parts[2]).unwrap_or_default();
                Self {
                    top,
                    right: lr,
                    bottom,
                    left: lr,
                }
            }
            4.. => {
                let top = Length::parse(parts[0]).unwrap_or_default();
                let right = Length::parse(parts[1]).unwrap_or_default();
                let bottom = Length::parse(parts[2]).unwrap_or_default();
                let left = Length::parse(parts[3]).unwrap_or_default();
                Self {
                    top,
                    right,
                    bottom,
                    left,
                }
            }
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BorderSide {
    pub width: f64,
    pub color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub matrix: [f64; 6],
}

impl Transform {
    pub fn identity() -> Self {
        Self {
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }

    pub fn to_kurbo(&self) -> kurbo::Affine {
        kurbo::Affine::new(self.matrix)
    }

    pub fn parse(s: &str) -> Option<Self> {
        let mut rest = s.trim();
        let mut matrix = kurbo::Affine::IDENTITY;
        let mut count = 0;
        while !rest.is_empty() {
            count += 1;
            if count > 32 {
                return None;
            }
            let open = rest.find('(')?;
            let close = rest.find(')')?;
            let name = rest[..open].trim();
            let args = rest[open + 1..close].replace(',', " ");
            let values: Vec<_> = args.split_whitespace().collect();
            let number = |i: usize| values.get(i)?.parse::<f64>().ok().filter(|v| v.is_finite());
            let length = |i: usize| {
                let v = *values.get(i)?;
                v.strip_suffix("px")
                    .unwrap_or(v)
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
            };
            let angle = |i: usize| {
                let v = *values.get(i)?;
                if let Some(v) = v.strip_suffix("deg") {
                    v.parse::<f64>().ok().map(f64::to_radians)
                } else if let Some(v) = v.strip_suffix("rad") {
                    v.parse::<f64>().ok()
                } else if let Some(v) = v.strip_suffix("turn") {
                    v.parse::<f64>().ok().map(|v| v * std::f64::consts::TAU)
                } else if v == "0" {
                    Some(0.0)
                } else {
                    None
                }
            };
            let transform = match (name, values.len()) {
                ("matrix", 6) => kurbo::Affine::new([
                    number(0)?,
                    number(1)?,
                    number(2)?,
                    number(3)?,
                    number(4)?,
                    number(5)?,
                ]),
                ("translate", 1 | 2) => kurbo::Affine::translate((
                    length(0)?,
                    if values.len() == 2 { length(1)? } else { 0.0 },
                )),
                ("translateX", 1) => kurbo::Affine::translate((length(0)?, 0.0)),
                ("translateY", 1) => kurbo::Affine::translate((0.0, length(0)?)),
                ("scale", 1 | 2) => kurbo::Affine::scale_non_uniform(
                    number(0)?,
                    if values.len() == 2 {
                        number(1)?
                    } else {
                        number(0)?
                    },
                ),
                ("scaleX", 1) => kurbo::Affine::scale_non_uniform(number(0)?, 1.0),
                ("scaleY", 1) => kurbo::Affine::scale_non_uniform(1.0, number(0)?),
                ("rotate", 1) => kurbo::Affine::rotate(angle(0)?),
                ("skewX", 1) => kurbo::Affine::new([1.0, 0.0, angle(0)?.tan(), 1.0, 0.0, 0.0]),
                ("skewY", 1) => kurbo::Affine::new([1.0, angle(0)?.tan(), 0.0, 1.0, 0.0, 0.0]),
                _ => return None,
            };
            matrix *= transform;
            rest = rest[close + 1..].trim();
        }
        let matrix = matrix.as_coeffs();
        (count > 0 && matrix.iter().all(|v| v.is_finite())).then_some(Self { matrix })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    pub offset_x: f64,
    pub offset_y: f64,
    pub blur_radius: f64,
    pub spread: f64,
    pub color: Color,
}

impl BoxShadow {
    /// The first outer shadow layer; inset layers are not painted yet.
    pub fn parse(s: &str) -> Option<Self> {
        let mut depth = 0usize;
        let mut start = 0;
        let mut layers = Vec::new();
        for (i, ch) in s.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    layers.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        layers.push(&s[start..]);
        layers
            .into_iter()
            .filter(|l| !l.to_ascii_lowercase().contains("inset"))
            .find_map(Self::parse_layer)
    }
    fn parse_layer(s: &str) -> Option<Self> {
        let mut parser = cssparser::Parser::new(s.trim());
        let mut lengths = Vec::new();
        let mut color = Color::BLACK;
        while !parser.is_exhausted() {
            let start = parser.position();
            let token = parser.next().ok()?.clone();
            match token {
                cssparser::Token::Dimension { .. } => {
                    lengths.push(match Length::parse(parser.slice_from(start))? {
                        Length::Px(v) => v,
                        Length::Em(v) | Length::Rem(v) => v * 16.0,
                        _ => return None,
                    })
                }
                cssparser::Token::Number { value: 0.0, .. } => lengths.push(0.0),
                cssparser::Token::Function(_) => {
                    parser
                        .parse_nested_block(|p| {
                            while p.next().is_ok() {}
                            Ok::<_, cssparser::ParseError<()>>(())
                        })
                        .ok()?;
                    color = Color::parse(parser.slice_from(start))?;
                }
                cssparser::Token::Ident(ref i) if i.eq_ignore_ascii_case("currentcolor") => {}
                _ => color = Color::parse(parser.slice_from(start))?,
            }
        }
        if !(2..=4).contains(&lengths.len())
            || lengths.iter().any(|v| !v.is_finite())
            || lengths.get(2).is_some_and(|b| *b < 0.0)
        {
            return None;
        }
        Some(Self {
            offset_x: lengths[0],
            offset_y: lengths[1],
            blur_radius: *lengths.get(2).unwrap_or(&0.0),
            spread: *lengths.get(3).unwrap_or(&0.0),
            color,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Borders {
    pub top: BorderSide,
    pub right: BorderSide,
    pub bottom: BorderSide,
    pub left: BorderSide,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub position: Position,
    pub box_sizing: BoxSizing,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    pub width: Length,
    pub height: Length,
    pub min_width: Length,
    pub max_width: Length,
    pub min_height: Length,
    pub max_height: Length,
    pub margin: RectLengths,
    pub padding: RectLengths,
    pub borders: Borders,
    pub border_hidden: [bool; 4],
    pub top: Length,
    pub right: Length,
    pub bottom: Length,
    pub left: Length,
    pub z_index: Option<i32>,
    pub opacity: f32,
    pub visible: bool,
    pub pointer_events: bool,
    pub cursor: String,
    pub color: Color,
    pub background_color: Color,
    pub background_image: Option<String>,
    pub background_position: (Length, Length),
    pub background_size: super::graphics::BackgroundSize,
    pub background_repeat: (bool, bool),
    pub clip_path: Option<super::graphics::ClipPath>,
    pub custom_properties: std::collections::BTreeMap<String, String>,
    pub line_height_factor: Option<f64>,
    pub font_size: f64,
    /// Computed root font metric for rem units; propagated with style context.
    pub root_font_size: f64,
    pub line_height: f64,
    pub text_align: TextAlign,
    pub font_family: String,
    pub font_weight: f32,
    pub font_italic: bool,
    pub white_space: WhiteSpace,
    pub flex_direction: FlexDirection,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
    pub flex_grow: f64,
    pub flex_shrink: f64,
    pub flex_basis: Length,
    pub flex_wrap: bool,
    pub row_gap: f64,
    pub column_gap: f64,
    pub grid_template_columns: Vec<GridTrack>,
    pub grid_template_rows: Vec<GridTrack>,
    pub grid_column_start: Option<i32>,
    pub grid_row_start: Option<i32>,
    pub grid_column_span: usize,
    pub grid_row_span: usize,
    /// Explicit end lines (negative counts from the end), resolved at layout.
    pub grid_column_end: Option<i32>,
    pub grid_row_end: Option<i32>,
    /// Named area placement per axis (`grid-area: main`, `grid-column: main`).
    pub grid_column_name: Option<String>,
    pub grid_row_name: Option<String>,
    pub grid_template_areas: Vec<Vec<String>>,
    pub border_spacing: f64,
    pub border_radius: f64,
    /// Percentage part of `border-radius`, resolved against the border box.
    pub border_radius_percent: f64,
    pub transform: Option<Transform>,
    pub transform_origin: (Length, Length),
    pub box_shadow: Option<BoxShadow>,
    pub object_fit: ObjectFit,
    pub object_position: (Length, Length),
    pub vertical_align: VerticalAlign,
    pub float: Float,
    /// `mask-image` and its size/position/repeat. Masks are not composited
    /// yet; an image mask is painted as the background instead (icon fonts
    /// drawn with `mask-image` over `background-color` then keep their shape).
    pub mask_image: Option<String>,
    /// Topmost `linear-gradient()` background layer.
    pub background_gradient: Option<LinearGradient>,
    /// `background-clip: text`.
    pub background_clip_text: bool,
    pub mask_size: Option<super::graphics::BackgroundSize>,
    pub mask_position: Option<(Length, Length)>,
    pub mask_repeat: Option<(bool, bool)>,
    /// `clear`: (left, right).
    pub clear: (bool, bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinearGradient {
    /// CSS gradient angle in degrees (0 = to top, 90 = to right).
    pub angle: f32,
    /// Color stops with optional positions as fractions of the gradient line.
    pub stops: Vec<(Color, Option<f32>)>,
}
impl LinearGradient {
    /// Stops with every position resolved, in increasing order.
    pub fn resolved_stops(&self) -> Vec<(f32, Color)> {
        let n = self.stops.len();
        let mut positions: Vec<Option<f32>> = self.stops.iter().map(|s| s.1).collect();
        if let Some(first) = positions.first_mut() {
            first.get_or_insert(0.0);
        }
        if let Some(last) = positions.last_mut() {
            last.get_or_insert(1.0);
        }
        // Clamp to be non-decreasing, then spread unpositioned stops evenly.
        let mut max = 0.0f32;
        for p in positions.iter_mut().flatten() {
            *p = p.max(max);
            max = *p;
        }
        let mut i = 0;
        while i < n {
            if positions[i].is_none() {
                let start = i - 1;
                let end = (i..n).find(|j| positions[*j].is_some()).unwrap_or(n - 1);
                let (a, b) = (
                    positions[start].unwrap_or(0.0),
                    positions[end].unwrap_or(1.0),
                );
                for (k, p) in positions.iter_mut().enumerate().take(end).skip(i) {
                    *p = Some(a + (b - a) * (k - start) as f32 / (end - start) as f32);
                }
                i = end;
            }
            i += 1;
        }
        positions
            .into_iter()
            .zip(&self.stops)
            .map(|(p, (c, _))| (p.unwrap_or(0.0), *c))
            .collect()
    }
    /// The color halfway along the gradient line.
    pub fn midpoint(&self) -> Color {
        let stops = self.resolved_stops();
        let mut previous = stops[0];
        for &(p, c) in &stops {
            if p >= 0.5 {
                let span = (p - previous.0).max(f32::EPSILON);
                let t = ((0.5 - previous.0) / span).clamp(0.0, 1.0);
                let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
                return Color {
                    r: mix(previous.1.r, c.r),
                    g: mix(previous.1.g, c.g),
                    b: mix(previous.1.b, c.b),
                    a: mix(previous.1.a, c.a),
                };
            }
            previous = (p, c);
        }
        previous.1
    }
}
/// Parse `linear-gradient(...)` (or `repeating-linear-gradient`, painted once).
fn linear_gradient(style: &ComputedStyle, val: &str) -> Option<LinearGradient> {
    let lower = val.trim().to_ascii_lowercase();
    let inner = lower
        .strip_prefix("linear-gradient(")
        .or_else(|| lower.strip_prefix("repeating-linear-gradient("))?
        .strip_suffix(')')?;
    let mut args = Vec::new();
    let (mut depth, mut start) = (0usize, 0);
    for (i, ch) in inner.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                args.push(inner[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    args.push(inner[start..].trim());
    let mut angle = 180.0f32;
    let mut first_stop = 0;
    // A leading direction and/or `in <colorspace>` interpolation clause.
    let head = args[0];
    let head_words: Vec<&str> = components(head);
    if head_words.first().is_some_and(|w| *w == "to" || *w == "in")
        || Length::parse(head_words.first().copied().unwrap_or("")).is_none()
            && head_words
                .first()
                .is_some_and(|w| w.ends_with("deg") || w.ends_with("turn") || w.ends_with("rad"))
    {
        first_stop = 1;
        let mut words = head_words.iter().copied().peekable();
        while let Some(w) = words.next() {
            match w {
                "to" => {
                    let mut sides = Vec::new();
                    while let Some(side) =
                        words.next_if(|s| matches!(*s, "top" | "bottom" | "left" | "right"))
                    {
                        sides.push(side);
                    }
                    let has = |s: &str| sides.contains(&s);
                    let x = if has("right") {
                        1.0
                    } else if has("left") {
                        -1.0
                    } else {
                        0.0
                    };
                    let y = if has("top") {
                        1.0
                    } else if has("bottom") {
                        -1.0
                    } else {
                        0.0
                    };
                    angle = (f32::atan2(x, y)).to_degrees().rem_euclid(360.0);
                }
                "in" => {
                    words.next();
                    let _ = words.next_if(|w| {
                        matches!(*w, "shorter" | "longer" | "increasing" | "decreasing")
                    });
                    let _ = words.next_if(|w| *w == "hue");
                }
                w => {
                    let number =
                        |suffix: &str| w.strip_suffix(suffix).and_then(|n| n.parse::<f32>().ok());
                    if let Some(d) = number("deg") {
                        angle = d;
                    } else if let Some(t) = number("turn") {
                        angle = t * 360.0;
                    } else if let Some(r) = number("rad") {
                        angle = r.to_degrees();
                    } else if let Some(g) = number("grad") {
                        angle = g * 0.9;
                    }
                }
            }
        }
    }
    let mut stops = Vec::new();
    for arg in &args[first_stop..] {
        let parts = components(arg);
        let Some(color) = parts.first().and_then(|c| color_value(style, c)) else {
            // A bare transition hint (`50%`) or an unknown stop.
            continue;
        };
        let positions: Vec<Option<f32>> = parts[1..]
            .iter()
            .map(|p| match Length::parse(p) {
                Some(Length::Percent(v)) => Some(v as f32 / 100.0),
                _ => None,
            })
            .collect();
        match positions.as_slice() {
            [] => stops.push((color, None)),
            [a] => stops.push((color, *a)),
            [a, b, ..] => {
                stops.push((color, *a));
                stops.push((color, *b));
            }
        }
    }
    (stops.len() >= 2 || stops.len() == 1).then(|| {
        if stops.len() == 1 {
            stops.push(stops[0]);
        }
        LinearGradient { angle, stops }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Float {
    #[default]
    None,
    Left,
    Right,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            display: Display::Inline,
            position: Position::Static,
            box_sizing: BoxSizing::ContentBox,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            width: Length::Auto,
            height: Length::Auto,
            min_width: Length::Auto,
            max_width: Length::Auto,
            min_height: Length::Auto,
            max_height: Length::Auto,
            margin: RectLengths::default(),
            padding: RectLengths::default(),
            borders: Borders::default(),
            border_hidden: [false; 4],
            top: Length::Auto,
            right: Length::Auto,
            bottom: Length::Auto,
            left: Length::Auto,
            z_index: None,
            opacity: 1.0,
            visible: true,
            pointer_events: true,
            cursor: "auto".into(),
            color: Color::BLACK,
            background_color: Color::TRANSPARENT,
            background_image: None,
            background_position: (Length::Percent(0.0), Length::Percent(0.0)),
            background_size: super::graphics::BackgroundSize::Auto,
            background_repeat: (true, true),
            clip_path: None,
            custom_properties: Default::default(),
            line_height_factor: Some(1.25),
            font_size: 16.0,
            root_font_size: 16.0,
            line_height: 20.0,
            text_align: TextAlign::Left,
            font_family: "sans-serif".into(),
            font_weight: 400.0,
            font_italic: false,
            white_space: WhiteSpace::Normal,
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Stretch,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Length::Auto,
            flex_wrap: false,
            row_gap: 0.0,
            column_gap: 0.0,
            grid_template_columns: Vec::new(),
            grid_template_rows: Vec::new(),
            grid_column_start: None,
            grid_row_start: None,
            grid_column_span: 1,
            grid_row_span: 1,
            grid_column_end: None,
            grid_row_end: None,
            grid_column_name: None,
            grid_row_name: None,
            grid_template_areas: Vec::new(),
            border_spacing: 2.0,
            border_radius: 0.0,
            border_radius_percent: 0.0,
            transform: None,
            transform_origin: (Length::Percent(50.0), Length::Percent(50.0)),
            box_shadow: None,
            object_fit: ObjectFit::Fill,
            object_position: (Length::Percent(50.0), Length::Percent(50.0)),
            vertical_align: VerticalAlign::Baseline,
            float: Float::None,
            mask_image: None,
            background_gradient: None,
            background_clip_text: false,
            mask_size: None,
            mask_position: None,
            mask_repeat: None,
            clear: (false, false),
        }
    }
}

// ---------------------------------------------------------------------------
// SELECTORS & SPECIFICITY
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleSelector {
    Universal,
    Type(String),
    Class(String),
    Id(String),
    AttrPresent(String),
    AttrEquals(String, String),
    AttrMatch(String, String, char, bool),
    Root,
    Hover,
    Focus,
    FocusWithin,
    Active,
    Checked,
    Disabled,
    Enabled,
    Link,
    Empty,
    FirstChild,
    LastChild,
    OnlyChild,
    NthChild(i32, i32, bool),
    Is(Vec<ComplexSelector>),
    Where(Vec<ComplexSelector>),
    Not(Vec<ComplexSelector>),
    /// `:has()` relative selectors, anchored at the subject element.
    Has(Vec<(Combinator, ComplexSelector)>),
    NthOfType(i32, i32, bool),
    Lang(String),
    Optional,
    ReadOnly(bool),
    PlaceholderShown,
    /// A pseudo-class that is valid but never matches here (`:visited`).
    Never,
    /// A pseudo-element (`::before`); never matches the originating element.
    PseudoElement(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
    Adjacent,
    Sibling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundSelector {
    pub components: Vec<SimpleSelector>,
}

impl CompoundSelector {
    pub fn specificity(&self) -> (usize, usize, usize) {
        let mut ids = 0;
        let mut classes = 0;
        let mut types = 0;
        for c in &self.components {
            match c {
                SimpleSelector::Id(_) => ids += 1,
                SimpleSelector::Class(_)
                | SimpleSelector::AttrPresent(_)
                | SimpleSelector::AttrEquals(_, _) => classes += 1,
                SimpleSelector::Type(_) => types += 1,
                SimpleSelector::Is(list) | SimpleSelector::Not(list) => {
                    let s = list
                        .iter()
                        .map(ComplexSelector::specificity)
                        .max()
                        .unwrap_or_default();
                    ids += s.0;
                    classes += s.1;
                    types += s.2;
                }
                SimpleSelector::Has(list) => {
                    let s = list
                        .iter()
                        .map(|(_, s)| s.specificity())
                        .max()
                        .unwrap_or_default();
                    ids += s.0;
                    classes += s.1;
                    types += s.2;
                }
                SimpleSelector::PseudoElement(_) => types += 1,
                SimpleSelector::Universal | SimpleSelector::Where(_) => {}
                _ => classes += 1,
            }
        }
        (ids, classes, types)
    }

    pub fn matches<D: DocumentView>(&self, doc: &D, node_key: NodeKey) -> bool {
        let Some(node) = doc.node(node_key) else {
            return false;
        };
        if node.kind() != NodeKind::Element {
            return false;
        };
        let tag = node
            .element_name()
            .map(|e| e.local.to_ascii_lowercase())
            .unwrap_or_default();

        for comp in &self.components {
            match comp {
                SimpleSelector::Universal => {}
                SimpleSelector::Type(name) => {
                    if !name.eq_ignore_ascii_case(&tag) {
                        return false;
                    }
                }
                SimpleSelector::Class(name) => {
                    let class_attr = node.attr("class").unwrap_or_default();
                    let has_class = class_attr.split_whitespace().any(|c| c == name);
                    if !has_class {
                        return false;
                    }
                }
                SimpleSelector::Id(name) => {
                    if node.attr("id") != Some(name.as_str()) {
                        return false;
                    }
                }
                SimpleSelector::AttrPresent(name) => {
                    if node.attr(name.as_str()).is_none() {
                        return false;
                    }
                }
                SimpleSelector::AttrEquals(name, val) => {
                    if node.attr(name.as_str()) != Some(val.as_str()) {
                        return false;
                    }
                }
                SimpleSelector::AttrMatch(name, value, op, insensitive) => {
                    let Some(attr) = node.attr(name) else {
                        return false;
                    };
                    let (attr, value) = if *insensitive {
                        (attr.to_ascii_lowercase(), value.to_ascii_lowercase())
                    } else {
                        (attr.to_owned(), value.clone())
                    };
                    let matched = match op {
                        '=' => attr == value,
                        '~' => {
                            !value.is_empty() && attr.split_ascii_whitespace().any(|v| v == value)
                        }
                        '|' => attr == value || attr.starts_with(&format!("{value}-")),
                        '^' => !value.is_empty() && attr.starts_with(&value),
                        '$' => !value.is_empty() && attr.ends_with(&value),
                        '*' => !value.is_empty() && attr.contains(&value),
                        _ => false,
                    };
                    if !matched {
                        return false;
                    }
                }
                SimpleSelector::Is(list) | SimpleSelector::Where(list) => {
                    if !list.iter().any(|s| s.matches(doc, node_key)) {
                        return false;
                    }
                }
                SimpleSelector::Not(list) => {
                    if list.iter().any(|s| s.matches(doc, node_key)) {
                        return false;
                    }
                }
                SimpleSelector::Never | SimpleSelector::PseudoElement(_) => return false,
                SimpleSelector::Has(list) => {
                    if !list
                        .iter()
                        .any(|(combinator, s)| has_relative(doc, node_key, *combinator, s))
                    {
                        return false;
                    }
                }
                SimpleSelector::Lang(range) => {
                    let mut current = Some(node_key);
                    let mut lang = None;
                    while let Some(k) = current {
                        let Some(n) = doc.node(k) else { break };
                        if let Some(l) = n.attr("lang") {
                            lang = Some(l.to_ascii_lowercase());
                            break;
                        }
                        current = n.parent();
                    }
                    if !lang.is_some_and(|l| l == *range || l.starts_with(&format!("{range}-"))) {
                        return false;
                    }
                }
                SimpleSelector::Optional | SimpleSelector::ReadOnly(_) => {
                    let form = matches!(tag.as_str(), "input" | "select" | "textarea");
                    let matched = match comp {
                        SimpleSelector::Optional => form && node.attr("required").is_none(),
                        SimpleSelector::ReadOnly(read_only) => {
                            let editable = matches!(tag.as_str(), "input" | "textarea")
                                && node.attr("readonly").is_none()
                                && node.attr("disabled").is_none()
                                || node
                                    .attr("contenteditable")
                                    .is_some_and(|v| !v.eq_ignore_ascii_case("false"));
                            editable != *read_only
                        }
                        _ => unreachable!(),
                    };
                    if !matched {
                        return false;
                    }
                }
                SimpleSelector::PlaceholderShown => {
                    if !matches!(tag.as_str(), "input" | "textarea")
                        || node.attr("placeholder").is_none()
                        || node.attr("value").is_some_and(|v| !v.is_empty())
                    {
                        return false;
                    }
                }
                SimpleSelector::Hover
                | SimpleSelector::Focus
                | SimpleSelector::FocusWithin
                | SimpleSelector::Active
                | SimpleSelector::Checked
                | SimpleSelector::Disabled
                | SimpleSelector::Enabled => {
                    let state = doc.element_state(node_key);
                    let matches = match comp {
                        SimpleSelector::Hover => state.hovered,
                        SimpleSelector::Focus => state.focused,
                        SimpleSelector::FocusWithin => state.focus_within,
                        SimpleSelector::Active => state.active,
                        SimpleSelector::Checked => state.checked,
                        SimpleSelector::Disabled => state.disabled,
                        SimpleSelector::Enabled => state.enabled,
                        _ => unreachable!(),
                    };
                    if !matches {
                        return false;
                    }
                }
                SimpleSelector::Link => {
                    if !matches!(tag.as_str(), "a" | "area") || node.attr("href").is_none() {
                        return false;
                    }
                }
                SimpleSelector::Root => {
                    if node
                        .parent()
                        .and_then(|p| doc.node(p))
                        .is_none_or(|p| p.kind() != NodeKind::Document)
                    {
                        return false;
                    }
                }
                SimpleSelector::Empty => {
                    if node.children().filter_map(|c| doc.node(c)).any(|c| {
                        c.kind() == NodeKind::Element || c.text().is_some_and(|t| !t.is_empty())
                    }) {
                        return false;
                    }
                }
                structural => {
                    let Some(parent) = node.parent().and_then(|p| doc.node(p)) else {
                        return false;
                    };
                    let of_type = matches!(structural, SimpleSelector::NthOfType(..));
                    let siblings: Vec<_> = parent
                        .children()
                        .filter(|k| {
                            doc.node(*k).is_some_and(|n| {
                                n.kind() == NodeKind::Element
                                    && (!of_type
                                        || n.element_name()
                                            .is_some_and(|e| e.local.eq_ignore_ascii_case(&tag)))
                            })
                        })
                        .collect();
                    let Some(index) = siblings.iter().position(|k| *k == node_key) else {
                        return false;
                    };
                    let index = index as i64 + 1;
                    let count = siblings.len() as i64;
                    let matches = match structural {
                        SimpleSelector::FirstChild => index == 1,
                        SimpleSelector::LastChild => index == count,
                        SimpleSelector::OnlyChild => count == 1,
                        SimpleSelector::NthChild(a, b, reverse)
                        | SimpleSelector::NthOfType(a, b, reverse) => {
                            let n = if *reverse { count - index + 1 } else { index } - *b as i64;
                            if *a == 0 {
                                n == 0
                            } else {
                                n % *a as i64 == 0 && n / *a as i64 >= 0
                            }
                        }
                        _ => false,
                    };
                    if !matches {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// Bounded `:has()` evaluation; large subtrees are cut off rather than
/// making every restyle quadratic.
fn has_relative<D: DocumentView>(
    doc: &D,
    subject: NodeKey,
    combinator: Combinator,
    selector: &ComplexSelector,
) -> bool {
    const LIMIT: usize = 4096;
    let Some(node) = doc.node(subject) else {
        return false;
    };
    match combinator {
        Combinator::Child => node.children().any(|c| selector.matches(doc, c)),
        Combinator::Descendant => {
            let mut stack: Vec<NodeKey> = node.children().collect();
            let mut seen = 0;
            while let Some(k) = stack.pop() {
                seen += 1;
                if seen > LIMIT {
                    return false;
                }
                if selector.matches(doc, k) {
                    return true;
                }
                if let Some(n) = doc.node(k) {
                    stack.extend(n.children());
                }
            }
            false
        }
        Combinator::Adjacent | Combinator::Sibling => {
            let Some(parent) = node.parent().and_then(|p| doc.node(p)) else {
                return false;
            };
            let following = parent
                .children()
                .skip_while(|k| *k != subject)
                .skip(1)
                .filter(|k| doc.node(*k).is_some_and(|n| n.kind() == NodeKind::Element));
            if combinator == Combinator::Adjacent {
                following.take(1).any(|k| selector.matches(doc, k))
            } else {
                following.into_iter().any(|k| selector.matches(doc, k))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplexSelector {
    pub items: Vec<(CompoundSelector, Option<Combinator>)>,
}

impl ComplexSelector {
    pub fn specificity(&self) -> (usize, usize, usize) {
        let mut total = (0, 0, 0);
        for (compound, _) in &self.items {
            let s = compound.specificity();
            total.0 += s.0;
            total.1 += s.1;
            total.2 += s.2;
        }
        total
    }

    pub fn matches<D: DocumentView>(&self, doc: &D, node_key: NodeKey) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let (last_compound, _) = self.items.last().unwrap();
        if !last_compound.matches(doc, node_key) {
            return false;
        }
        if self.items.len() == 1 {
            return true;
        }

        self.matches_at(doc, node_key, self.items.len() - 1)
    }
    fn matches_at<D: DocumentView>(&self, doc: &D, key: NodeKey, index: usize) -> bool {
        if !self.items[index].0.matches(doc, key) {
            return false;
        }
        if index == 0 {
            return true;
        }
        let combinator = self.items[index - 1].1.unwrap_or(Combinator::Descendant);
        let parent = || doc.node(key).and_then(NodeView::parent);
        match combinator {
            Combinator::Child => parent().is_some_and(|p| self.matches_at(doc, p, index - 1)),
            Combinator::Descendant => {
                let mut ancestor = parent();
                while let Some(p) = ancestor {
                    if self.matches_at(doc, p, index - 1) {
                        return true;
                    }
                    ancestor = doc.node(p).and_then(NodeView::parent);
                }
                false
            }
            Combinator::Adjacent | Combinator::Sibling => {
                let Some(parent) = parent().and_then(|p| doc.node(p)) else {
                    return false;
                };
                let previous: Vec<_> = parent
                    .children()
                    .take_while(|k| *k != key)
                    .filter(|k| doc.node(*k).is_some_and(|n| n.kind() == NodeKind::Element))
                    .collect();
                if combinator == Combinator::Adjacent {
                    previous
                        .last()
                        .is_some_and(|p| self.matches_at(doc, *p, index - 1))
                } else {
                    previous
                        .into_iter()
                        .rev()
                        .any(|p| self.matches_at(doc, p, index - 1))
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CSS PARSING (RULES & SELECTORS)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub property: String,
    pub value: String,
    pub important: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaQuery {
    pub negated: bool,
    pub media_type: Option<MediaType>,
    pub features: Vec<MediaFeature>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    All,
    Screen,
    Print,
    Speech,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MediaFeature {
    MinWidth(Length),
    MaxWidth(Length),
    Width(Length),
    MinHeight(Length),
    MaxHeight(Length),
    Height(Length),
    Orientation(Orientation),
    MinResolution(f64),
    MaxResolution(f64),
    Resolution(f64),
    PrefersColorScheme(ColorScheme),
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorScheme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy)]
pub struct MediaContext {
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub device_pixel_ratio: f64,
    pub dark_mode: bool,
}

impl Default for MediaContext {
    fn default() -> Self {
        Self {
            viewport_width: 800.0,
            viewport_height: 600.0,
            device_pixel_ratio: 1.0,
            dark_mode: false,
        }
    }
}

impl MediaQuery {
    pub fn matches(&self, ctx: &MediaContext) -> bool {
        let type_matches = match self.media_type {
            None | Some(MediaType::All | MediaType::Screen) => true,
            Some(MediaType::Print | MediaType::Speech) => false,
        };
        if !type_matches {
            return self.negated;
        }
        for feature in &self.features {
            let feat_matches = match feature {
                MediaFeature::MinWidth(len) => {
                    let px = len.resolve(ctx.viewport_width, 16.0, 16.0);
                    ctx.viewport_width >= px
                }
                MediaFeature::MaxWidth(len) => {
                    let px = len.resolve(ctx.viewport_width, 16.0, 16.0);
                    ctx.viewport_width <= px
                }
                MediaFeature::Width(len) => {
                    let px = len.resolve(ctx.viewport_width, 16.0, 16.0);
                    (ctx.viewport_width - px).abs() < 0.5
                }
                MediaFeature::MinHeight(len) => {
                    let px = len.resolve(ctx.viewport_height, 16.0, 16.0);
                    ctx.viewport_height >= px
                }
                MediaFeature::MaxHeight(len) => {
                    let px = len.resolve(ctx.viewport_height, 16.0, 16.0);
                    ctx.viewport_height <= px
                }
                MediaFeature::Height(len) => {
                    let px = len.resolve(ctx.viewport_height, 16.0, 16.0);
                    (ctx.viewport_height - px).abs() < 0.5
                }
                MediaFeature::Orientation(o) => {
                    let current = if ctx.viewport_width > ctx.viewport_height {
                        Orientation::Landscape
                    } else {
                        Orientation::Portrait
                    };
                    *o == current
                }
                MediaFeature::MinResolution(dppx) => ctx.device_pixel_ratio >= *dppx,
                MediaFeature::MaxResolution(dppx) => ctx.device_pixel_ratio <= *dppx,
                MediaFeature::Resolution(dppx) => (ctx.device_pixel_ratio - *dppx).abs() < 0.01,
                MediaFeature::PrefersColorScheme(cs) => match cs {
                    ColorScheme::Light => !ctx.dark_mode,
                    ColorScheme::Dark => ctx.dark_mode,
                },
                MediaFeature::Unknown(_) => false,
            };
            if !feat_matches {
                return self.negated;
            }
        }
        !self.negated
    }
}

pub fn matches_media(queries: &[MediaQuery], ctx: &MediaContext) -> bool {
    if queries.is_empty() {
        return true;
    }
    queries.iter().any(|q| q.matches(ctx))
}

/// Index of the `)` closing the `(` at the start of `s`.
fn closing_paren(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}
/// Media Queries 4 range syntax: `width >= 48rem`, `40em < width <= 64em`.
fn media_range(feature: &str) -> Option<Vec<MediaFeature>> {
    // Split into operands and comparison operators.
    let mut parts: Vec<String> = vec![String::new()];
    let mut ops = Vec::new();
    let mut chars = feature.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, '<' | '>' | '=') {
            let mut op = c.to_string();
            if c != '=' && chars.peek() == Some(&'=') {
                op.push(chars.next()?);
            }
            ops.push(op);
            parts.push(String::new());
        } else {
            parts.last_mut()?.push(c);
        }
    }
    let parts: Vec<String> = parts
        .iter()
        .map(|p| p.trim().to_ascii_lowercase())
        .collect();
    if ops.is_empty() || ops.len() != parts.len() - 1 || ops.len() > 2 {
        return None;
    }
    let feature_at = parts.iter().position(|p| p == "width" || p == "height")?;
    let is_width = parts[feature_at] == "width";
    // Strict comparisons shift the bound by a hundredth of a pixel.
    let shifted = |l: Length, delta: f64| match l {
        Length::Px(v) => Length::Px(v + delta),
        Length::Em(v) => Length::Calc([delta, 0.0, v, 0.0]),
        Length::Rem(v) => Length::Calc([delta, 0.0, 0.0, v]),
        Length::Calc(mut v) => {
            v[0] += delta;
            Length::Calc(v)
        }
        other => other,
    };
    let mut out = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        // Normalize to `feature OP value`.
        let (value, op) = if i == feature_at {
            (&parts[i + 1], op.clone())
        } else if i + 1 == feature_at {
            let flipped = match op.as_str() {
                "<" => ">",
                "<=" => ">=",
                ">" => "<",
                ">=" => "<=",
                other => other,
            };
            (&parts[i], flipped.to_owned())
        } else {
            return None;
        };
        let length = Length::parse(value)?;
        let feature = match (op.as_str(), is_width) {
            (">=", true) => MediaFeature::MinWidth(length),
            (">", true) => MediaFeature::MinWidth(shifted(length, 0.01)),
            ("<=", true) => MediaFeature::MaxWidth(length),
            ("<", true) => MediaFeature::MaxWidth(shifted(length, -0.01)),
            ("=", true) => MediaFeature::Width(length),
            (">=", false) => MediaFeature::MinHeight(length),
            (">", false) => MediaFeature::MinHeight(shifted(length, 0.01)),
            ("<=", false) => MediaFeature::MaxHeight(length),
            ("<", false) => MediaFeature::MaxHeight(shifted(length, -0.01)),
            ("=", false) => MediaFeature::Height(length),
            _ => return None,
        };
        out.push(feature);
    }
    Some(out)
}
pub fn parse_media_query_list(s: &str) -> Vec<MediaQuery> {
    s.split(',')
        .filter_map(|part| {
            let p = part.trim();
            if p.is_empty() {
                return None;
            }
            let mut negated = false;
            let mut media_type = None;
            let mut features = Vec::new();

            let mut remaining = p;
            while let Some(start) = remaining.find('(') {
                let prefix = remaining[..start].trim();
                if !prefix.is_empty() {
                    for word in prefix.split_whitespace() {
                        let w = word.to_ascii_lowercase();
                        if w == "not" {
                            negated = true;
                        } else if w == "only" {
                        } else if w == "all" {
                            media_type = Some(MediaType::All);
                        } else if w == "screen" {
                            media_type = Some(MediaType::Screen);
                        } else if w == "print" {
                            media_type = Some(MediaType::Print);
                        } else if w == "speech" {
                            media_type = Some(MediaType::Speech);
                        } else if w != "and" {
                            features.push(MediaFeature::Unknown(w));
                        }
                    }
                }
                if let Some(end) = closing_paren(&remaining[start..]) {
                    let feat_str = &remaining[start + 1..start + end];
                    if let Some((name, val)) = feat_str.split_once(':') {
                        let name = name.trim().to_ascii_lowercase();
                        let val = val.trim();
                        let feat = match name.as_str() {
                            "min-width" => Length::parse(val).map(MediaFeature::MinWidth),
                            "max-width" => Length::parse(val).map(MediaFeature::MaxWidth),
                            "width" => Length::parse(val).map(MediaFeature::Width),
                            "min-height" => Length::parse(val).map(MediaFeature::MinHeight),
                            "max-height" => Length::parse(val).map(MediaFeature::MaxHeight),
                            "height" => Length::parse(val).map(MediaFeature::Height),
                            "orientation" => match val.to_ascii_lowercase().as_str() {
                                "landscape" => {
                                    Some(MediaFeature::Orientation(Orientation::Landscape))
                                }
                                "portrait" => {
                                    Some(MediaFeature::Orientation(Orientation::Portrait))
                                }
                                _ => None,
                            },
                            "prefers-color-scheme" => match val.to_ascii_lowercase().as_str() {
                                "dark" => Some(MediaFeature::PrefersColorScheme(ColorScheme::Dark)),
                                "light" => {
                                    Some(MediaFeature::PrefersColorScheme(ColorScheme::Light))
                                }
                                _ => None,
                            },
                            "min-resolution" | "max-resolution" | "resolution" => {
                                let num = if let Some(dppx) =
                                    val.strip_suffix("dppx").or_else(|| val.strip_suffix("x"))
                                {
                                    dppx.trim().parse::<f64>().ok()
                                } else if let Some(dpi) = val.strip_suffix("dpi") {
                                    dpi.trim().parse::<f64>().ok().map(|d| d / 96.0)
                                } else {
                                    val.parse::<f64>().ok()
                                };
                                num.map(|n| match name.as_str() {
                                    "min-resolution" => MediaFeature::MinResolution(n),
                                    "max-resolution" => MediaFeature::MaxResolution(n),
                                    _ => MediaFeature::Resolution(n),
                                })
                            }
                            _ => Some(MediaFeature::Unknown(name.clone())),
                        };
                        if let Some(f) = feat {
                            features.push(f);
                        } else {
                            features.push(MediaFeature::Unknown(name));
                        }
                    } else if let Some(range) = media_range(feat_str) {
                        features.extend(range);
                    } else {
                        features.push(MediaFeature::Unknown(feat_str.into()));
                    }
                    remaining = &remaining[start + end + 1..];
                } else {
                    features.push(MediaFeature::Unknown(remaining.into()));
                    break;
                }
            }
            let tail = remaining.trim();
            if !tail.is_empty() {
                for word in tail.split_whitespace() {
                    let w = word.to_ascii_lowercase();
                    if w == "not" {
                        negated = true;
                    } else if w == "only" {
                    } else if w == "all" {
                        media_type = Some(MediaType::All);
                    } else if w == "screen" {
                        media_type = Some(MediaType::Screen);
                    } else if w == "print" {
                        media_type = Some(MediaType::Print);
                    } else if w == "speech" {
                        media_type = Some(MediaType::Speech);
                    } else if w != "and" {
                        features.push(MediaFeature::Unknown(w));
                    }
                }
            }
            Some(MediaQuery {
                negated,
                media_type,
                features,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct FontSourceSpec {
    pub url: String,
    pub format: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FontFaceRule {
    pub font_family: String,
    pub src: Vec<FontSourceSpec>,
    pub font_weight: Option<u16>,
    pub font_style: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportRule {
    pub url: String,
    pub media: Vec<MediaQuery>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub selectors: Vec<ComplexSelector>,
    pub declarations: Vec<Declaration>,
    pub media: Vec<Vec<MediaQuery>>,
    /// Cascade layer order within the sheet (`None` = unlayered).
    pub layer: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct StyleSheet {
    pub rules: Vec<Rule>,
    pub font_faces: Vec<FontFaceRule>,
    pub imports: Vec<ImportRule>,
    pub unsupported: Vec<String>,
    /// `@property` registrations: (name, initial value, inherits).
    pub registered_properties: Vec<(String, String, bool)>,
}

impl StyleSheet {
    pub fn parse(css: &str) -> Self {
        super::syntax::stylesheet(css)
    }
}
pub fn parse_declarations(s: &str) -> Vec<Declaration> {
    super::syntax::declarations(s)
}
pub fn parse_selector_list(s: &str) -> Vec<ComplexSelector> {
    super::syntax::selectors(s)
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

pub fn default_user_agent_stylesheet() -> StyleSheet {
    StyleSheet::parse(
        r#"
head, style, script, meta, link, title, template, base, area, datalist, noscript, param, rp, source, track, dialog:not([open]) { display: none; }
[hidden]:not([hidden="until-found" i]) {display:none;}
html { display: block; }
body { display: block; margin: 8px; font-size: 16px; line-height: normal; color: black; }
address, blockquote, center, div, figure, figcaption, footer, form, header, hr, legend, listing, main, p, plaintext, pre, search, xmp,
article, aside, h1, h2, h3, h4, h5, h6, hgroup, nav, section, dialog, details, summary, fieldset, optgroup, menu, dir, dd, dl, dt, ol, ul, picture-caption { display: block; }
span, a, b, strong, i, em, u, s, small, big, sub, sup, abbr, cite, code, kbd, samp, var, mark, q, time, label, font, bdi, bdo, data, output, del, ins { display: inline; }
li { display: list-item; }
summary { display: block; }
a[href] { color: #0000ee; text-decoration: underline; }
u, ins { text-decoration: underline; }
s, del, strike { text-decoration: line-through; }
table { display: table; border-collapse: separate; border-spacing: 2px; box-sizing: border-box; }
caption { display: table-caption; text-align: center; }
colgroup { display: table-column-group; }
col { display: table-column; }
thead { display: table-header-group; }
tbody { display: table-row-group; }
tfoot { display: table-footer-group; }
tr { display: table-row; }
td, th { display: table-cell; padding: 1px; }
th { font-weight: bold; text-align: center; }
img, video, canvas, svg, iframe, embed, object { display: inline-block; }
button, input, textarea, select, meter, progress { display: inline-block; }
input, textarea, select, button { font-size: 13.333px; font-family: sans-serif; line-height: normal; color: black; }
input, textarea { box-sizing: content-box; width: 147px; padding: 1px 2px; border: 1px solid #767676; background-color: white; }
select { box-sizing: border-box; height: 20px; padding: 0 2px; border: 1px solid #767676; background-color: white; }
button, input[type="submit" i], input[type="button" i], input[type="reset" i] { box-sizing: border-box; width: auto; padding: 1px 6px; border: 1px solid #767676; border-radius: 2px; background-color: #efefef; text-align: center; }
input[type="checkbox" i], input[type="radio" i] { box-sizing: border-box; width: 13px; height: 13px; margin: 3px 3px 3px 4px; padding: 0; border: 0; background-color: transparent; }
input[type="hidden" i] { display: none; }
textarea { white-space: pre-wrap; width: 180px; height: 32px; }
iframe { border: 2px inset #767676; width: 300px; height: 150px; }
dialog[open] { position: absolute; left: 0; right: 0; margin: auto; border: 1.5px solid black; padding: 1em; background-color: white; color: black; width: fit-content; }
hr { margin: 8px 0; border: 1px inset #9a9a9a; height: 0; }
fieldset { margin: 0 2px; padding: 6px 12px 10px; border: 2px groove #c0c0c0; }
h1 { margin: 21.44px 0; font-size: 32px; font-weight: bold; }
h2 { margin: 19.92px 0; font-size: 24px; font-weight: bold; }
h3 { margin: 18.72px 0; font-size: 18.72px; font-weight: bold; }
h4 { margin: 21.28px 0; font-weight: bold; }
h5 { margin: 22.18px 0; font-size: 13.28px; font-weight: bold; }
h6 { margin: 24.97px 0; font-size: 10.72px; font-weight: bold; }
p, blockquote, figure, dl { margin: 16px 0; }
blockquote, figure { margin-left: 40px; margin-right: 40px; }
dd { margin-left: 40px; }
ul, ol, menu, dir { margin: 16px 0; padding-left: 40px; }
ul ul, ul ol, ol ul, ol ol { margin-top: 0; margin-bottom: 0; }
b, strong, dt { font-weight: bold; }
i, em, cite, var, dfn, address { font-style: italic; }
small { font-size: 0.83em; }
big { font-size: 1.17em; }
sub { vertical-align: sub; font-size: 0.83em; }
sup { vertical-align: super; font-size: 0.83em; }
mark { background-color: yellow; color: black; }
pre, xmp, listing, plaintext { white-space: pre; font-family: monospace; margin: 13px 0; }
code, kbd, samp, tt { font-family: monospace; }
center { text-align: center; }
center > table, center > :not(table, tbody, tr, td) > table, :not(td, th, tr, table)[align="center" i] > table { margin-left: auto; margin-right: auto; text-align: left; }
"#,
    )
}

// ---------------------------------------------------------------------------
// CASCADE & STYLE RESOLUTION
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct MatchedDecl {
    property: String,
    value: String,
    origin: u8, // 0 = UA, 1 = Author, 2 = Inline
    important: bool,
    specificity: (usize, usize, usize),
    source_order: usize,
    /// Cascade layer rank for author rules; unlayered rules rank highest.
    layer: u32,
}

thread_local! {
    static USER_AGENT: StyleSheet = default_user_agent_stylesheet();
}
/// HTML dimension values: `120`, `120px`, `50%`. Zero-or-positive only.
fn html_dimension(value: &str) -> Option<String> {
    let v = value.trim();
    let digits = v.trim_end_matches(['%', 'x', 'p']).trim();
    let n: f64 = digits
        .parse()
        .ok()
        .filter(|n: &f64| n.is_finite() && *n >= 0.0)?;
    Some(if v.ends_with('%') {
        format!("{n}%")
    } else {
        format!("{n}px")
    })
}
fn html_color(value: &str) -> Option<String> {
    let v = value.trim();
    if Color::parse(v).is_some() {
        return Some(v.to_owned());
    }
    // Legacy `bgcolor="ffffff"` without the hash.
    (v.len() == 6 && v.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("#{v}"))
}
fn presentational_hints<D: DocumentView>(doc: &D, key: NodeKey) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let Some(node) = doc.node(key) else {
        return out;
    };
    let Some(tag) = node.element_name().map(|n| n.local.to_ascii_lowercase()) else {
        return out;
    };
    let attr = |name: &str| node.attr(name).filter(|v| !v.trim().is_empty());
    let tag = tag.as_str();
    if matches!(
        tag,
        "img"
            | "iframe"
            | "video"
            | "canvas"
            | "svg"
            | "embed"
            | "object"
            | "table"
            | "td"
            | "th"
            | "col"
            | "hr"
            | "input"
    ) {
        for property in ["width", "height"] {
            if (tag == "input" && attr("type").is_none_or(|t| !t.eq_ignore_ascii_case("image")))
                || (tag == "hr" && property == "height")
            {
                continue;
            }
            if let Some(v) = attr(property).and_then(html_dimension) {
                out.push((property, v));
            }
        }
    }
    if matches!(
        tag,
        "body" | "table" | "tr" | "td" | "th" | "thead" | "tbody" | "tfoot"
    ) && let Some(c) = attr("bgcolor").and_then(html_color)
    {
        out.push(("background-color", c));
    }
    if tag == "body"
        && let Some(c) = attr("text").and_then(html_color)
    {
        out.push(("color", c));
    }
    if let Some(align) = attr("align").map(str::to_ascii_lowercase) {
        match (tag, align.as_str()) {
            ("table", "center") => {
                out.push(("margin-left", "auto".into()));
                out.push(("margin-right", "auto".into()));
            }
            ("table" | "img" | "iframe" | "object" | "embed", "left" | "right") => {
                out.push(("float", align.clone()));
            }
            (
                "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "td" | "th" | "tr"
                | "thead" | "tbody" | "tfoot" | "caption" | "legend",
                "left" | "right" | "center" | "justify" | "middle",
            ) => {
                let value = if align == "middle" {
                    "center"
                } else {
                    align.as_str()
                };
                out.push(("text-align", value.into()));
            }
            _ => {}
        }
    }
    if matches!(tag, "td" | "th" | "tr" | "thead" | "tbody" | "tfoot")
        && let Some(v) = attr("valign").map(str::to_ascii_lowercase)
        && matches!(v.as_str(), "top" | "middle" | "bottom" | "baseline")
    {
        out.push(("vertical-align", v));
    }
    if matches!(tag, "td" | "th") && node.attr("nowrap").is_some() {
        out.push(("white-space", "nowrap".into()));
    }
    if tag == "font" {
        if let Some(c) = attr("color").and_then(html_color) {
            out.push(("color", c));
        }
        if let Some(face) = attr("face") {
            out.push(("font-family", face.to_owned()));
        }
        if let Some(size) = attr("size") {
            let size = size.trim();
            let n: i32 = size.trim_start_matches(['+', '-']).parse().unwrap_or(3);
            let n = if size.starts_with('+') {
                3 + n
            } else if size.starts_with('-') {
                3 - n
            } else {
                n
            };
            let px = [10.0, 13.0, 16.0, 18.0, 24.0, 32.0, 48.0][(n.clamp(1, 7) - 1) as usize];
            out.push(("font-size", format!("{px}px")));
        }
    }
    if tag == "table" {
        if let Some(spacing) = attr("cellspacing").and_then(html_dimension) {
            out.push(("border-spacing", spacing));
        }
        if let Some(border) = attr("border").and_then(html_dimension) {
            out.push(("border", format!("{border} outset #808080")));
        }
    }
    if matches!(tag, "td" | "th") {
        // Cells inherit padding and borders from the nearest table's attributes.
        let mut ancestor = node.parent();
        for _ in 0..3 {
            let Some(table) = ancestor.and_then(|k| doc.node(k)) else {
                break;
            };
            if table
                .element_name()
                .is_some_and(|n| n.local.eq_ignore_ascii_case("table"))
            {
                if let Some(padding) = table.attr("cellpadding").and_then(html_dimension) {
                    out.push(("padding", padding));
                }
                if table
                    .attr("border")
                    .and_then(|b| b.trim().parse::<f64>().ok())
                    .is_some_and(|b| b > 0.0)
                {
                    out.push(("border", "1px inset #808080".into()));
                }
                break;
            }
            ancestor = table.parent();
        }
    }
    if tag == "hr" && node.attr("noshade").is_some() {
        out.push(("border-style", "solid".into()));
    }
    out
}
thread_local! {
    /// Viewport of the style pass in progress, for viewport-relative terms
    /// inside `calc()`/`clamp()` and font sizes.
    static VIEWPORT: std::cell::Cell<(f64, f64)> = const { std::cell::Cell::new((800.0, 600.0)) };
}
pub(super) fn viewport() -> (f64, f64) {
    VIEWPORT.with(std::cell::Cell::get)
}
/// Resolve `vw`/`vh` against the current style pass's viewport.
pub fn viewport_px(l: Length) -> Length {
    let (w, h) = viewport();
    match l {
        Length::Vw(v) => Length::Px(v * w / 100.0),
        Length::Vh(v) => Length::Px(v * h / 100.0),
        l => l,
    }
}
pub fn resolve_computed_style<D: DocumentView>(
    doc: &D,
    node_key: NodeKey,
    parent_style: Option<&ComputedStyle>,
    author_sheets: &[&StyleSheet],
    media_ctx: Option<&MediaContext>,
) -> ComputedStyle {
    let default_ctx = MediaContext::default();
    let m_ctx = media_ctx.unwrap_or(&default_ctx);
    VIEWPORT.with(|v| v.set((m_ctx.viewport_width, m_ctx.viewport_height)));
    let mut style = parent_style.map_or_else(ComputedStyle::default, |p| ComputedStyle {
        // Inherited properties:
        custom_properties: p.custom_properties.clone(),
        line_height_factor: p.line_height_factor,
        color: p.color,
        visible: p.visible,
        pointer_events: p.pointer_events,
        cursor: p.cursor.clone(),
        font_size: p.font_size,
        root_font_size: p.root_font_size,
        line_height: p.line_height,
        text_align: p.text_align,
        font_family: p.font_family.clone(),
        font_weight: p.font_weight,
        font_italic: p.font_italic,
        white_space: p.white_space,
        ..Default::default()
    });

    let Some(node) = doc.node(node_key) else {
        return style;
    };
    if node.kind() != NodeKind::Element {
        return style;
    };

    let is_document_element = node
        .parent()
        .and_then(|p| doc.node(p))
        .is_some_and(|n| n.kind() == NodeKind::Document);
    // Registered custom properties (`@property`) start at their initial value:
    // on every element when they do not inherit, otherwise from the root.
    for sheet in author_sheets {
        for (name, initial, inherits) in &sheet.registered_properties {
            if !inherits || !style.custom_properties.contains_key(name) {
                style
                    .custom_properties
                    .insert(name.clone(), initial.clone());
            }
        }
    }
    let mut matched: Vec<MatchedDecl> = Vec::new();

    // 1. UA Stylesheet
    let mut order = 0;
    USER_AGENT.with(|ua| {
        for rule in &ua.rules {
            if !rule.media.iter().all(|group| matches_media(group, m_ctx)) {
                continue;
            }
            for sel in &rule.selectors {
                if sel.matches(doc, node_key) {
                    let spec = sel.specificity();
                    for d in &rule.declarations {
                        matched.push(MatchedDecl {
                            layer: u32::MAX,
                            property: d.property.clone(),
                            value: d.value.clone(),
                            origin: 0,
                            important: d.important,
                            specificity: spec,
                            source_order: order,
                        });
                        order += 1;
                    }
                }
            }
        }
    });

    // HTML presentational attributes map to author-level hints with zero
    // specificity that precede every authored rule (HTML §15 Rendering).
    for (property, value) in presentational_hints(doc, node_key) {
        matched.push(MatchedDecl {
            layer: u32::MAX,
            property: property.into(),
            value,
            origin: 1,
            important: false,
            specificity: (0, 0, 0),
            source_order: 0,
        });
    }
    // 2. Author Stylesheets
    for (sheet_index, sheet) in author_sheets.iter().enumerate() {
        for rule in &sheet.rules {
            let layer = rule.layer.map_or(u32::MAX, |l| {
                (sheet_index as u32)
                    .saturating_mul(4096)
                    .saturating_add(l.min(4095))
            });
            if !rule.media.iter().all(|group| matches_media(group, m_ctx)) {
                continue;
            }
            for sel in &rule.selectors {
                if sel.matches(doc, node_key) {
                    let spec = sel.specificity();
                    for d in &rule.declarations {
                        matched.push(MatchedDecl {
                            layer,
                            property: d.property.clone(),
                            value: d.value.clone(),
                            origin: 1,
                            important: d.important,
                            specificity: spec,
                            source_order: order,
                        });
                        order += 1;
                    }
                }
            }
        }
    }

    // 3. Inline style attribute
    if let Some(inline_css) = node.attr("style") {
        for d in parse_declarations(inline_css) {
            matched.push(MatchedDecl {
                layer: u32::MAX,
                property: d.property,
                value: d.value,
                origin: 2,
                important: d.important,
                specificity: (1, 0, 0), // Inline origin weight
                source_order: order,
            });
            order += 1;
        }
    }

    // Sort according to CSS Cascade:
    // 1. Important (true > false)
    // 2. Origin (Inline > Author > UA for normal; UA > Author > Inline for !important)
    // 3. Specificity
    // 4. Source order
    matched.sort_by(|a, b| {
        if a.important != b.important {
            return a.important.cmp(&b.important);
        }
        let (origin_a, origin_b) = if a.important {
            (
                if a.origin == 0 { 3 } else { a.origin },
                if b.origin == 0 { 3 } else { b.origin },
            )
        } else {
            (a.origin, b.origin)
        };
        if origin_a != origin_b {
            return origin_a.cmp(&origin_b);
        }
        // Cascade layers: later layers and then unlayered rules win for
        // normal declarations; the order reverses for !important ones.
        if a.layer != b.layer {
            return if a.important {
                b.layer.cmp(&a.layer)
            } else {
                a.layer.cmp(&b.layer)
            };
        }
        if a.specificity != b.specificity {
            return a.specificity.cmp(&b.specificity);
        }
        a.source_order.cmp(&b.source_order)
    });

    // Resolve custom properties before ordinary properties, independent of order.
    for decl in &matched {
        if decl.property.starts_with("--") {
            match decl.value.trim() {
                "initial" => {
                    style.custom_properties.remove(&decl.property);
                }
                "inherit" | "unset" => {
                    if let Some(value) =
                        parent_style.and_then(|p| p.custom_properties.get(&decl.property))
                    {
                        style
                            .custom_properties
                            .insert(decl.property.clone(), value.clone());
                    } else {
                        style.custom_properties.remove(&decl.property);
                    }
                }
                _ => {
                    style
                        .custom_properties
                        .insert(decl.property.clone(), decl.value.clone());
                }
            }
        }
    }
    style.custom_properties = super::syntax::resolve_variables(&style.custom_properties);
    // Pending variable shorthands participate in the cascade as their
    // longhands. Expand only after substitution, retaining the original rank.
    // Invalid-at-computed-value shorthands reset their longhands (CSS Variables
    // 3.1); an earlier declaration must not silently become the winner.
    let mut matched: Vec<MatchedDecl> = matched
        .into_iter()
        .flat_map(|d| {
            let names = super::syntax::shorthand_properties(&d.property);
            if names.is_empty() {
                return vec![d];
            }
            let value = super::syntax::substitute(&d.value, &style.custom_properties)
                .unwrap_or_else(|| "unset".into());
            let mut expanded = super::syntax::expand_shorthand(Declaration {
                property: d.property.clone(),
                value,
                important: d.important,
            });
            if expanded.is_empty() {
                expanded = names
                    .iter()
                    .map(|p| Declaration {
                        property: (*p).into(),
                        value: "unset".into(),
                        important: d.important,
                    })
                    .collect();
            }
            expanded
                .into_iter()
                .map(|e| MatchedDecl {
                    layer: u32::MAX,
                    property: e.property,
                    value: e.value,
                    ..d.clone()
                })
                .collect()
        })
        .collect();
    // Font-relative computed values and currentColor use the winning font and
    // color, regardless of declaration order or selector specificity.
    matched.sort_by_key(|d| match d.property.as_str() {
        "font-size" => 0,
        "color" => 1,
        _ => 2,
    });
    for decl in matched {
        if decl.property.starts_with("--") {
            continue;
        }
        let value = super::syntax::substitute(&decl.value, &style.custom_properties);
        let inherited = matches!(
            decl.property.as_str(),
            "color"
                | "visibility"
                | "pointer-events"
                | "cursor"
                | "font-size"
                | "font-family"
                | "font-weight"
                | "font-style"
                | "line-height"
                | "text-align"
                | "white-space"
        );
        let value = value
            .as_deref()
            .unwrap_or(if inherited { "inherit" } else { "initial" });
        if value == "inherit" || (value == "unset" && inherited) {
            copy_property(
                &mut style,
                parent_style.unwrap_or(&ComputedStyle::default()),
                &decl.property,
            );
        } else if value == "initial" || value == "unset" {
            copy_property(&mut style, &ComputedStyle::default(), &decl.property);
        } else if decl.property == "font-size" {
            if let Some(l) =
                Length::parse(value).filter(|l| !matches!(l, Length::Vw(_) | Length::Vh(_)))
            {
                let parent_size = parent_style.map(|p| p.font_size).unwrap_or(16.0);
                let size = l.resolve(
                    parent_size,
                    parent_size,
                    if is_document_element {
                        16.0
                    } else {
                        style.root_font_size
                    },
                );
                if size >= 0.0 && size.is_finite() {
                    style.font_size = size;
                    if is_document_element {
                        style.root_font_size = size;
                    }
                }
            }
        } else if decl.property == "color" && value.eq_ignore_ascii_case("currentcolor") {
            style.color = parent_style.map(|p| p.color).unwrap_or(Color::BLACK);
        } else {
            let resolved = if value.eq_ignore_ascii_case("currentcolor") {
                format!(
                    "rgba({},{},{},{})",
                    style.color.r,
                    style.color.g,
                    style.color.b,
                    style.color.a as f32 / 255.0
                )
            } else {
                value.to_owned()
            };
            apply_declaration(&mut style, &decl.property, &resolved);
        }
    }
    for (side, hidden) in [
        &mut style.borders.top,
        &mut style.borders.right,
        &mut style.borders.bottom,
        &mut style.borders.left,
    ]
    .into_iter()
    .zip(style.border_hidden)
    {
        if hidden {
            side.width = 0.0;
        }
    }
    if let Some(factor) = style.line_height_factor {
        style.line_height = style.font_size * factor;
    }
    // `background-clip: text` shows the background through the glyphs. Glyphs
    // are painted with a solid color, so use the gradient's middle color and
    // do not paint the background box itself.
    if style.background_clip_text {
        if style.color.a == 0
            && let Some(g) = &style.background_gradient
        {
            style.color = g.midpoint();
        }
        style.background_gradient = None;
        style.background_color = Color::TRANSPARENT;
    }
    if let Some(mask) = style.mask_image.clone() {
        style.background_image = Some(mask);
        style.background_color = Color::TRANSPARENT;
        style.background_size = style
            .mask_size
            .clone()
            .unwrap_or(super::graphics::BackgroundSize::Auto);
        style.background_position = style
            .mask_position
            .unwrap_or((Length::Percent(0.0), Length::Percent(0.0)));
        style.background_repeat = style.mask_repeat.unwrap_or((true, true));
    }
    if let Some(el_name) = node.element_name() {
        let tag = el_name.local;
        if matches!(tag, "img" | "iframe" | "canvas" | "video" | "embed" | "svg") {
            if style.width == Length::Auto
                && let Some(w_attr) = node.attr("width")
                && let Some(l) =
                    Length::parse(w_attr).or_else(|| w_attr.parse::<f64>().ok().map(Length::Px))
            {
                style.width = l;
            }
            if style.height == Length::Auto
                && let Some(h_attr) = node.attr("height")
                && let Some(l) =
                    Length::parse(h_attr).or_else(|| h_attr.parse::<f64>().ok().map(Length::Px))
            {
                style.height = l;
            }
        }
    }
    if node.element_name().is_some_and(|n| n.local == "canvas") {
        if style.width == Length::Auto {
            style.width = Length::Px(300.0);
        }
        if style.height == Length::Auto {
            style.height = Length::Px(150.0);
        }
    }
    style
}

/// A plain or `calc()`-computed number.
fn css_number(val: &str) -> Option<f64> {
    let v = val.trim();
    v.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .or_else(|| super::syntax::calc_number(v))
}
/// Top-level comma-separated parts, keeping function arguments together.
fn top_level_commas(val: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, 0);
    for (i, ch) in val.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(&val[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&val[start..]);
    out
}
/// Top-level space-separated components, keeping function arguments together.
fn components(val: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, None);
    for (i, ch) in val.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c.is_whitespace() && depth == 0 => {
                if let Some(s) = start.take() {
                    out.push(&val[s..i]);
                }
                continue;
            }
            _ => {}
        }
        start.get_or_insert(i);
    }
    if let Some(s) = start {
        out.push(&val[s..]);
    }
    out
}
fn color_value(style: &ComputedStyle, val: &str) -> Option<Color> {
    if val.trim().eq_ignore_ascii_case("currentcolor") {
        Some(style.color)
    } else {
        Color::parse(val)
    }
}
fn border_width(style: &ComputedStyle, val: &str) -> Option<f64> {
    match val.to_ascii_lowercase().as_str() {
        "thin" => Some(1.0),
        "medium" => Some(3.0),
        "thick" => Some(5.0),
        _ => match Length::parse(val)? {
            Length::Auto | Length::Percent(_) => None,
            l => Some(
                l.resolve(0.0, style.font_size, style.root_font_size)
                    .max(0.0),
            ),
        },
    }
}
fn border_side(style: &mut ComputedStyle, index: usize) -> &mut BorderSide {
    match index {
        0 => &mut style.borders.top,
        1 => &mut style.borders.right,
        2 => &mut style.borders.bottom,
        _ => &mut style.borders.left,
    }
}
fn background_repeat(parts: &[&str]) -> Option<(bool, bool)> {
    let axis = |v: &str| match v {
        "repeat" | "space" | "round" => Some(true),
        "no-repeat" => Some(false),
        _ => None,
    };
    match parts {
        ["repeat-x"] => Some((true, false)),
        ["repeat-y"] => Some((false, true)),
        [one] => axis(one).map(|r| (r, r)),
        [x, y] => Some((axis(x)?, axis(y)?)),
        _ => None,
    }
}
/// `background: <image> <position> [/ <size>] <repeat> ... <color>`. The
/// shorthand resets every longhand; only the final layer carries a color, and
/// only the final layer is painted.
fn background_shorthand(style: &mut ComputedStyle, val: &str) {
    style.background_image = None;
    style.background_gradient = None;
    style.background_position = (Length::Percent(0.0), Length::Percent(0.0));
    style.background_size = super::graphics::BackgroundSize::Auto;
    style.background_repeat = (true, true);
    style.background_color = Color::TRANSPARENT;
    for layer in top_level_commas(val) {
        let gradient = components(layer)
            .into_iter()
            .find(|p| p.to_ascii_lowercase().contains("linear-gradient("));
        if let Some(g) = gradient
            && style.background_gradient.is_none()
        {
            style.background_gradient = linear_gradient(style, g);
        }
    }
    // Split layers at top-level commas. Only one image layer is painted: the
    // topmost one that has an image; the final layer carries the color.
    let mut layers = Vec::new();
    let (mut depth, mut start) = (0usize, 0);
    for (i, ch) in val.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                layers.push(&val[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    layers.push(&val[start..]);
    let image_layer = layers
        .iter()
        .position(|l| l.to_ascii_lowercase().contains("url("))
        .unwrap_or(layers.len() - 1);
    for (index, layer) in layers.iter().enumerate() {
        if index == image_layer {
            background_layer(style, layer);
        }
        if index == layers.len() - 1
            && let Some(c) = components(layer).into_iter().find_map(|part| {
                let lower = part.to_ascii_lowercase();
                (!lower.contains('(')
                    || lower.starts_with("rgb")
                    || lower.starts_with("hsl")
                    || lower.starts_with("hwb"))
                .then(|| color_value(style, part))
                .flatten()
            })
        {
            style.background_color = c;
        }
    }
}
fn background_layer(style: &mut ComputedStyle, layer: &str) {
    let mut depth = 0usize;
    let mut slash = None;
    for (i, ch) in layer.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '/' if depth == 0 => slash = Some(i),
            _ => {}
        }
    }
    let (before, after) = match slash {
        Some(i) => (&layer[..i], &layer[i + 1..]),
        None => (layer, ""),
    };
    let mut position = Vec::new();
    let mut repeat = Vec::new();
    let mut rest: Vec<&str> = components(before);
    let after = components(after);
    let size_len = after
        .iter()
        .take(2)
        .take_while(|p| matches!(**p, "auto" | "cover" | "contain") || Length::parse(p).is_some())
        .count();
    if size_len > 0
        && let Some(size) = super::graphics::BackgroundSize::parse(&after[..size_len].join(" "))
    {
        style.background_size = size;
    }
    rest.extend_from_slice(&after[size_len..]);
    for part in rest {
        let lower = part.to_ascii_lowercase();
        if lower.starts_with("url(") || lower.starts_with('"') || lower.starts_with('\'') {
            style.background_image = super::graphics::image_url(part);
        } else if matches!(
            lower.as_str(),
            "repeat" | "no-repeat" | "repeat-x" | "repeat-y" | "space" | "round"
        ) {
            repeat.push(part);
        } else if matches!(
            lower.as_str(),
            "left" | "right" | "top" | "bottom" | "center"
        ) || Length::parse(part).is_some()
        {
            position.push(part);
        }
    }
    if let Some(r) = background_repeat(&repeat) {
        style.background_repeat = r;
    }
    if let Some(p) = super::graphics::position(&position.join(" ")) {
        style.background_position = p;
    }
}
enum GridLine {
    Auto,
    Number(i32),
    Span(usize),
    Name(String),
}
fn grid_line(val: &str) -> GridLine {
    let v = val.trim();
    let lower = v.to_ascii_lowercase();
    if lower.is_empty() || lower == "auto" {
        GridLine::Auto
    } else if let Some(n) = lower.strip_prefix("span") {
        GridLine::Span(n.trim().parse::<usize>().unwrap_or(1).clamp(1, 256))
    } else if let Ok(n) = lower.parse::<i32>() {
        if n == 0 {
            GridLine::Auto
        } else {
            GridLine::Number(n.clamp(-256, 256))
        }
    } else {
        // `main-start`/`main-end` name the area's edges.
        let name = v.trim_end_matches("-start").trim_end_matches("-end");
        GridLine::Name(name.to_owned())
    }
}
fn grid_axis(style: &mut ComputedStyle, column: bool, start: GridLine, end: GridLine) {
    let (mut s, mut e, mut span, mut name) = (None, None, 1usize, None);
    match start {
        GridLine::Number(n) => s = Some(n),
        GridLine::Span(n) => span = n,
        GridLine::Name(n) => name = Some(n),
        GridLine::Auto => {}
    }
    match end {
        GridLine::Number(n) => e = Some(n),
        GridLine::Span(n) => span = n,
        GridLine::Name(n) => {
            name.get_or_insert(n);
        }
        GridLine::Auto => {}
    }
    if column {
        style.grid_column_start = s;
        style.grid_column_end = e;
        style.grid_column_span = span;
        style.grid_column_name = name;
    } else {
        style.grid_row_start = s;
        style.grid_row_end = e;
        style.grid_row_span = span;
        style.grid_row_name = name;
    }
}
fn grid_areas(val: &str) -> Option<Vec<Vec<String>>> {
    if val.trim() == "none" {
        return Some(Vec::new());
    }
    let mut rows = Vec::new();
    let mut rest = val;
    while let Some(open) = rest.find(['"', '\'']) {
        let quote = rest.as_bytes()[open] as char;
        let after = &rest[open + 1..];
        let close = after.find(quote)?;
        rows.push(
            after[..close]
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        );
        rest = &after[close + 1..];
    }
    let width = rows.first().map_or(0, Vec::len);
    (width > 0 && rows.iter().all(|r| r.len() == width) && rows.len() <= 256 && width <= 256)
        .then_some(rows)
}
/// Map a logical property to its physical equivalents for horizontal,
/// left-to-right text: `(longhands, value split into start/end)`.
fn logical_longhands(prop: &str) -> Option<(&'static str, &'static str, bool)> {
    Some(match prop {
        "margin-inline" => ("margin-left", "margin-right", true),
        "margin-block" => ("margin-top", "margin-bottom", true),
        "padding-inline" => ("padding-left", "padding-right", true),
        "padding-block" => ("padding-top", "padding-bottom", true),
        "inset-inline" => ("left", "right", true),
        "inset-block" => ("top", "bottom", true),
        "border-inline" => ("border-left", "border-right", false),
        "border-block" => ("border-top", "border-bottom", false),
        "border-inline-width" => ("border-left-width", "border-right-width", true),
        "border-block-width" => ("border-top-width", "border-bottom-width", true),
        "border-inline-color" => ("border-left-color", "border-right-color", true),
        "border-block-color" => ("border-top-color", "border-bottom-color", true),
        "border-inline-style" => ("border-left-style", "border-right-style", true),
        "border-block-style" => ("border-top-style", "border-bottom-style", true),
        _ => return None,
    })
}
fn logical_single(prop: &str) -> Option<&'static str> {
    Some(match prop {
        "margin-inline-start" => "margin-left",
        "margin-inline-end" => "margin-right",
        "margin-block-start" => "margin-top",
        "margin-block-end" => "margin-bottom",
        "padding-inline-start" => "padding-left",
        "padding-inline-end" => "padding-right",
        "padding-block-start" => "padding-top",
        "padding-block-end" => "padding-bottom",
        "inset-inline-start" => "left",
        "inset-inline-end" => "right",
        "inset-block-start" => "top",
        "inset-block-end" => "bottom",
        "inline-size" => "width",
        "block-size" => "height",
        "min-inline-size" => "min-width",
        "max-inline-size" => "max-width",
        "min-block-size" => "min-height",
        "max-block-size" => "max-height",
        "border-inline-start" => "border-left",
        "border-inline-end" => "border-right",
        "border-block-start" => "border-top",
        "border-block-end" => "border-bottom",
        "border-inline-start-width" => "border-left-width",
        "border-inline-end-width" => "border-right-width",
        "border-inline-start-color" => "border-left-color",
        "border-inline-end-color" => "border-right-color",
        "border-start-start-radius" => "border-top-left-radius",
        "border-start-end-radius" => "border-top-right-radius",
        "border-end-start-radius" => "border-bottom-left-radius",
        "border-end-end-radius" => "border-bottom-right-radius",
        _ => return None,
    })
}
fn apply_declaration(style: &mut ComputedStyle, prop: &str, val: &str) {
    if let Some(physical) = logical_single(prop) {
        return apply_declaration(style, physical, val);
    }
    if let Some((start, end, split)) = logical_longhands(prop) {
        let parts = components(val);
        let (a, b) = match (split, parts.as_slice()) {
            (true, [one]) => (*one, *one),
            (true, [one, two]) => (*one, *two),
            (false, _) => (val, val),
            _ => return,
        };
        apply_declaration(style, start, a);
        apply_declaration(style, end, b);
        return;
    }
    if prop == "inset" {
        let parts = components(val);
        if (1..=4).contains(&parts.len()) {
            let quad = [
                parts[0],
                *parts.get(1).unwrap_or(&parts[0]),
                *parts.get(2).unwrap_or(&parts[0]),
                *parts.get(3).or(parts.get(1)).unwrap_or(&parts[0]),
            ];
            for (side, v) in ["top", "right", "bottom", "left"].into_iter().zip(quad) {
                apply_declaration(style, side, v);
            }
        }
        return;
    }
    match prop {
        "visibility" => {
            if matches!(val, "visible" | "hidden" | "collapse") {
                style.visible = val == "visible";
            }
        }
        "pointer-events" => {
            if matches!(val, "auto" | "none") {
                style.pointer_events = val == "auto";
            }
        }
        "display" => {
            if let Some(d) = Display::parse(val) {
                style.display = d;
            }
        }
        "position" => {
            if let Some(p) = Position::parse(val) {
                style.position = p;
            }
        }
        "box-sizing" => {
            if let Some(b) = BoxSizing::parse(val) {
                style.box_sizing = b;
            }
        }
        "width" => {
            if let Some(l) = Length::parse(val) {
                style.width = l;
            }
        }
        "height" => {
            if let Some(l) = Length::parse(val) {
                style.height = l;
            }
        }
        "min-width" => {
            if let Some(l) = Length::parse(val) {
                style.min_width = l;
            }
        }
        "max-width" => {
            if let Some(l) = Length::parse(val) {
                style.max_width = l;
            }
        }
        "min-height" => {
            if let Some(l) = Length::parse(val) {
                style.min_height = l;
            }
        }
        "max-height" => {
            if let Some(l) = Length::parse(val) {
                style.max_height = l;
            }
        }
        "margin" => style.margin = RectLengths::parse_quad(val),
        "margin-top" => {
            if let Some(l) = Length::parse(val) {
                style.margin.top = l;
            }
        }
        "margin-right" => {
            if let Some(l) = Length::parse(val) {
                style.margin.right = l;
            }
        }
        "margin-bottom" => {
            if let Some(l) = Length::parse(val) {
                style.margin.bottom = l;
            }
        }
        "margin-left" => {
            if let Some(l) = Length::parse(val) {
                style.margin.left = l;
            }
        }
        "padding" => style.padding = RectLengths::parse_quad(val),
        "padding-top" => {
            if let Some(l) = Length::parse(val) {
                style.padding.top = l;
            }
        }
        "padding-right" => {
            if let Some(l) = Length::parse(val) {
                style.padding.right = l;
            }
        }
        "padding-bottom" => {
            if let Some(l) = Length::parse(val) {
                style.padding.bottom = l;
            }
        }
        "padding-left" => {
            if let Some(l) = Length::parse(val) {
                style.padding.left = l;
            }
        }
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            // Initial values: medium width, no style, currentcolor.
            let mut side = BorderSide {
                width: 3.0,
                color: style.color,
            };
            let mut styled = false;
            for part in components(val) {
                if let Some(w) = border_width(style, part) {
                    side.width = w;
                } else if let Some(c) = color_value(style, part) {
                    side.color = c;
                } else if matches!(
                    part.to_ascii_lowercase().as_str(),
                    "solid"
                        | "dotted"
                        | "dashed"
                        | "double"
                        | "groove"
                        | "ridge"
                        | "inset"
                        | "outset"
                ) {
                    styled = true;
                }
            }
            let indices: &[usize] = match prop {
                "border-top" => &[0],
                "border-right" => &[1],
                "border-bottom" => &[2],
                "border-left" => &[3],
                _ => &[0, 1, 2, 3],
            };
            for &i in indices {
                style.border_hidden[i] = !styled;
                *border_side(style, i) = side;
            }
        }
        "border-style" => {
            let p: Vec<_> = val.split_whitespace().collect();
            if !p.is_empty() {
                let values = [
                    p[0],
                    *p.get(1).unwrap_or(&p[0]),
                    *p.get(2).unwrap_or(&p[0]),
                    *p.get(3).or(p.get(1)).unwrap_or(&p[0]),
                ];
                style.border_hidden = values.map(|p| matches!(p, "none" | "hidden"));
            }
        }
        // Per-side longhands only; `border-width`/`-color`/`-style` themselves
        // are shorthands handled below.
        p if [
            "border-top-",
            "border-right-",
            "border-bottom-",
            "border-left-",
        ]
        .iter()
        .any(|side| p.starts_with(side))
            && (p.ends_with("-width") || p.ends_with("-color") || p.ends_with("-style")) =>
        {
            let index = if p.starts_with("border-top-") {
                0
            } else if p.starts_with("border-right-") {
                1
            } else if p.starts_with("border-bottom-") {
                2
            } else if p.starts_with("border-left-") {
                3
            } else {
                return;
            };
            if p.ends_with("-width") {
                if let Some(w) = border_width(style, val) {
                    border_side(style, index).width = w;
                }
            } else if p.ends_with("-color") {
                if let Some(c) = color_value(style, val) {
                    border_side(style, index).color = c;
                }
            } else {
                style.border_hidden[index] = matches!(val, "none" | "hidden");
            }
        }
        "border-width" | "border-color" => {
            let parts = components(val);
            if (1..=4).contains(&parts.len()) {
                let quad = [
                    parts[0],
                    *parts.get(1).unwrap_or(&parts[0]),
                    *parts.get(2).unwrap_or(&parts[0]),
                    *parts.get(3).or(parts.get(1)).unwrap_or(&parts[0]),
                ];
                for (i, part) in quad.into_iter().enumerate() {
                    if prop == "border-width" {
                        if let Some(w) = border_width(style, part) {
                            border_side(style, i).width = w;
                        }
                    } else if let Some(c) = color_value(style, part) {
                        border_side(style, i).color = c;
                    }
                }
            }
        }
        "border-spacing" => {
            if let Some(l) = Length::parse(val) {
                style.border_spacing = l
                    .resolve(0.0, style.font_size, style.root_font_size)
                    .max(0.0);
            }
        }
        // Elliptical and per-corner radii are approximated by one circular
        // radius: the first horizontal value.
        p if p == "border-radius" || (p.starts_with("border-") && p.ends_with("-radius")) => {
            if p != "border-radius"
                && (style.border_radius > 0.0 || style.border_radius_percent > 0.0)
            {
                return;
            }
            let first = val
                .split('/')
                .next()
                .and_then(|v| components(v).first().copied());
            match first.and_then(Length::parse) {
                Some(Length::Percent(pct)) => {
                    style.border_radius = 0.0;
                    style.border_radius_percent = pct.max(0.0);
                }
                Some(l) => {
                    style.border_radius = l
                        .resolve(0.0, style.font_size, style.root_font_size)
                        .max(0.0);
                    style.border_radius_percent = 0.0;
                }
                None => {}
            }
        }
        "color" => {
            if val.eq_ignore_ascii_case("currentcolor") {
                // Inherited color is already in place.
            } else if let Some(c) = Color::parse(val) {
                style.color = c;
            }
        }
        "background-image" => {
            style.background_image = None;
            style.background_gradient = None;
            for layer in top_level_commas(val) {
                let lower = layer.trim().to_ascii_lowercase();
                if lower.starts_with("url(") && style.background_image.is_none() {
                    style.background_image = super::graphics::image_url(layer.trim());
                } else if lower.contains("linear-gradient(") && style.background_gradient.is_none()
                {
                    style.background_gradient = linear_gradient(style, layer);
                }
            }
        }
        "background-clip" | "-webkit-background-clip" => {
            style.background_clip_text = val.trim().eq_ignore_ascii_case("text");
        }
        "background-position" => {
            if let Some(p) = super::graphics::position(val) {
                style.background_position = p;
            }
        }
        "background-size" => {
            if let Some(size) = super::graphics::BackgroundSize::parse(val) {
                style.background_size = size;
            }
        }
        "background-repeat" => {
            if let Some(repeat) = background_repeat(&components(val)) {
                style.background_repeat = repeat;
            }
        }
        "clip-path" => style.clip_path = super::graphics::ClipPath::parse(val),
        "background-color" => {
            if let Some(c) = color_value(style, val) {
                style.background_color = c;
            }
        }
        "background" => background_shorthand(style, val),
        "cursor" => {
            if matches!(
                val,
                "auto"
                    | "default"
                    | "none"
                    | "context-menu"
                    | "help"
                    | "pointer"
                    | "progress"
                    | "wait"
                    | "cell"
                    | "crosshair"
                    | "text"
                    | "vertical-text"
                    | "alias"
                    | "copy"
                    | "move"
                    | "no-drop"
                    | "not-allowed"
                    | "grab"
                    | "grabbing"
                    | "all-scroll"
                    | "col-resize"
                    | "row-resize"
                    | "n-resize"
                    | "e-resize"
                    | "s-resize"
                    | "w-resize"
                    | "ne-resize"
                    | "nw-resize"
                    | "se-resize"
                    | "sw-resize"
                    | "ew-resize"
                    | "ns-resize"
                    | "nesw-resize"
                    | "nwse-resize"
                    | "zoom-in"
                    | "zoom-out"
            ) {
                style.cursor = val.into();
            }
        }
        "font-family" => style.font_family = val.to_owned(),
        "font-weight" => {
            style.font_weight = match val {
                "bold" => 700.0,
                "normal" => 400.0,
                _ => val
                    .parse::<f32>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .unwrap_or(style.font_weight)
                    .clamp(1.0, 1000.0),
            };
        }
        "font-style" => style.font_italic = matches!(val, "italic" | "oblique"),
        "white-space" => {
            if let Some(value) = WhiteSpace::parse(val) {
                style.white_space = value;
            }
        }
        "font-size" => {
            if let Some(l) = Length::parse(val).map(viewport_px) {
                let size = l.resolve(style.font_size, style.font_size, style.root_font_size);
                if size >= 0.0 && size.is_finite() {
                    style.font_size = size.min(4096.0);
                }
            }
        }
        "line-height" => {
            if val == "normal" {
                style.line_height_factor = Some(1.25);
            } else if let Ok(factor) = val.parse::<f64>() {
                if factor >= 0.0 && factor.is_finite() {
                    style.line_height_factor = Some(factor);
                }
            } else if let Some(l) =
                Length::parse(val).filter(|l| !matches!(l, Length::Vw(_) | Length::Vh(_)))
            {
                let h = l.resolve(style.font_size, style.font_size, style.root_font_size);
                if h >= 0.0 && h.is_finite() {
                    style.line_height = h;
                    style.line_height_factor = None;
                }
            }
        }
        "text-align" => {
            if let Some(a) = TextAlign::parse(val) {
                style.text_align = a;
            }
        }
        "top" => {
            if let Some(l) = Length::parse(val) {
                style.top = l;
            }
        }
        "right" => {
            if let Some(l) = Length::parse(val) {
                style.right = l;
            }
        }
        "bottom" => {
            if let Some(l) = Length::parse(val) {
                style.bottom = l;
            }
        }
        "left" => {
            if let Some(l) = Length::parse(val) {
                style.left = l;
            }
        }
        "z-index" => {
            style.z_index = if val.trim().eq_ignore_ascii_case("auto") {
                None
            } else {
                css_number(val).map(|n| n.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32)
            }
        }
        "mask-image" | "-webkit-mask-image" => style.mask_image = super::graphics::image_url(val),
        "mask" | "-webkit-mask" => {
            style.mask_image = components(val)
                .into_iter()
                .find(|p| p.to_ascii_lowercase().starts_with("url("))
                .and_then(super::graphics::image_url);
        }
        "mask-size" | "-webkit-mask-size" => {
            style.mask_size = super::graphics::BackgroundSize::parse(val.trim());
        }
        "mask-position" | "-webkit-mask-position" => {
            style.mask_position = super::graphics::position(val.trim());
        }
        "mask-repeat" | "-webkit-mask-repeat" => {
            style.mask_repeat = background_repeat(&components(val));
        }
        "float" => {
            style.float = match val.trim().to_ascii_lowercase().as_str() {
                "left" | "inline-start" => Float::Left,
                "right" | "inline-end" => Float::Right,
                _ => Float::None,
            }
        }
        "clear" => {
            style.clear = match val.trim().to_ascii_lowercase().as_str() {
                "left" | "inline-start" => (true, false),
                "right" | "inline-end" => (false, true),
                "both" => (true, true),
                _ => (false, false),
            }
        }
        "opacity" => {
            let v = val.trim();
            let op = v
                .strip_suffix('%')
                .and_then(|p| p.trim().parse::<f64>().ok().map(|p| p / 100.0))
                .or_else(|| css_number(v));
            if let Some(op) = op {
                style.opacity = op.clamp(0.0, 1.0) as f32;
            }
        }
        "overflow" => {
            if let Some(o) = Overflow::parse(val) {
                style.overflow_x = o;
                style.overflow_y = o;
            }
        }
        "overflow-x" => {
            if let Some(o) = Overflow::parse(val) {
                style.overflow_x = o;
            }
        }
        "overflow-y" => {
            if let Some(o) = Overflow::parse(val) {
                style.overflow_y = o;
            }
        }
        "flex-direction" => {
            if let Some(fd) = FlexDirection::parse(val) {
                style.flex_direction = fd;
            }
        }
        "justify-content" => {
            if let Some(jc) = JustifyContent::parse(val) {
                style.justify_content = jc;
            }
        }
        "align-items" => {
            if let Some(ai) = AlignItems::parse(val) {
                style.align_items = ai;
            }
        }
        "flex-grow" => {
            if let Some(g) = css_number(val).filter(|g| *g >= 0.0) {
                style.flex_grow = g;
            }
        }
        "flex-shrink" => {
            if let Some(s) = css_number(val).filter(|s| *s >= 0.0) {
                style.flex_shrink = s;
            }
        }
        "flex-basis" => {
            if let Some(l) = Length::parse(val) {
                style.flex_basis = l;
            }
        }
        "flex-wrap" => style.flex_wrap = val == "wrap",
        "gap" => {
            let values: Vec<_> = components(val)
                .into_iter()
                .filter_map(Length::parse)
                .collect();
            if let Some(v) = values.first() {
                style.row_gap = v
                    .resolve(0.0, style.font_size, style.root_font_size)
                    .max(0.0);
                style.column_gap = values
                    .get(1)
                    .unwrap_or(v)
                    .resolve(0.0, style.font_size, style.root_font_size)
                    .max(0.0);
            }
        }
        "row-gap" => {
            if let Some(v) = Length::parse(val) {
                style.row_gap = v
                    .resolve(0.0, style.font_size, style.root_font_size)
                    .max(0.0);
            }
        }
        "column-gap" => {
            if let Some(v) = Length::parse(val) {
                style.column_gap = v
                    .resolve(0.0, style.font_size, style.root_font_size)
                    .max(0.0);
            }
        }
        "grid-template-columns" => {
            if let Some(tracks) = super::syntax::tracks(val) {
                style.grid_template_columns = tracks;
            }
        }
        "grid-template-rows" => {
            if let Some(tracks) = super::syntax::tracks(val) {
                style.grid_template_rows = tracks;
            }
        }
        "grid-column" | "grid-row" => {
            let mut parts = val.split('/');
            let first = parts.next().unwrap_or("");
            let second = parts.next().unwrap_or("auto");
            grid_axis(
                style,
                prop == "grid-column",
                grid_line(first),
                grid_line(second),
            );
        }
        "grid-area" => {
            let parts: Vec<&str> = val.split('/').map(str::trim).collect();
            let line = |i: usize, fallback: usize| {
                parts.get(i).map(|p| grid_line(p)).unwrap_or_else(|| {
                    match grid_line(parts[fallback.min(parts.len() - 1)]) {
                        GridLine::Name(n) => GridLine::Name(n),
                        _ => GridLine::Auto,
                    }
                })
            };
            grid_axis(style, false, line(0, 0), line(2, 0));
            grid_axis(style, true, line(1, 0), line(3, 1));
        }
        "grid-column-start" | "grid-row-start" | "grid-column-end" | "grid-row-end" => {
            let column = prop.starts_with("grid-column");
            let (start, end) = if column {
                (style.grid_column_start, style.grid_column_end)
            } else {
                (style.grid_row_start, style.grid_row_end)
            };
            let keep = |n: Option<i32>| n.map_or(GridLine::Auto, GridLine::Number);
            if prop.ends_with("-start") {
                grid_axis(style, column, grid_line(val), keep(end));
            } else {
                grid_axis(style, column, keep(start), grid_line(val));
            }
        }
        "grid-template-areas" => {
            style.grid_template_areas = grid_areas(val).unwrap_or_default();
        }
        "grid-template" => {
            let (rows, columns) = val.split_once('/').unwrap_or((val, ""));
            if rows.contains('"') || rows.contains('\'') {
                let mut areas = Vec::new();
                let mut sizes = Vec::new();
                let mut rest = rows;
                while let Some(open) = rest.find(['"', '\'']) {
                    let quote = rest.as_bytes()[open] as char;
                    let after = &rest[open + 1..];
                    let Some(close) = after.find(quote) else {
                        break;
                    };
                    areas.push(
                        after[..close]
                            .split_whitespace()
                            .map(str::to_owned)
                            .collect(),
                    );
                    rest = &after[close + 1..];
                    let size = rest.split(['"', '\'']).next().unwrap_or("").trim();
                    sizes.push(if size.is_empty() { "auto" } else { size });
                }
                style.grid_template_areas = areas;
                if let Some(tracks) = super::syntax::tracks(&sizes.join(" ")) {
                    style.grid_template_rows = tracks;
                }
            } else if let Some(tracks) = super::syntax::tracks(rows.trim()) {
                style.grid_template_rows = tracks;
                style.grid_template_areas.clear();
            }
            if let Some(tracks) = super::syntax::tracks(columns.trim()) {
                style.grid_template_columns = tracks;
            }
        }
        "transform" => style.transform = Transform::parse(val),
        "transform-origin" => {
            let p: Vec<_> = val.split_ascii_whitespace().collect();
            let axis = |v: &str| match v {
                "center" => Some(Length::Percent(50.0)),
                "left" | "top" => Some(Length::Percent(0.0)),
                "right" | "bottom" => Some(Length::Percent(100.0)),
                _ => Length::parse(v),
            };
            if let Some(x) = p.first().and_then(|v| axis(v)) {
                style.transform_origin = (
                    x,
                    p.get(1)
                        .and_then(|v| axis(v))
                        .unwrap_or(Length::Percent(50.0)),
                );
            }
        }
        "object-fit" => {
            if let Some(of) = ObjectFit::parse(val) {
                style.object_fit = of;
            }
        }
        "object-position" => {
            let p: Vec<_> = val.split_ascii_whitespace().collect();
            let axis = |v: &str| match v {
                "center" => Some(Length::Percent(50.0)),
                "left" | "top" => Some(Length::Percent(0.0)),
                "right" | "bottom" => Some(Length::Percent(100.0)),
                _ => Length::parse(v),
            };
            if let Some(x) = p.first().and_then(|v| axis(v)) {
                style.object_position = (
                    x,
                    p.get(1)
                        .and_then(|v| axis(v))
                        .unwrap_or(Length::Percent(50.0)),
                );
            }
        }
        "vertical-align" => {
            if let Some(va) = VerticalAlign::parse(val) {
                style.vertical_align = va;
            }
        }
        "box-shadow" => style.box_shadow = BoxShadow::parse(val),
        _ => {}
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WhiteSpace {
    #[default]
    Normal,
    NoWrap,
    Pre,
    PreWrap,
    PreLine,
    BreakSpaces,
}
impl WhiteSpace {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "normal" => Self::Normal,
            "nowrap" => Self::NoWrap,
            "pre" => Self::Pre,
            "pre-wrap" => Self::PreWrap,
            "pre-line" => Self::PreLine,
            "break-spaces" => Self::BreakSpaces,
            _ => return None,
        })
    }
}

fn copy_property(target: &mut ComputedStyle, source: &ComputedStyle, prop: &str) {
    macro_rules! copy { ($($name:literal => $field:ident),* $(,)?) => { match prop { $( $name => target.$field=source.$field.clone(), )* _=>{} } }; }
    copy! { "display"=>display,"position"=>position,"box-sizing"=>box_sizing,
    "color"=>color,"background-color"=>background_color,"background"=>background_color,
    "width"=>width,"height"=>height,"min-width"=>min_width,"max-width"=>max_width,"min-height"=>min_height,"max-height"=>max_height,
    "margin"=>margin,"padding"=>padding,"border"=>borders,"font-size"=>font_size,"font-family"=>font_family,
    "font-weight"=>font_weight,"font-style"=>font_italic,"white-space"=>white_space,"text-align"=>text_align,
    "opacity"=>opacity,"visibility"=>visible,"pointer-events"=>pointer_events,"cursor"=>cursor,"top"=>top,"right"=>right,"bottom"=>bottom,"left"=>left,"z-index"=>z_index,
    "transform"=>transform,"box-shadow"=>box_shadow,"border-radius"=>border_radius,
    "overflow-x"=>overflow_x,"overflow-y"=>overflow_y,"flex-direction"=>flex_direction,
    "flex-grow"=>flex_grow,"flex-shrink"=>flex_shrink,"flex-basis"=>flex_basis,"align-items"=>align_items,"justify-content"=>justify_content,"object-fit"=>object_fit,"object-position"=>object_position,"vertical-align"=>vertical_align }
    if prop == "line-height" {
        target.line_height = source.line_height;
        target.line_height_factor = source.line_height_factor;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GridTrack {
    Length(Length),
    Fraction(f64),
    MinMax(Length, Box<GridTrack>),
    /// `repeat(auto-fill | auto-fit, …)`; expanded once the container width is known.
    AutoRepeat(Vec<GridTrack>),
}

pub(super) fn supported_property(property: &str) -> bool {
    matches!(
        property,
        "visibility" | "pointer-events" | "vertical-align" | "cursor"
    ) || property.starts_with("--")
        || matches!(
            property,
            "align-items"
                | "background"
                | "background-color"
                | "background-image"
                | "background-position"
                | "background-size"
                | "background-repeat"
                | "clip-path"
                | "bold"
                | "border"
                | "border-bottom"
                | "border-color"
                | "border-left"
                | "border-radius"
                | "border-right"
                | "border-spacing"
                | "border-top"
                | "border-width"
                | "bottom"
                | "box-shadow"
                | "box-sizing"
                | "center"
                | "color"
                | "column-gap"
                | "display"
                | "flex-basis"
                | "flex-direction"
                | "flex-grow"
                | "flex-shrink"
                | "flex-wrap"
                | "font-family"
                | "font-size"
                | "font-style"
                | "font-weight"
                | "gap"
                | "grid-column"
                | "grid-column-start"
                | "grid-row"
                | "grid-row-start"
                | "grid-template-columns"
                | "grid-template-rows"
                | "grid-template-areas"
                | "grid-template"
                | "grid-area"
                | "grid-column-end"
                | "grid-row-end"
                | "float"
                | "clear"
                | "height"
                | "justify-content"
                | "left"
                | "line-height"
                | "margin"
                | "margin-bottom"
                | "margin-left"
                | "margin-right"
                | "margin-top"
                | "max-height"
                | "max-width"
                | "min-height"
                | "min-width"
                | "normal"
                | "object-fit"
                | "object-position"
                | "opacity"
                | "overflow"
                | "overflow-x"
                | "overflow-y"
                | "padding"
                | "padding-bottom"
                | "padding-left"
                | "padding-right"
                | "padding-top"
                | "position"
                | "right"
                | "row-gap"
                | "text-align"
                | "top"
                | "transform"
                | "transform-origin"
                | "white-space"
                | "width"
                | "z-index"
        )
}
