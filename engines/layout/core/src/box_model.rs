//! Layout boxes and their geometry.
use crate::*;
use wega_dom::NodeKey;

pub fn rect_contains(r: &Rect, p: Point) -> bool {
    p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxType {
    Block,
    Inline,
    InlineBlock,
    Flex,
    Grid,
    Table,
    TableRow,
    TableCell,
    AnonymousBlock,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormControlKind {
    TextInput {
        is_password: bool,
        is_textarea: bool,
    },
    Checkbox,
    Radio,
    Button,
    Select,
}

#[derive(Debug, Clone, Default)]
pub struct FormState {
    pub kind: Option<FormControlKind>,
    pub checked: bool,
    pub disabled: bool,
    pub is_focused: bool,
    pub caret_rect: Option<Rect>,
    pub selection_rects: Vec<Rect>,
}

// ---------------------------------------------------------------------------
// BOX TREE REPRESENTATION
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub node: Option<NodeKey>,
    pub box_type: BoxType,
    pub style: ComputedStyle,
    pub border_box: Rect,
    pub padding: Insets,
    pub border: Insets,
    pub margin: Insets,
    pub children: Vec<LayoutBox>,
    pub lines: Vec<LineBox>,
    pub text_content: Option<String>,
    pub fragments: Vec<Rect>,
    pub image: Option<peniko::ImageData>,
    pub svg: Option<SvgGraphic>,
    pub background_image: Option<peniko::ImageData>,
    pub background_svg: Option<SvgGraphic>,
    pub is_root: bool,
    /// The `body` element child of the root element (overflow propagation).
    pub is_body: bool,
    pub flow_margin_top: f64,
    pub table_col_span: usize,
    pub table_row_span: usize,
    pub scroll_offset: Point,
    pub scroll_width: f64,
    pub scroll_height: f64,
    pub form_state: Option<FormState>,
    /// An inline-level flex, grid or table container (`inline-flex` etc.):
    /// laid out by its inner formatting context, placed as an atomic inline.
    pub inline_level: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedInlineBox {
    pub id: u64,
    pub rect: Rect,
}

#[derive(Debug, Clone)]
pub struct LineBox {
    pub rect: Rect,
    /// Baseline measured from the owning content box's top.
    pub baseline: f64,
    pub text_runs: Vec<crate::text::GlyphRun>,
    pub inline_boxes: Vec<PositionedInlineBox>,
}

impl LayoutBox {
    pub fn replaced_size(&self) -> Option<(f64, f64)> {
        self.image
            .as_ref()
            .map(|i| (i.width as f64, i.height as f64))
            .or_else(|| {
                self.svg
                    .as_ref()
                    .map(|t| (t.tree.size().width() as f64, t.tree.size().height() as f64))
            })
    }
    pub fn new(node: Option<NodeKey>, box_type: BoxType, style: ComputedStyle) -> Self {
        Self {
            node,
            box_type,
            style,
            border_box: Rect::default(),
            padding: Insets::default(),
            border: Insets::default(),
            margin: Insets::default(),
            children: Vec::new(),
            lines: Vec::new(),
            text_content: None,
            fragments: Vec::new(),
            image: None,
            svg: None,
            background_image: None,
            background_svg: None,
            is_root: false,
            is_body: false,
            flow_margin_top: 0.0,
            table_col_span: 1,
            table_row_span: 1,
            scroll_offset: Point::default(),
            scroll_width: 0.0,
            scroll_height: 0.0,
            form_state: None,
            inline_level: false,
        }
    }
    /// Used corner radius; percentages resolve against the border box.
    pub fn radius(&self) -> f64 {
        let short = self.border_box.width.min(self.border_box.height).max(0.0);
        (self.style.border_radius + self.style.border_radius_percent / 100.0 * short)
            .min(short / 2.0)
            .max(0.0)
    }
    /// A float taken out of normal flow (absolute positioning wins over float).
    pub fn is_floating(&self) -> bool {
        self.style.float != Float::None
            && !matches!(self.style.position, Position::Absolute | Position::Fixed)
            && !self.is_root
            && self.box_type != BoxType::Text
    }
    /// Atomic inline-level boxes participate in their parent's line boxes.
    pub fn is_atomic_inline(&self) -> bool {
        self.box_type == BoxType::InlineBlock || self.inline_level
    }
    pub fn is_block_level(&self) -> bool {
        !self.inline_level
            && matches!(
                self.box_type,
                BoxType::Block
                    | BoxType::Flex
                    | BoxType::Grid
                    | BoxType::Table
                    | BoxType::TableRow
                    | BoxType::AnonymousBlock
            )
    }

    pub fn content_box(&self) -> Rect {
        Rect {
            x: self.border_box.x + self.border.left + self.padding.left,
            y: self.border_box.y + self.border.top + self.padding.top,
            width: (self.border_box.width
                - self.border.left
                - self.border.right
                - self.padding.left
                - self.padding.right)
                .max(0.0),
            height: (self.border_box.height
                - self.border.top
                - self.border.bottom
                - self.padding.top
                - self.padding.bottom)
                .max(0.0),
        }
    }

    pub fn padding_box(&self) -> Rect {
        Rect {
            x: self.border_box.x + self.border.left,
            y: self.border_box.y + self.border.top,
            width: (self.border_box.width - self.border.left - self.border.right).max(0.0),
            height: (self.border_box.height - self.border.top - self.border.bottom).max(0.0),
        }
    }

    pub fn box_model(&self) -> BoxModel {
        BoxModel {
            border_box: self.border_box,
            border: self.border,
            padding: self.padding,
            margin: self.margin,
        }
    }

    pub fn client_box(&self) -> ClientBox {
        if self.style.display == Display::Inline {
            return ClientBox::default();
        }
        let p = self.padding_box();
        ClientBox {
            left: self.border.left,
            top: self.border.top,
            width: p.width,
            height: p.height,
        }
    }

    pub fn content_box_metrics(&self) -> ContentBox {
        let c = self.content_box();
        ContentBox {
            size: Size {
                width: c.width,
                height: c.height,
            },
            padding_left: self.padding.left,
            padding_top: self.padding.top,
        }
    }
}

// ---------------------------------------------------------------------------
// BOX TREE GENERATOR
// ---------------------------------------------------------------------------

/// Bounded native recursion on the 8 MiB engine thread. The larger legacy
/// layout limit is not safe for native debug frames (252-depth probe timed out).
pub const MAX_NATIVE_LAYOUT_DEPTH: usize = 128;
