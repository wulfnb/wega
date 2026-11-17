//! Wega-owned Layout Engine.
//!
//! Converts DOM nodes and `ComputedStyle` into a Box Tree and performs
//! multi-pass layout calculations (Block formatting, Inline formatting,
//! Flexbox, Min/Max constraints, Positioning, and Overflow).

use crate::native_engine::css::*;
pub use wega_dom::layout::{BoxModel, ClientBox, ContentBox, Insets, Point, Rect, Size};
use wega_dom::{DocumentView, NodeKey, NodeKind, NodeView};

// ---------------------------------------------------------------------------
// HELPER FUNCTIONS
// ---------------------------------------------------------------------------

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
    pub scroll_offset: Point,
    pub scroll_width: f64,
    pub scroll_height: f64,
}

#[derive(Debug, Clone)]
pub struct LineBox {
    pub rect: Rect,
    pub text_runs: Vec<(Rect, String, Color, f64)>, // rect, text, color, font_size
}

impl LayoutBox {
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
            scroll_offset: Point::default(),
            scroll_width: 0.0,
            scroll_height: 0.0,
        }
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
                Display::None => return None,
            };

            let mut layout_box = LayoutBox::new(Some(node_key), box_type, style.clone());

            let mut raw_children = Vec::new();
            for child_key in node.children() {
                if let Some(child_box) = build_box_tree(doc, child_key, Some(&style), author_sheets)
                {
                    raw_children.push(child_box);
                }
            }

            // Anonymous block box generation for mixed inline and block siblings
            if box_type == BoxType::Block
                || box_type == BoxType::TableCell
                || box_type == BoxType::InlineBlock
            {
                let has_block_child = raw_children
                    .iter()
                    .any(|c| c.box_type != BoxType::Inline && c.box_type != BoxType::Text);
                let has_inline_child = raw_children
                    .iter()
                    .any(|c| c.box_type == BoxType::Inline || c.box_type == BoxType::Text);

                if has_block_child && has_inline_child {
                    let mut clustered = Vec::new();
                    let mut current_inlines = Vec::new();
                    for child in raw_children {
                        if child.box_type == BoxType::Inline || child.box_type == BoxType::Text {
                            current_inlines.push(child);
                        } else {
                            if !current_inlines.is_empty() {
                                let mut anon =
                                    LayoutBox::new(None, BoxType::AnonymousBlock, style.clone());
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
                if let Some(child_box) =
                    build_box_tree(doc, child_key, Some(&root_style), author_sheets)
                {
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
    layout_box(root, containing_block, containing_block, ctx);
}

fn layout_box(
    box_node: &mut LayoutBox,
    containing_block: Rect,
    positioned_cb: Rect,
    ctx: &LayoutContext,
) {
    match box_node.box_type {
        BoxType::Flex => layout_flex(box_node, containing_block, positioned_cb, ctx),
        BoxType::Grid => layout_grid(box_node, containing_block, positioned_cb, ctx),
        BoxType::Table => layout_table(box_node, containing_block, positioned_cb, ctx),
        _ => layout_block_or_flow(box_node, containing_block, positioned_cb, ctx),
    }
}

fn resolve_insets(
    lengths: &RectLengths,
    parent_w: f64,
    font_size: f64,
    root_font_size: f64,
) -> Insets {
    Insets {
        top: lengths.top.resolve(parent_w, font_size, root_font_size),
        right: lengths.right.resolve(parent_w, font_size, root_font_size),
        bottom: lengths.bottom.resolve(parent_w, font_size, root_font_size),
        left: lengths.left.resolve(parent_w, font_size, root_font_size),
    }
}

fn clamp_dimension(
    val: f64,
    min_len: Length,
    max_len: Length,
    parent_dim: f64,
    fs: f64,
    rfs: f64,
) -> f64 {
    let mut clamped = val;
    if max_len != Length::Auto {
        let max_val = max_len.resolve(parent_dim, fs, rfs);
        if clamped > max_val {
            clamped = max_val;
        }
    }
    if min_len != Length::Auto {
        let min_val = min_len.resolve(parent_dim, fs, rfs);
        if clamped < min_val {
            clamped = min_val;
        }
    }
    clamped.max(0.0)
}

fn layout_block_or_flow(
    box_node: &mut LayoutBox,
    containing_block: Rect,
    positioned_cb: Rect,
    ctx: &LayoutContext,
) {
    let fs = box_node.style.font_size;
    let rfs = ctx.font_size_root;
    let cb_w = containing_block.width;
    let cb_h = containing_block.height;

    box_node.padding = resolve_insets(&box_node.style.padding, cb_w, fs, rfs);
    box_node.border = Insets {
        top: box_node.style.borders.top.width,
        right: box_node.style.borders.right.width,
        bottom: box_node.style.borders.bottom.width,
        left: box_node.style.borders.left.width,
    };
    box_node.margin = resolve_insets(&box_node.style.margin, cb_w, fs, rfs);

    let total_border_w = box_node.border.left + box_node.border.right;
    let total_padding_w = box_node.padding.left + box_node.padding.right;
    let total_border_h = box_node.border.top + box_node.border.bottom;
    let total_padding_h = box_node.padding.top + box_node.padding.bottom;

    let is_absolute =
        box_node.style.position == Position::Absolute || box_node.style.position == Position::Fixed;
    let effective_cb = if is_absolute {
        positioned_cb
    } else {
        containing_block
    };

    // 1. Resolve content width
    let mut content_w = match box_node.style.width {
        Length::Px(px) => {
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (px - total_border_w - total_padding_w).max(0.0)
            } else {
                px
            }
        }
        Length::Percent(pct) => {
            let total = effective_cb.width * (pct / 100.0);
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (total - total_border_w - total_padding_w).max(0.0)
            } else {
                total
            }
        }
        Length::Auto => {
            if is_absolute {
                // If both left and right are specified, width is stretch
                if box_node.style.left != Length::Auto && box_node.style.right != Length::Auto {
                    let left_v = box_node.style.left.resolve(effective_cb.width, fs, rfs);
                    let right_v = box_node.style.right.resolve(effective_cb.width, fs, rfs);
                    (effective_cb.width
                        - left_v
                        - right_v
                        - box_node.margin.left
                        - box_node.margin.right
                        - total_border_w
                        - total_padding_w)
                        .max(0.0)
                } else {
                    0.0
                }
            } else if box_node.box_type == BoxType::Block
                || box_node.box_type == BoxType::AnonymousBlock
            {
                (cb_w
                    - box_node.margin.left
                    - box_node.margin.right
                    - total_border_w
                    - total_padding_w)
                    .max(0.0)
            } else {
                0.0 // Shrink-to-fit computed for inlines
            }
        }
        Length::Em(em) => em * fs,
        Length::Rem(rem) => rem * rfs,
    };

    // Apply min-width and max-width constraints
    content_w = clamp_dimension(
        content_w,
        box_node.style.min_width,
        box_node.style.max_width,
        effective_cb.width,
        fs,
        rfs,
    );

    let border_box_w = content_w + total_border_w + total_padding_w;

    // 2. Resolve X coordinate
    let x = if is_absolute {
        if box_node.style.left != Length::Auto {
            effective_cb.x
                + box_node.style.left.resolve(effective_cb.width, fs, rfs)
                + box_node.margin.left
        } else if box_node.style.right != Length::Auto {
            effective_cb.x + effective_cb.width
                - box_node.style.right.resolve(effective_cb.width, fs, rfs)
                - box_node.margin.right
                - border_box_w
        } else {
            effective_cb.x + box_node.margin.left
        }
    } else {
        containing_block.x + box_node.margin.left
    };

    // 3. Resolve Y coordinate
    let y = if is_absolute {
        if box_node.style.top != Length::Auto {
            effective_cb.y
                + box_node.style.top.resolve(effective_cb.height, fs, rfs)
                + box_node.margin.top
        } else if box_node.style.bottom != Length::Auto {
            effective_cb.y + effective_cb.height
                - box_node.style.bottom.resolve(effective_cb.height, fs, rfs)
                - box_node.margin.bottom
        } else {
            effective_cb.y + box_node.margin.top
        }
    } else {
        containing_block.y + box_node.margin.top
    };

    box_node.border_box.x = x;
    box_node.border_box.y = y;
    box_node.border_box.width = border_box_w;

    // Determine positioned containing block for children
    let this_is_pos = box_node.style.position != Position::Static;
    let next_positioned_cb = if this_is_pos {
        box_node.padding_box()
    } else {
        positioned_cb
    };

    // Check if children are inline/text or block
    let is_ifc = !box_node.children.is_empty()
        && box_node
            .children
            .iter()
            .all(|c| c.box_type == BoxType::Inline || c.box_type == BoxType::Text);

    let mut content_h = 0.0;

    if is_ifc {
        let lines = layout_inline_formatting_context(
            &mut box_node.children,
            content_w,
            fs,
            box_node.style.line_height,
            box_node.style.text_align,
        );
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
            let child_is_out_of_flow = child.style.position == Position::Absolute
                || child.style.position == Position::Fixed;

            let child_cb = Rect {
                x: inner_cb.x,
                y: cursor_y,
                width: inner_cb.width,
                height: 0.0,
            };

            layout_box(child, child_cb, next_positioned_cb, ctx);

            if !child_is_out_of_flow {
                let child_outer_h =
                    child.border_box.height + child.margin.top + child.margin.bottom;
                cursor_y += child_outer_h;
                content_h += child_outer_h;
            }
        }
    }

    // 4. Resolve content height
    let mut final_content_h = match box_node.style.height {
        Length::Px(px) => {
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (px - total_border_h - total_padding_h).max(0.0)
            } else {
                px
            }
        }
        Length::Percent(pct) => {
            let total = effective_cb.height * (pct / 100.0);
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (total - total_border_h - total_padding_h).max(0.0)
            } else {
                total
            }
        }
        Length::Auto => {
            if is_absolute
                && box_node.style.top != Length::Auto
                && box_node.style.bottom != Length::Auto
            {
                let top_v = box_node.style.top.resolve(effective_cb.height, fs, rfs);
                let bottom_v = box_node.style.bottom.resolve(effective_cb.height, fs, rfs);
                (effective_cb.height
                    - top_v
                    - bottom_v
                    - box_node.margin.top
                    - box_node.margin.bottom
                    - total_border_h
                    - total_padding_h)
                    .max(0.0)
            } else {
                content_h
            }
        }
        Length::Em(em) => em * fs,
        Length::Rem(rem) => rem * rfs,
    };

    // Apply min-height and max-height constraints
    final_content_h = clamp_dimension(
        final_content_h,
        box_node.style.min_height,
        box_node.style.max_height,
        effective_cb.height,
        fs,
        rfs,
    );

    let border_box_h = final_content_h + total_border_h + total_padding_h;
    box_node.border_box.height = border_box_h;

    // If absolute element was positioned by bottom only, adjust y now that height is known
    if is_absolute && box_node.style.top == Length::Auto && box_node.style.bottom != Length::Auto {
        box_node.border_box.y = effective_cb.y + effective_cb.height
            - box_node.style.bottom.resolve(effective_cb.height, fs, rfs)
            - box_node.margin.bottom
            - border_box_h;
    }

    // 5. Apply relative positioning offsets
    if box_node.style.position == Position::Relative {
        if box_node.style.left != Length::Auto {
            let left_off = box_node.style.left.resolve(cb_w, fs, rfs);
            box_node.border_box.x += left_off;
        } else if box_node.style.right != Length::Auto {
            let right_off = box_node.style.right.resolve(cb_w, fs, rfs);
            box_node.border_box.x -= right_off;
        }

        if box_node.style.top != Length::Auto {
            let top_off = box_node.style.top.resolve(cb_h, fs, rfs);
            box_node.border_box.y += top_off;
        } else if box_node.style.bottom != Length::Auto {
            let bottom_off = box_node.style.bottom.resolve(cb_h, fs, rfs);
            box_node.border_box.y -= bottom_off;
        }
    }

    // 6. Compute scrollable overflow dimensions
    let mut max_x = box_node.border_box.x + box_node.border_box.width;
    let mut max_y = box_node.border_box.y + box_node.border_box.height;
    for child in &box_node.children {
        max_x = max_x.max(child.border_box.x + child.border_box.width + child.margin.right);
        max_y = max_y.max(child.border_box.y + child.border_box.height + child.margin.bottom);
    }
    box_node.scroll_width = (max_x - box_node.border_box.x).max(box_node.border_box.width);
    box_node.scroll_height = (max_y - box_node.border_box.y).max(box_node.border_box.height);
}

// ---------------------------------------------------------------------------
// INLINE FORMATTING CONTEXT WITH FONT METRICS & TEXT WRAPPING
// ---------------------------------------------------------------------------

/// Estimate character width using proportional glyph metrics for typical sans-serif fonts.
fn char_advance(c: char, font_size: f64) -> f64 {
    let factor = match c {
        'i' | 'l' | '!' | '|' | ':' | ';' | '\'' | '.' | ',' => 0.28,
        'f' | 'r' | 't' | 'I' | 'j' | '(' | ')' | '[' | ']' => 0.35,
        'm' | 'w' | 'M' | 'W' => 0.85,
        'A' | 'B' | 'D' | 'G' | 'H' | 'N' | 'O' | 'Q' | 'R' | 'U' => 0.70,
        'C' | 'E' | 'F' | 'K' | 'P' | 'S' | 'T' | 'V' | 'X' | 'Y' | 'Z' => 0.65,
        ' ' => 0.33,
        _ => 0.52, // average lowercase letters and digits
    };
    factor * font_size
}

fn measure_word(word: &str, font_size: f64) -> f64 {
    word.chars().map(|c| char_advance(c, font_size)).sum()
}

fn collect_inline_runs(box_node: &LayoutBox, out: &mut Vec<(String, Color, f64)>) {
    if let Some(text) = &box_node.text_content {
        out.push((text.clone(), box_node.style.color, box_node.style.font_size));
    }
    for child in &box_node.children {
        collect_inline_runs(child, out);
    }
}

fn layout_inline_formatting_context(
    children: &mut [LayoutBox],
    avail_w: f64,
    font_size: f64,
    line_height: f64,
    align: TextAlign,
) -> Vec<LineBox> {
    let mut lines: Vec<LineBox> = Vec::new();
    let mut current_runs: Vec<(Rect, String, Color, f64)> = Vec::new();
    let mut current_x = 0.0;
    let mut current_y = 0.0;

    let space_w = char_advance(' ', font_size);

    // Collect all text runs from inline children
    let mut raw_runs = Vec::new();
    for child in children.iter() {
        collect_inline_runs(child, &mut raw_runs);
    }

    for (text, color, run_fs) in raw_runs {
        for word in text.split_whitespace() {
            let word_w = measure_word(word, run_fs);

            // Wrap line if word overflows current line
            if current_x + word_w > avail_w && current_x > 0.0 {
                let line_w = current_runs.last().map(|(r, ..)| r.x + r.width).unwrap_or(0.0);
                let line_rect = Rect {
                    x: 0.0,
                    y: current_y,
                    width: line_w,
                    height: line_height,
                };
                lines.push(LineBox {
                    rect: line_rect,
                    text_runs: std::mem::take(&mut current_runs),
                });
                current_y += line_height;
                current_x = 0.0;
            }

            let run_rect = Rect {
                x: current_x,
                y: current_y,
                width: word_w,
                height: line_height,
            };
            current_runs.push((run_rect, word.to_string(), color, run_fs));
            current_x += word_w + space_w;
        }
    }

    if !current_runs.is_empty() {
        let line_w = current_runs.last().map(|(r, ..)| r.x + r.width).unwrap_or(0.0);
        let line_rect = Rect {
            x: 0.0,
            y: current_y,
            width: line_w,
            height: line_height,
        };
        lines.push(LineBox {
            rect: line_rect,
            text_runs: current_runs,
        });
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
            for (r, _, _, _) in &mut line.text_runs {
                r.x += offset;
            }
        }
    }

    lines
}

// ---------------------------------------------------------------------------
// FLEXBOX ENGINE
// ---------------------------------------------------------------------------

fn layout_flex(
    box_node: &mut LayoutBox,
    containing_block: Rect,
    positioned_cb: Rect,
    ctx: &LayoutContext,
) {
    let fs = box_node.style.font_size;
    let rfs = ctx.font_size_root;
    let cb_w = containing_block.width;
    let cb_h = containing_block.height;

    box_node.padding = resolve_insets(&box_node.style.padding, cb_w, fs, rfs);
    box_node.border = Insets {
        top: box_node.style.borders.top.width,
        right: box_node.style.borders.right.width,
        bottom: box_node.style.borders.bottom.width,
        left: box_node.style.borders.left.width,
    };
    box_node.margin = resolve_insets(&box_node.style.margin, cb_w, fs, rfs);

    let total_border_w = box_node.border.left + box_node.border.right;
    let total_padding_w = box_node.padding.left + box_node.padding.right;
    let total_border_h = box_node.border.top + box_node.border.bottom;
    let total_padding_h = box_node.padding.top + box_node.padding.bottom;

    let mut container_w = match box_node.style.width {
        Length::Px(px) => {
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (px - total_border_w - total_padding_w).max(0.0)
            } else {
                px
            }
        }
        Length::Percent(pct) => {
            let total = cb_w * (pct / 100.0);
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (total - total_border_w - total_padding_w).max(0.0)
            } else {
                total
            }
        }
        _ => {
            (cb_w - box_node.margin.left - box_node.margin.right - total_border_w - total_padding_w)
                .max(0.0)
        }
    };

    container_w = clamp_dimension(
        container_w,
        box_node.style.min_width,
        box_node.style.max_width,
        cb_w,
        fs,
        rfs,
    );
    let border_box_w = container_w + total_border_w + total_padding_w;

    box_node.border_box.x = containing_block.x + box_node.margin.left;
    box_node.border_box.y = containing_block.y + box_node.margin.top;
    box_node.border_box.width = border_box_w;

    let inner_w = container_w;
    let is_row = matches!(
        box_node.style.flex_direction,
        FlexDirection::Row | FlexDirection::RowReverse
    );
    let is_reverse = matches!(
        box_node.style.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );

    let next_positioned_cb = if box_node.style.position != Position::Static {
        box_node.padding_box()
    } else {
        positioned_cb
    };

    // First pass: measure child natural/basis sizes and flex factors
    let mut total_grow = 0.0;
    let mut total_shrink = 0.0;
    let mut total_base_main = 0.0;
    let mut max_cross: f64 = 0.0;

    let mut in_flow_indices = Vec::new();

    for (idx, child) in box_node.children.iter_mut().enumerate() {
        if child.style.position == Position::Absolute || child.style.position == Position::Fixed {
            layout_box(
                child,
                Rect { x: 0.0, y: 0.0, width: inner_w, height: 0.0 },
                next_positioned_cb,
                ctx,
            );
            continue;
        }

        in_flow_indices.push(idx);

        let child_cb = Rect {
            x: 0.0,
            y: 0.0,
            width: inner_w,
            height: 0.0,
        };
        layout_box(child, child_cb, next_positioned_cb, ctx);

        let base_main = if child.style.flex_basis != Length::Auto {
            child
                .style
                .flex_basis
                .resolve(inner_w, child.style.font_size, rfs)
        } else if is_row {
            child.border_box.width
        } else {
            child.border_box.height
        };

        let base_cross = if is_row {
            child.border_box.height
        } else {
            child.border_box.width
        };

        total_base_main += base_main;
        max_cross = max_cross.max(base_cross);
        total_grow += child.style.flex_grow;
        total_shrink += child.style.flex_shrink;
    }

    let main_space = inner_w; // Available main space for row
    let free_space = main_space - total_base_main;

    // Distribute space or shrink
    let count = in_flow_indices.len();
    let mut gap_space = 0.0;
    let mut start_offset = 0.0;

    if free_space > 0.0 {
        if total_grow > 0.0 {
            for &idx in &in_flow_indices {
                let child = &mut box_node.children[idx];
                let extra = (child.style.flex_grow / total_grow) * free_space;
                if is_row {
                    child.border_box.width += extra;
                } else {
                    child.border_box.height += extra;
                }
            }
        } else {
            // Apply justify-content
            match box_node.style.justify_content {
                JustifyContent::FlexStart => start_offset = 0.0,
                JustifyContent::FlexEnd => start_offset = free_space,
                JustifyContent::Center => start_offset = free_space / 2.0,
                JustifyContent::SpaceBetween => {
                    if count > 1 {
                        gap_space = free_space / (count - 1) as f64;
                    }
                }
                JustifyContent::SpaceAround => {
                    if count > 0 {
                        gap_space = free_space / count as f64;
                        start_offset = gap_space / 2.0;
                    }
                }
                JustifyContent::SpaceEvenly => {
                    if count > 0 {
                        gap_space = free_space / (count + 1) as f64;
                        start_offset = gap_space;
                    }
                }
            }
        }
    } else if free_space < 0.0 && total_shrink > 0.0 {
        let overflow = -free_space;
        for &idx in &in_flow_indices {
            let child = &mut box_node.children[idx];
            let shrink_amount = (child.style.flex_shrink / total_shrink) * overflow;
            if is_row {
                child.border_box.width = (child.border_box.width - shrink_amount).max(0.0);
            } else {
                child.border_box.height = (child.border_box.height - shrink_amount).max(0.0);
            }
        }
    }

    // Position children along main and cross axes
    let inner_origin_x = box_node.border_box.x + box_node.border.left + box_node.padding.left;
    let inner_origin_y = box_node.border_box.y + box_node.border.top + box_node.padding.top;

    let mut cursor_main = if is_row {
        inner_origin_x + start_offset
    } else {
        inner_origin_y + start_offset
    };

    let cross_start = if is_row {
        inner_origin_y
    } else {
        inner_origin_x
    };
    let cross_avail = max_cross;

    let ordered_indices: Vec<usize> = if is_reverse {
        in_flow_indices.into_iter().rev().collect()
    } else {
        in_flow_indices
    };

    for idx in ordered_indices {
        let child = &mut box_node.children[idx];
        let child_cross = if is_row {
            child.border_box.height
        } else {
            child.border_box.width
        };

        // Align-items on cross axis
        let cross_pos = match box_node.style.align_items {
            AlignItems::FlexStart | AlignItems::Baseline => cross_start,
            AlignItems::FlexEnd => cross_start + (cross_avail - child_cross).max(0.0),
            AlignItems::Center => cross_start + (cross_avail - child_cross).max(0.0) / 2.0,
            AlignItems::Stretch => {
                if is_row {
                    child.border_box.height = cross_avail;
                } else {
                    child.border_box.width = cross_avail;
                }
                cross_start
            }
        };

        if is_row {
            child.border_box.x = cursor_main;
            child.border_box.y = cross_pos;
            cursor_main += child.border_box.width + gap_space;
        } else {
            child.border_box.x = cross_pos;
            child.border_box.y = cursor_main;
            cursor_main += child.border_box.height + gap_space;
        }
    }

    // Resolve container height
    let mut final_content_h = match box_node.style.height {
        Length::Px(px) => {
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (px - total_border_h - total_padding_h).max(0.0)
            } else {
                px
            }
        }
        Length::Percent(pct) => {
            let total = cb_h * (pct / 100.0);
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (total - total_border_h - total_padding_h).max(0.0)
            } else {
                total
            }
        }
        Length::Auto => {
            if is_row {
                max_cross
            } else {
                cursor_main - inner_origin_y
            }
        }
        Length::Em(em) => em * fs,
        Length::Rem(rem) => rem * rfs,
    };

    final_content_h = clamp_dimension(
        final_content_h,
        box_node.style.min_height,
        box_node.style.max_height,
        cb_h,
        fs,
        rfs,
    );

    let border_box_h = final_content_h + total_border_h + total_padding_h;
    box_node.border_box.height = border_box_h;

    // Apply relative positioning if applicable
    if box_node.style.position == Position::Relative {
        if box_node.style.left != Length::Auto {
            box_node.border_box.x += box_node.style.left.resolve(cb_w, fs, rfs);
        } else if box_node.style.right != Length::Auto {
            box_node.border_box.x -= box_node.style.right.resolve(cb_w, fs, rfs);
        }
        if box_node.style.top != Length::Auto {
            box_node.border_box.y += box_node.style.top.resolve(cb_h, fs, rfs);
        } else if box_node.style.bottom != Length::Auto {
            box_node.border_box.y -= box_node.style.bottom.resolve(cb_h, fs, rfs);
        }
    }

    let mut max_x = box_node.border_box.x + box_node.border_box.width;
    let mut max_y = box_node.border_box.y + box_node.border_box.height;
    for child in &box_node.children {
        max_x = max_x.max(child.border_box.x + child.border_box.width);
        max_y = max_y.max(child.border_box.y + child.border_box.height);
    }
    box_node.scroll_width = (max_x - box_node.border_box.x).max(box_node.border_box.width);
    box_node.scroll_height = (max_y - box_node.border_box.y).max(box_node.border_box.height);
}

// ---------------------------------------------------------------------------
// GRID & TABLE LAYOUT BASICS
// ---------------------------------------------------------------------------

fn layout_grid(
    box_node: &mut LayoutBox,
    containing_block: Rect,
    positioned_cb: Rect,
    ctx: &LayoutContext,
) {
    layout_block_or_flow(box_node, containing_block, positioned_cb, ctx);
}

fn layout_table(
    box_node: &mut LayoutBox,
    containing_block: Rect,
    positioned_cb: Rect,
    ctx: &LayoutContext,
) {
    layout_block_or_flow(box_node, containing_block, positioned_cb, ctx);
}

// ---------------------------------------------------------------------------
// HIT TESTING & GEOMETRY QUERIES
// ---------------------------------------------------------------------------

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
