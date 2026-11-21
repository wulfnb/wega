//! Keyword-valued properties.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Float {
    #[default]
    None,
    Left,
    Right,
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
    pub fn parse(value: &str) -> Option<Self> {
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
