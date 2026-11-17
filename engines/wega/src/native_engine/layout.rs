//! Wega Native Layout Engine: Block, Inline, Flexbox, Positioning, Grid, Table, and Box Tree.
use super::css::*;
use wega_dom::layout::{BoxModel, ClientBox, ContentBox, Insets, Point, Rect, Size};
use wega_dom::{DocumentView, NodeKey, NodeKind, NodeView};

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
    Text,
    AnonymousBlock,
}

#[derive(Debug, Clone)]
pub struct TextRun {
    pub text: String,
    pub width: f64,
}

#[derive(Debug, Clone)]
pub struct LineBox {
    pub rect: Rect,
    pub text_runs: Vec<(Rect, String)>,
}

#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub node: Option<NodeKey>,
    pub box_type: BoxType,
    pub style: ComputedStyle,
    pub border_box: Rect,
    pub margin: Insets,
    pub padding: Insets,
    pub border: Insets,
    pub children: Vec<LayoutBox>,
    pub lines: Vec<LineBox>,
    pub text_content: Option<String>,
    pub scroll_offset: Point,
    pub scroll_width: f64,
    pub scroll_height: f64,
}

impl LayoutBox {
    pub fn new(node: Option<NodeKey>, box_type: BoxType, style: ComputedStyle) -> Self {
        Self {
            node,
            box_type,
            style,
            border_box: Rect::ZERO,
            margin: Insets::default(),
            padding: Insets::default(),
            border: Insets::default(),
            children: Vec::new(),
            lines: Vec::new(),
            text_content: None,
            scroll_offset: Point::default(),
            scroll_width: 0.0,
            scroll_height: 0.0,
        }
    }

    pub fn content_box(&self) -> Rect {
        Rect {
            x: self.border_box.x + self.border.left + self.padding.left,
            y: self.border_box.y + self.border.top + self.padding.top,
            width: (self.border_box.width - self.border.left - self.border.right - self.padding.left - self.padding.right).max(0.0),
            height: (self.border_box.height - self.border.top - self.border.bottom - self.padding.top - self.padding.bottom).max(0.0),
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
            size: Size { width: c.width, height: c.height },
            padding_left: self.padding.left,
            padding_top: self.padding.top,
        }
    }
}

// ---------------------------------------------------------------------------
// BOX TREE BUILDER
// ---------------------------------------------------------------------------

pub fn build_box_tree<'a, D: DocumentView>(
    doc: &'a D,
    node_key: NodeKey,
    parent_style: Option<&ComputedStyle>,
    author_sheets: &[&StyleSheet],
) -> Option<LayoutBox> {
    let node = doc.node(node_key)?;
    match node.kind() {
        NodeKind::Element => {
            let style = resolve_computed_style(doc, node_key, parent_style, author_sheets);
            if style.display == Display::None {
                return None;
            }

            let box_type = match style.display {
                Display::Block => BoxType::Block,
                Display::Inline => BoxType::Inline,
                Display::InlineBlock => BoxType::InlineBlock,
                Display::Flex => BoxType::Flex,
                Display::Grid => BoxType::Grid,
                Display::Table => BoxType::Table,
                Display::TableRow => BoxType::TableRow,
                Display::TableCell => BoxType::TableCell,
                Display::None => unreachable!(),
            };

            let mut layout_box = LayoutBox::new(Some(node_key), box_type, style.clone());

            let mut raw_children: Vec<LayoutBox> = Vec::new();
            for child_key in node.children() {
                if let Some(child_box) = build_box_tree(doc, child_key, Some(&style), author_sheets) {
                    raw_children.push(child_box);
                }
            }

            if box_type == BoxType::Block || box_type == BoxType::TableCell || box_type == BoxType::InlineBlock {
                let has_block_child = raw_children.iter().any(|c| c.box_type != BoxType::Inline && c.box_type != BoxType::Text);
                let has_inline_child = raw_children.iter().any(|c| c.box_type == BoxType::Inline || c.box_type == BoxType::Text);

                if has_block_child && has_inline_child {
                    let mut clustered = Vec::new();
                    let mut current_inlines = Vec::new();
                    for child in raw_children {
                        if child.box_type == BoxType::Inline || child.box_type == BoxType::Text {
                            current_inlines.push(child);
                        } else {
                            if !current_inlines.is_empty() {
                                let mut anon = LayoutBox::new(None, BoxType::AnonymousBlock, style.clone());
                                anon.children = std::mem::take(&mut current_inlines);
                                clustered.push(anon);
                            }
                            clustered.push(child);
                        }
                    }
                    if !current_inlines.is_empty() {
                        let mut anon = LayoutBox::new(None, BoxType::AnonymousBlock, style.clone());
                        anon.children = current_inlines;
                        clustered.push(anon);
                    }
                    layout_box.children = clustered;
                } else {
                    layout_box.children = raw_children;
                }
            } else {
                layout_box.children = raw_children;
            }

            Some(layout_box)
        }
        NodeKind::Text => {
            let text = node.text()?.trim();
            if text.is_empty() {
                return None;
            }
            let style = parent_style.cloned().unwrap_or_default();
            let mut text_box = LayoutBox::new(Some(node_key), BoxType::Text, style);
            text_box.text_content = Some(text.to_string());
            Some(text_box)
        }
        NodeKind::Document => {
            let root_style = ComputedStyle {
                display: Display::Block,
                ..Default::default()
            };
            let mut root_box = LayoutBox::new(Some(node_key), BoxType::Block, root_style.clone());
            for child_key in node.children() {
                if let Some(child_box) = build_box_tree(doc, child_key, Some(&root_style), author_sheets) {
                    root_box.children.push(child_box);
                }
            }
            Some(root_box)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// LAYOUT ENGINE CALCULATIONS
// ---------------------------------------------------------------------------

pub struct LayoutContext {
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub font_size_root: f64,
}

pub fn perform_layout(root: &mut LayoutBox, ctx: &LayoutContext) {
    let containing_block = Rect {
        x: 0.0,
        y: 0.0,
        width: ctx.viewport_width,
        height: ctx.viewport_height,
    };
    layout_box(root, containing_block, ctx);
}

fn layout_box(box_node: &mut LayoutBox, containing_block: Rect, ctx: &LayoutContext) {
    match box_node.box_type {
        BoxType::Flex => layout_flex(box_node, containing_block, ctx),
        BoxType::Grid => layout_grid(box_node, containing_block, ctx),
        BoxType::Table => layout_table(box_node, containing_block, ctx),
        _ => layout_block_or_flow(box_node, containing_block, ctx),
    }
}

fn resolve_insets(lengths: &RectLengths, parent_w: f64, font_size: f64, root_font_size: f64) -> Insets {
    Insets {
        top: lengths.top.resolve(parent_w, font_size, root_font_size),
        right: lengths.right.resolve(parent_w, font_size, root_font_size),
        bottom: lengths.bottom.resolve(parent_w, font_size, root_font_size),
        left: lengths.left.resolve(parent_w, font_size, root_font_size),
    }
}

fn layout_block_or_flow(box_node: &mut LayoutBox, containing_block: Rect, ctx: &LayoutContext) {
    let fs = box_node.style.font_size;
    let rfs = ctx.font_size_root;
    let cb_w = containing_block.width;

    box_node.padding = resolve_insets(&box_node.style.padding, cb_w, fs, rfs);
    box_node.border = Insets {
        top: box_node.style.borders.top.width,
        right: box_node.style.borders.right.width,
        bottom: box_node.style.borders.bottom.width,
        left: box_node.style.borders.left.width,
    };
    box_node.margin = resolve_insets(&box_node.style.margin, cb_w, fs, rfs);

    // Compute width
    let content_w = match box_node.style.width {
        Length::Px(px) => {
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (px - box_node.border.left - box_node.border.right - box_node.padding.left - box_node.padding.right).max(0.0)
            } else {
                px
            }
        }
        Length::Percent(pct) => {
            let total = cb_w * (pct / 100.0);
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (total - box_node.border.left - box_node.border.right - box_node.padding.left - box_node.padding.right).max(0.0)
            } else {
                total
            }
        }
        Length::Auto => {
            if box_node.box_type == BoxType::Block || box_node.box_type == BoxType::AnonymousBlock {
                (cb_w - box_node.margin.left - box_node.margin.right - box_node.border.left - box_node.border.right - box_node.padding.left - box_node.padding.right).max(0.0)
            } else {
                0.0 // Shrink-to-fit computed later for inlines
            }
        }
        Length::Em(em) => em * fs,
        Length::Rem(rem) => rem * rfs,
    };

    let total_border_w = box_node.border.left + box_node.border.right;
    let total_padding_w = box_node.padding.left + box_node.padding.right;
    let border_box_w = content_w + total_border_w + total_padding_w;

    let x = containing_block.x + box_node.margin.left;
    let y = containing_block.y + box_node.margin.top;

    box_node.border_box.x = x;
    box_node.border_box.y = y;
    box_node.border_box.width = border_box_w;

    // Check if children are inline/text or block
    let is_ifc = box_node.children.iter().all(|c| c.box_type == BoxType::Inline || c.box_type == BoxType::Text);

    let mut content_h = 0.0;

    if is_ifc && !box_node.children.is_empty() {
        let lines = layout_inline_formatting_context(&mut box_node.children, content_w, fs, box_node.style.line_height, box_node.style.text_align);
        content_h = lines.iter().map(|l| l.rect.height).sum();
        box_node.lines = lines;
    } else {
        let mut cursor_y = y + box_node.border.top + box_node.padding.top;
        let inner_cb_x = x + box_node.border.left + box_node.padding.left;
        let inner_cb = Rect {
            x: inner_cb_x,
            y: cursor_y,
            width: content_w,
            height: 0.0,
        };

        for child in &mut box_node.children {
            let child_cb = Rect {
                x: inner_cb.x,
                y: cursor_y,
                width: inner_cb.width,
                height: 0.0,
            };
            layout_box(child, child_cb, ctx);
            let child_outer_h = child.border_box.height + child.margin.top + child.margin.bottom;
            cursor_y += child_outer_h;
            content_h += child_outer_h;
        }
    }

    // Compute height
    let final_content_h = match box_node.style.height {
        Length::Px(px) => {
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (px - box_node.border.top - box_node.border.bottom - box_node.padding.top - box_node.padding.bottom).max(0.0)
            } else {
                px
            }
        }
        Length::Percent(pct) => {
            let total = containing_block.height * (pct / 100.0);
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (total - box_node.border.top - box_node.border.bottom - box_node.padding.top - box_node.padding.bottom).max(0.0)
            } else {
                total
            }
        }
        Length::Auto => content_h,
        Length::Em(em) => em * fs,
        Length::Rem(rem) => rem * rfs,
    };

    box_node.border_box.height = final_content_h + box_node.border.top + box_node.border.bottom + box_node.padding.top + box_node.padding.bottom;

    // Apply relative positioning if specified
    if box_node.style.position == Position::Relative {
        let top_off = box_node.style.top.resolve(containing_block.height, fs, rfs);
        let left_off = box_node.style.left.resolve(containing_block.width, fs, rfs);
        box_node.border_box.x += left_off;
        box_node.border_box.y += top_off;
    }

    box_node.scroll_width = box_node.border_box.width;
    box_node.scroll_height = box_node.border_box.height.max(content_h);
}

// ---------------------------------------------------------------------------
// INLINE FORMATTING CONTEXT
// ---------------------------------------------------------------------------

fn layout_inline_formatting_context(
    children: &mut [LayoutBox],
    avail_w: f64,
    font_size: f64,
    line_height: f64,
    align: TextAlign,
) -> Vec<LineBox> {
    let mut lines: Vec<LineBox> = Vec::new();
    let char_w = font_size * 0.55;

    let mut current_runs: Vec<(Rect, String)> = Vec::new();
    let mut current_x = 0.0;
    let mut current_y = 0.0;

    for child in children.iter_mut() {
        if let Some(text) = &child.text_content {
            for word in text.split_whitespace() {
                let word_w = word.chars().count() as f64 * char_w;
                let space_w = char_w;

                if current_x + word_w > avail_w && current_x > 0.0 {
                    // Break line
                    let line_w = current_x;
                    let line_rect = Rect {
                        x: 0.0,
                        y: current_y,
                        width: line_w,
                        height: line_height,
                    };
                    lines.push(LineBox { rect: line_rect, text_runs: std::mem::take(&mut current_runs) });
                    current_y += line_height;
                    current_x = 0.0;
                }

                let run_rect = Rect {
                    x: current_x,
                    y: current_y,
                    width: word_w,
                    height: line_height,
                };
                current_runs.push((run_rect, word.to_string()));
                current_x += word_w + space_w;
            }
        }
    }

    if !current_runs.is_empty() {
        let line_w = current_x;
        let line_rect = Rect {
            x: 0.0,
            y: current_y,
            width: line_w,
            height: line_height,
        };
        lines.push(LineBox { rect: line_rect, text_runs: current_runs });
    }

    // Apply text alignment
    for line in &mut lines {
        let remaining = (avail_w - line.rect.width).max(0.0);
        let offset = match align {
            TextAlign::Left | TextAlign::Justify => 0.0,
            TextAlign::Center => remaining / 2.0,
            TextAlign::Right => remaining,
        };
        if offset > 0.0 {
            line.rect.x += offset;
            for (r, _) in &mut line.text_runs {
                r.x += offset;
            }
        }
    }

    lines
}

// ---------------------------------------------------------------------------
// FLEXBOX LAYOUT
// ---------------------------------------------------------------------------

fn layout_flex(box_node: &mut LayoutBox, containing_block: Rect, ctx: &LayoutContext) {
    let fs = box_node.style.font_size;
    let rfs = ctx.font_size_root;
    let cb_w = containing_block.width;

    box_node.padding = resolve_insets(&box_node.style.padding, cb_w, fs, rfs);
    box_node.border = Insets {
        top: box_node.style.borders.top.width,
        right: box_node.style.borders.right.width,
        bottom: box_node.style.borders.bottom.width,
        left: box_node.style.borders.left.width,
    };
    box_node.margin = resolve_insets(&box_node.style.margin, cb_w, fs, rfs);

    let container_w = match box_node.style.width {
        Length::Px(px) => px,
        Length::Percent(pct) => cb_w * (pct / 100.0),
        _ => cb_w - box_node.margin.left - box_node.margin.right,
    };

    box_node.border_box.x = containing_block.x + box_node.margin.left;
    box_node.border_box.y = containing_block.y + box_node.margin.top;
    box_node.border_box.width = container_w;

    let inner_w = (container_w - box_node.border.left - box_node.border.right - box_node.padding.left - box_node.padding.right).max(0.0);
    let is_row = matches!(box_node.style.flex_direction, FlexDirection::Row | FlexDirection::RowReverse);

    // First pass: measure child natural sizes
    let mut total_grow = 0.0;
    let mut total_base_main = 0.0;
    let mut max_cross: f64 = 0.0;

    for child in &mut box_node.children {
        let child_cb = Rect { x: 0.0, y: 0.0, width: inner_w, height: 0.0 };
        layout_box(child, child_cb, ctx);
        let base_main = if is_row { child.border_box.width } else { child.border_box.height };
        let base_cross = if is_row { child.border_box.height } else { child.border_box.width };
        total_base_main += base_main;
        max_cross = max_cross.max(base_cross);
        total_grow += child.style.flex_grow;
    }

    let free_space = (inner_w - total_base_main).max(0.0);

    // Distribute free space
    let mut cursor_main = if is_row {
        box_node.border_box.x + box_node.border.left + box_node.padding.left
    } else {
        box_node.border_box.y + box_node.border.top + box_node.padding.top
    };

    let cross_start = if is_row {
        box_node.border_box.y + box_node.border.top + box_node.padding.top
    } else {
        box_node.border_box.x + box_node.border.left + box_node.padding.left
    };

    for child in &mut box_node.children {
        let extra = if total_grow > 0.0 {
            (child.style.flex_grow / total_grow) * free_space
        } else {
            0.0
        };

        if is_row {
            child.border_box.x = cursor_main;
            child.border_box.y = cross_start;
            child.border_box.width += extra;
            cursor_main += child.border_box.width;
        } else {
            child.border_box.x = cross_start;
            child.border_box.y = cursor_main;
            child.border_box.height += extra;
            cursor_main += child.border_box.height;
        }
    }

    let final_h = if is_row {
        max_cross + box_node.border.top + box_node.border.bottom + box_node.padding.top + box_node.padding.bottom
    } else {
        cursor_main - box_node.border_box.y + box_node.border.bottom + box_node.padding.bottom
    };

    box_node.border_box.height = final_h;
}

// ---------------------------------------------------------------------------
// GRID & TABLE LAYOUT BASICS
// ---------------------------------------------------------------------------

fn layout_grid(box_node: &mut LayoutBox, containing_block: Rect, ctx: &LayoutContext) {
    layout_block_or_flow(box_node, containing_block, ctx);
}

fn layout_table(box_node: &mut LayoutBox, containing_block: Rect, ctx: &LayoutContext) {
    layout_block_or_flow(box_node, containing_block, ctx);
}

// ---------------------------------------------------------------------------
// HIT TESTING & GEOMETRY QUERIES
// ---------------------------------------------------------------------------

pub fn rect_contains(r: &Rect, p: Point) -> bool {
    p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
}

pub fn hit_test_node(box_node: &LayoutBox, point: Point) -> Option<NodeKey> {
    if !rect_contains(&box_node.border_box, point) {
        return None;
    }
    for child in box_node.children.iter().rev() {
        if let Some(target) = hit_test_node(child, point) {
            return Some(target);
        }
    }
    box_node.node
}
