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
        let s = s.trim().to_ascii_lowercase();
        if s == "transparent" {
            return Some(Self::TRANSPARENT);
        }
        if let Some(c) = match s.as_str() {
            "black" => Some(Self::BLACK),
            "white" => Some(Self::WHITE),
            "red" => Some(Self {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            }),
            "green" => Some(Self {
                r: 0,
                g: 128,
                b: 0,
                a: 255,
            }),
            "blue" => Some(Self {
                r: 0,
                g: 0,
                b: 255,
                a: 255,
            }),
            "yellow" => Some(Self {
                r: 255,
                g: 255,
                b: 0,
                a: 255,
            }),
            "cyan" => Some(Self {
                r: 0,
                g: 255,
                b: 255,
                a: 255,
            }),
            "magenta" => Some(Self {
                r: 255,
                g: 0,
                b: 255,
                a: 255,
            }),
            "gray" | "grey" => Some(Self {
                r: 128,
                g: 128,
                b: 128,
                a: 255,
            }),
            "lightgray" | "lightgrey" => Some(Self {
                r: 211,
                g: 211,
                b: 211,
                a: 255,
            }),
            "darkgray" | "darkgrey" => Some(Self {
                r: 169,
                g: 169,
                b: 169,
                a: 255,
            }),
            "silver" => Some(Self {
                r: 192,
                g: 192,
                b: 192,
                a: 255,
            }),
            "maroon" => Some(Self {
                r: 128,
                g: 0,
                b: 0,
                a: 255,
            }),
            "navy" => Some(Self {
                r: 0,
                g: 0,
                b: 128,
                a: 255,
            }),
            "olive" => Some(Self {
                r: 128,
                g: 128,
                b: 0,
                a: 255,
            }),
            "purple" => Some(Self {
                r: 128,
                g: 0,
                b: 128,
                a: 255,
            }),
            "teal" => Some(Self {
                r: 0,
                g: 128,
                b: 128,
                a: 255,
            }),
            "orange" => Some(Self {
                r: 255,
                g: 165,
                b: 0,
                a: 255,
            }),
            _ => None,
        } {
            return Some(c);
        }

        if let Some(hex) = s.strip_prefix('#') {
            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            match hex.len() {
                3 => {
                    let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                    let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                    let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                    return Some(Self { r, g, b, a: 255 });
                }
                4 => {
                    let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                    let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                    let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                    let a = u8::from_str_radix(&hex[3..4].repeat(2), 16).ok()?;
                    return Some(Self { r, g, b, a });
                }
                6 => {
                    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                    return Some(Self { r, g, b, a: 255 });
                }
                8 => {
                    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                    let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                    return Some(Self { r, g, b, a });
                }
                _ => return None,
            }
        }

        if (s.starts_with("rgb(") || s.starts_with("rgba(")) && s.ends_with(')') {
            let start = s.find('(')? + 1;
            let end = s.len() - 1;
            let inner = &s[start..end];
            let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
            if parts.len() >= 3 {
                let r: u8 = parts[0].parse().ok()?;
                let g: u8 = parts[1].parse().ok()?;
                let b: u8 = parts[2].parse().ok()?;
                let a = if parts.len() >= 4 {
                    if let Ok(alpha_f) = parts[3].parse::<f32>() {
                        (alpha_f.clamp(0.0, 1.0) * 255.0).round() as u8
                    } else {
                        255
                    }
                } else {
                    255
                };
                return Some(Self { r, g, b, a });
            }
        }

        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
    Px(f64),
    Percent(f64),
    Em(f64),
    Rem(f64),
    Calc([f64; 4]),
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
        if let Some(v) = s.strip_suffix("vw") {
            return v
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .map(Self::Vw);
        }
        if let Some(v) = s.strip_suffix("vh") {
            return v
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .map(Self::Vh);
        }
        if let Some(px) = s.strip_suffix("px") {
            return px
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .map(Self::Px);
        }
        if let Some(pct) = s.strip_suffix('%') {
            return pct
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .map(Self::Percent);
        }
        if let Some(rem) = s.strip_suffix("rem") {
            return rem
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .map(Self::Rem);
        }
        if let Some(em) = s.strip_suffix("em") {
            return em
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .map(Self::Em);
        }
        if s.starts_with("calc(") {
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
            _ => None,
        }
    }

    pub fn is_block_level(&self) -> bool {
        matches!(self, Self::Block | Self::Flex | Self::Grid | Self::Table)
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
    Scroll,
    Auto,
}

impl Overflow {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "visible" => Some(Self::Visible),
            "hidden" => Some(Self::Hidden),
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
        let parsed = Self::parse_unchecked(s)?;
        parsed
            .matrix
            .iter()
            .all(|n| n.is_finite())
            .then_some(parsed)
    }
    fn parse_unchecked(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.starts_with("translate(") && s.ends_with(")") {
            let inner = &s[10..s.len() - 1];
            let parts: Vec<&str> = inner.split(",").map(str::trim).collect();
            let tx = parts.get(0)?.trim_end_matches("px").parse::<f64>().ok()?;
            let ty = if let Some(p1) = parts.get(1) {
                p1.trim_end_matches("px").parse::<f64>().ok()?
            } else {
                0.0
            };
            return Some(Self {
                matrix: [1.0, 0.0, 0.0, 1.0, tx, ty],
            });
        }
        if s.starts_with("scale(") && s.ends_with(")") {
            let inner = &s[6..s.len() - 1];
            let parts: Vec<&str> = inner.split(",").map(str::trim).collect();
            let sx = parts.get(0)?.parse::<f64>().ok()?;
            let sy = if let Some(p1) = parts.get(1) {
                p1.parse::<f64>().ok()?
            } else {
                sx
            };
            return Some(Self {
                matrix: [sx, 0.0, 0.0, sy, 0.0, 0.0],
            });
        }
        if s.starts_with("rotate(") && s.ends_with(")") {
            let inner = s[7..s.len() - 1].trim();
            let deg = inner.trim_end_matches("deg").parse::<f64>().ok()?;
            let rad = deg.to_radians();
            let cos = rad.cos();
            let sin = rad.sin();
            return Some(Self {
                matrix: [cos, sin, -sin, cos, 0.0, 0.0],
            });
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    pub offset_x: f64,
    pub offset_y: f64,
    pub blur_radius: f64,
    pub color: Color,
}

impl BoxShadow {
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() >= 3 {
            let ox = parts[0].trim_end_matches("px").parse::<f64>().ok()?;
            let oy = parts[1].trim_end_matches("px").parse::<f64>().ok()?;
            let mut blur = 0.0;
            let color_str = if parts.len() >= 4 {
                blur = parts[2]
                    .trim_end_matches("px")
                    .parse::<f64>()
                    .unwrap_or(0.0);
                parts[3..].join(" ")
            } else {
                parts[2..].join(" ")
            };
            let color = Color::parse(&color_str).unwrap_or(Color::BLACK);
            Some(Self {
                offset_x: ox,
                offset_y: oy,
                blur_radius: blur,
                color,
            })
        } else {
            None
        }
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
    pub color: Color,
    pub background_color: Color,
    pub custom_properties: std::collections::BTreeMap<String, String>,
    pub line_height_factor: Option<f64>,
    pub font_size: f64,
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
    pub border_spacing: f64,
    pub border_radius: f64,
    pub transform: Option<Transform>,
    pub transform_origin: (Length, Length),
    pub box_shadow: Option<BoxShadow>,
    pub object_fit: ObjectFit,
    pub object_position: (Length, Length),
    pub vertical_align: VerticalAlign,
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
            color: Color::BLACK,
            background_color: Color::TRANSPARENT,
            custom_properties: Default::default(),
            line_height_factor: Some(1.25),
            font_size: 16.0,
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
            border_spacing: 2.0,
            border_radius: 0.0,
            transform: None,
            transform_origin: (Length::Percent(50.0), Length::Percent(50.0)),
            box_shadow: None,
            object_fit: ObjectFit::Fill,
            object_position: (Length::Percent(50.0), Length::Percent(50.0)),
            vertical_align: VerticalAlign::Baseline,
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
    Empty,
    FirstChild,
    LastChild,
    OnlyChild,
    NthChild(i32, i32, bool),
    Is(Vec<ComplexSelector>),
    Where(Vec<ComplexSelector>),
    Not(Vec<ComplexSelector>),
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
                    let siblings: Vec<_> = parent
                        .children()
                        .filter(|k| doc.node(*k).is_some_and(|n| n.kind() == NodeKind::Element))
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
                        SimpleSelector::NthChild(a, b, reverse) => {
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
                if let Some(end) = remaining[start..].find(')') {
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
}

#[derive(Debug, Clone, Default)]
pub struct StyleSheet {
    pub rules: Vec<Rule>,
    pub font_faces: Vec<FontFaceRule>,
    pub imports: Vec<ImportRule>,
    pub unsupported: Vec<String>,
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
head, style, script, meta, link, title, template, base { display: none; }
[hidden] {display:none;}
html { display: block; }
body { display: block; margin: 8px; font-size: 16px; line-height: 20px; color: black; background-color: white; }
div, p, h1, h2, h3, h4, h5, h6, ul, ol, li, header, footer, nav, section, article, main, form, fieldset, figure, figcaption, blockquote, dl, dt, dd, details, summary { display: block; }
span, a, b, strong, i, em, u { display: inline; }
a[href] { color: blue; }
table { display: table; border-collapse: separate; }
tr { display: table-row; }
td, th { display: table-cell; }
img, button, input, textarea, select { display: inline-block; }
button { box-sizing: border-box; padding: 2px 6px; border: 2px solid #aaaaaa; background-color: #f0f0f0; }
input, textarea, select { box-sizing: border-box; width: 185px; height: 24px; border: 1px solid #aaaaaa; background-color: white; }
textarea { white-space: pre-wrap; height: 60px; }
iframe { display: inline-block; border: 1px solid black; width: 300px; height: 150px; }
h1 { margin: 16px 0; font-size: 32px; font-weight: bold; }
h2 { margin: 14px 0; font-size: 24px; font-weight: bold; }
h3 { margin: 12px 0; font-size: 18px; font-weight: bold; }
p { margin: 12px 0; }
ul, ol { margin: 12px 0; padding-left: 40px; }
li { display: block; }
b, strong { font-weight: bold; }
i, em { font-style: italic; }
pre { display: block; white-space: pre; font-family: monospace; }
code { font-family: monospace; }
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
}

thread_local! {
    static USER_AGENT: StyleSheet = default_user_agent_stylesheet();
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
    let mut style = parent_style.map_or_else(ComputedStyle::default, |p| ComputedStyle {
        // Inherited properties:
        custom_properties: p.custom_properties.clone(),
        line_height_factor: p.line_height_factor,
        color: p.color,
        visible: p.visible,
        pointer_events: p.pointer_events,
        font_size: p.font_size,
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

    // HTML dimensions are author presentational hints, below authored rules.
    if node
        .element_name()
        .is_some_and(|n| matches!(n.local, "img" | "iframe" | "video" | "canvas"))
    {
        for property in ["width", "height"] {
            if let Some(value) = node
                .attr(property)
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|n| n.is_finite() && *n >= 0.0)
            {
                matched.push(MatchedDecl {
                    property: property.into(),
                    value: format!("{value}px"),
                    origin: 1,
                    important: false,
                    specificity: (0, 0, 0),
                    source_order: 0,
                });
            }
        }
    }
    // 2. Author Stylesheets
    for sheet in author_sheets {
        for rule in &sheet.rules {
            if !rule.media.iter().all(|group| matches_media(group, m_ctx)) {
                continue;
            }
            for sel in &rule.selectors {
                if sel.matches(doc, node_key) {
                    let spec = sel.specificity();
                    for d in &rule.declarations {
                        matched.push(MatchedDecl {
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
                let size = l.resolve(parent_size, parent_size, 16.0);
                if size >= 0.0 && size.is_finite() {
                    style.font_size = size;
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
    if let Some(el_name) = node.element_name() {
        let tag = el_name.local;
        if matches!(tag, "img" | "iframe" | "canvas" | "video" | "embed") {
            if style.width == Length::Auto {
                if let Some(w_attr) = node.attr("width") {
                    if let Some(l) =
                        Length::parse(w_attr).or_else(|| w_attr.parse::<f64>().ok().map(Length::Px))
                    {
                        style.width = l;
                    }
                }
            }
            if style.height == Length::Auto {
                if let Some(h_attr) = node.attr("height") {
                    if let Some(l) =
                        Length::parse(h_attr).or_else(|| h_attr.parse::<f64>().ok().map(Length::Px))
                    {
                        style.height = l;
                    }
                }
            }
        }
    }
    style
}

fn apply_declaration(style: &mut ComputedStyle, prop: &str, val: &str) {
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
        "border" => {
            let mut width = 0.0;
            let mut color = Color::BLACK;
            let mut has_width = false;
            for part in val.split_whitespace() {
                if let Some(Length::Px(w)) = Length::parse(part) {
                    width = w;
                    has_width = true;
                } else if let Some(c) = Color::parse(part) {
                    color = c;
                } else if matches!(part, "none" | "hidden" | "0") {
                    width = 0.0;
                    has_width = true;
                }
            }
            if !has_width && !matches!(val.trim(), "none" | "hidden" | "0") {
                width = 1.0;
            }
            let side = BorderSide { width, color };
            style.border_hidden = [!val.split_whitespace().any(|p| {
                matches!(
                    p,
                    "solid"
                        | "dotted"
                        | "dashed"
                        | "double"
                        | "groove"
                        | "ridge"
                        | "inset"
                        | "outset"
                )
            }); 4];
            style.borders = Borders {
                top: side,
                right: side,
                bottom: side,
                left: side,
            };
        }
        "border-top" | "border-right" | "border-bottom" | "border-left" => {
            let mut side = BorderSide {
                width: 3.0,
                color: style.color,
            };
            for token in val.split_ascii_whitespace() {
                if let Some(Length::Px(w)) = Length::parse(token) {
                    side.width = w.max(0.0);
                } else if let Some(c) = Color::parse(token) {
                    side.color = c;
                } else if matches!(token, "none" | "hidden") {
                    side.width = 0.0;
                }
            }
            let index = match prop {
                "border-top" => 0,
                "border-right" => 1,
                "border-bottom" => 2,
                _ => 3,
            };
            style.border_hidden[index] = !val.split_whitespace().any(|p| {
                matches!(
                    p,
                    "solid"
                        | "dotted"
                        | "dashed"
                        | "double"
                        | "groove"
                        | "ridge"
                        | "inset"
                        | "outset"
                )
            });
            match prop {
                "border-top" => style.borders.top = side,
                "border-right" => style.borders.right = side,
                "border-bottom" => style.borders.bottom = side,
                _ => style.borders.left = side,
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
        p if p.starts_with("border-")
            && (p.ends_with("-width") || p.ends_with("-color") || p.ends_with("-style")) =>
        {
            let (index, side) = if p.starts_with("border-top-") {
                (0, &mut style.borders.top)
            } else if p.starts_with("border-right-") {
                (1, &mut style.borders.right)
            } else if p.starts_with("border-bottom-") {
                (2, &mut style.borders.bottom)
            } else if p.starts_with("border-left-") {
                (3, &mut style.borders.left)
            } else {
                return;
            };
            if p.ends_with("-width") {
                if let Some(w) = Length::parse(val)
                    .map(|l| l.resolve(0.0, style.font_size, 16.0))
                    .or(match val {
                        "thin" => Some(1.0),
                        "medium" => Some(3.0),
                        "thick" => Some(5.0),
                        _ => None,
                    })
                {
                    side.width = w.max(0.0);
                }
            } else if p.ends_with("-color") {
                if let Some(c) = Color::parse(val) {
                    side.color = c;
                }
            } else {
                style.border_hidden[index] = matches!(val, "none" | "hidden");
            }
        }
        "border-width" => {
            if let Some(Length::Px(w)) = Length::parse(val) {
                style.borders.top.width = w;
                style.borders.right.width = w;
                style.borders.bottom.width = w;
                style.borders.left.width = w;
            }
        }
        "border-color" => {
            if let Some(c) = Color::parse(val) {
                style.borders.top.color = c;
                style.borders.right.color = c;
                style.borders.bottom.color = c;
                style.borders.left.color = c;
            }
        }
        "border-spacing" => {
            if let Some(l) = Length::parse(val) {
                style.border_spacing = l.resolve(0.0, style.font_size, 16.0).max(0.0);
            }
        }
        "border-radius" => {
            if let Some(Length::Px(r)) = Length::parse(val) {
                style.border_radius = r;
            }
        }
        "color" => {
            if let Some(c) = Color::parse(val) {
                style.color = c;
            }
        }
        "background" | "background-color" => {
            if let Some(c) = Color::parse(val) {
                style.background_color = c;
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
            if let Some(l) = Length::parse(val) {
                let size = l.resolve(style.font_size, style.font_size, 16.0);
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
                let h = l.resolve(style.font_size, style.font_size, 16.0);
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
        "z-index" => style.z_index = val.trim().parse::<i32>().ok(),
        "opacity" => {
            if let Ok(op) = val.trim().parse::<f32>() {
                if op.is_finite() {
                    style.opacity = op.clamp(0.0, 1.0);
                }
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
            if let Ok(g) = val.trim().parse::<f64>() {
                style.flex_grow = g;
            }
        }
        "flex-shrink" => {
            if let Ok(s) = val.trim().parse::<f64>() {
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
            let values: Vec<_> = val
                .split_ascii_whitespace()
                .filter_map(Length::parse)
                .collect();
            if let Some(v) = values.first() {
                style.row_gap = v.resolve(0.0, style.font_size, 16.0).max(0.0);
                style.column_gap = values
                    .get(1)
                    .unwrap_or(v)
                    .resolve(0.0, style.font_size, 16.0)
                    .max(0.0);
            }
        }
        "row-gap" => {
            if let Some(v) = Length::parse(val) {
                style.row_gap = v.resolve(0.0, style.font_size, 16.0).max(0.0);
            }
        }
        "column-gap" => {
            if let Some(v) = Length::parse(val) {
                style.column_gap = v.resolve(0.0, style.font_size, 16.0).max(0.0);
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
            let first = parts.next().map(str::trim).unwrap_or("");
            let second = parts.next().map(str::trim);
            let (start, span) = if let Some(sp) = first.strip_prefix("span ") {
                (None, sp.trim().parse::<usize>().unwrap_or(1))
            } else {
                let s = first.parse::<i32>().ok();
                let sp = if let Some(sec) = second {
                    if let Some(sp_str) = sec.strip_prefix("span ") {
                        sp_str.trim().parse::<usize>().unwrap_or(1)
                    } else if let Ok(e) = sec.parse::<i32>() {
                        s.map(|start_val| (e - start_val).max(1) as usize)
                            .unwrap_or(1)
                    } else {
                        1
                    }
                } else {
                    1
                };
                (s, sp)
            };
            let span = span.clamp(1, 256);
            if prop == "grid-column" {
                style.grid_column_start = start;
                style.grid_column_span = span;
            } else {
                style.grid_row_start = start;
                style.grid_row_span = span;
            }
        }
        "grid-column-start" => style.grid_column_start = val.trim().parse::<i32>().ok(),
        "grid-row-start" => style.grid_row_start = val.trim().parse::<i32>().ok(),
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
    "opacity"=>opacity,"visibility"=>visible,"pointer-events"=>pointer_events,"top"=>top,"right"=>right,"bottom"=>bottom,"left"=>left,"z-index"=>z_index,
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
}

pub(super) fn supported_property(property: &str) -> bool {
    matches!(property, "visibility" | "pointer-events" | "vertical-align")
        || property.starts_with("--")
        || matches!(
            property,
            "align-items"
                | "background"
                | "background-color"
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
