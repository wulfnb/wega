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
    pub text_runs: Vec<super::text::GlyphRun>,
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

pub fn build_box_tree<D: DocumentView>(
    doc: &D,
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

            if node
                .element_name()
                .is_some_and(|n| n.local.eq_ignore_ascii_case("br"))
            {
                layout_box.text_content = Some("\n".into());
            }
            let mut raw_children = Vec::new();
            for child_key in node.children() {
                if let Some(child_box) = build_box_tree(doc, child_key, Some(&style), author_sheets)
                {
                    raw_children.push(child_box);
                }
            }

            // Whitespace-only anonymous items between block/flex/grid/table children
            // do not generate boxes. Keep them inside an inline formatting context.
            let has_block = raw_children
                .iter()
                .any(|c| !matches!(c.box_type, BoxType::Text | BoxType::Inline));
            if has_block
                || matches!(
                    box_type,
                    BoxType::Flex | BoxType::Grid | BoxType::Table | BoxType::TableRow
                )
            {
                raw_children.retain(|c| {
                    !(c.box_type == BoxType::Text
                        && matches!(
                            c.style.white_space,
                            WhiteSpace::Normal | WhiteSpace::NoWrap | WhiteSpace::PreLine
                        )
                        && c.text_content.as_ref().is_some_and(|t| {
                            t.chars()
                                .all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c'))
                        }))
                });
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
            let text = node.text()?;
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

fn layout_inline_formatting_context(
    children: &mut [LayoutBox],
    avail_w: f64,
    _font_size: f64,
    _line_height: f64,
    align: TextAlign,
) -> Vec<LineBox> {
    super::text::shape_inline(children, avail_w, align)
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
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: inner_w,
                    height: 0.0,
                },
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

    let container_w = match box_node.style.width {
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

    box_node.border_box.x = containing_block.x + box_node.margin.left;
    box_node.border_box.y = containing_block.y + box_node.margin.top;
    box_node.border_box.width = container_w + total_border_w + total_padding_w;

    let inner_origin_x = box_node.border_box.x + box_node.border.left + box_node.padding.left;
    let inner_origin_y = box_node.border_box.y + box_node.border.top + box_node.padding.top;
    let inner_w = container_w;

    // Resolve column track widths
    let num_cols = if !box_node.style.grid_template_columns.is_empty() {
        box_node.style.grid_template_columns.len()
    } else {
        1
    };

    let mut col_widths = vec![0.0; num_cols];
    let mut fixed_sum = 0.0;
    let mut auto_count = 0;

    for (c, len) in box_node.style.grid_template_columns.iter().enumerate() {
        match len {
            Length::Px(px) => {
                col_widths[c] = *px;
                fixed_sum += px;
            }
            Length::Percent(pct) => {
                let w = inner_w * (pct / 100.0);
                col_widths[c] = w;
                fixed_sum += w;
            }
            _ => auto_count += 1,
        }
    }

    let remaining_w = (inner_w - fixed_sum).max(0.0);
    let auto_w = if auto_count > 0 {
        remaining_w / auto_count as f64
    } else {
        0.0
    };

    for (c, len) in box_node.style.grid_template_columns.iter().enumerate() {
        if matches!(len, Length::Auto | Length::Em(_) | Length::Rem(_)) {
            col_widths[c] = auto_w;
        }
    }
    if box_node.style.grid_template_columns.is_empty() && num_cols == 1 {
        col_widths[0] = inner_w;
    }

    // Row tracks
    let default_row_h = 40.0;

    let next_positioned_cb = if box_node.style.position != Position::Static {
        box_node.padding_box()
    } else {
        positioned_cb
    };

    let mut auto_col = 0;
    let mut auto_row = 0;
    let mut max_row_seen = 0;

    for child in &mut box_node.children {
        if child.style.position == Position::Absolute || child.style.position == Position::Fixed {
            layout_box(
                child,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: inner_w,
                    height: 0.0,
                },
                next_positioned_cb,
                ctx,
            );
            continue;
        }

        let (col_idx, row_idx) = match (child.style.grid_column_start, child.style.grid_row_start) {
            (Some(c), Some(r)) => (
                ((c - 1).max(0) as usize).min(num_cols.saturating_sub(1)),
                (r - 1).max(0) as usize,
            ),
            (Some(c), None) => (
                ((c - 1).max(0) as usize).min(num_cols.saturating_sub(1)),
                auto_row,
            ),
            (None, Some(r)) => (auto_col, (r - 1).max(0) as usize),
            (None, None) => {
                let c = auto_col;
                let r = auto_row;
                auto_col += 1;
                if auto_col >= num_cols {
                    auto_col = 0;
                    auto_row += 1;
                }
                (c, r)
            }
        };

        max_row_seen = max_row_seen.max(row_idx);

        let cell_x = inner_origin_x + col_widths[..col_idx].iter().sum::<f64>();
        let cell_y = inner_origin_y + (row_idx as f64 * default_row_h);
        let cell_w = col_widths.get(col_idx).copied().unwrap_or(inner_w);
        let cell_h = default_row_h;

        let cell_cb = Rect {
            x: cell_x,
            y: cell_y,
            width: cell_w,
            height: cell_h,
        };
        layout_box(child, cell_cb, next_positioned_cb, ctx);

        child.border_box.x = cell_x;
        child.border_box.y = cell_y;
        child.border_box.width = cell_w;
        if child.border_box.height == 0.0 {
            child.border_box.height = cell_h;
        }
    }

    let total_rows_h = (max_row_seen + 1) as f64 * default_row_h;
    let final_content_h = match box_node.style.height {
        Length::Px(px) => px,
        Length::Percent(pct) => cb_h * (pct / 100.0),
        _ => total_rows_h,
    };

    box_node.border_box.height = final_content_h + total_border_h + total_padding_h;
}

fn layout_table(
    box_node: &mut LayoutBox,
    containing_block: Rect,
    positioned_cb: Rect,
    ctx: &LayoutContext,
) {
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

    let total_border_w = box_node.border.left + box_node.border.right;
    let total_padding_w = box_node.padding.left + box_node.padding.right;
    let total_border_h = box_node.border.top + box_node.border.bottom;
    let total_padding_h = box_node.padding.top + box_node.padding.bottom;

    let container_w = match box_node.style.width {
        Length::Px(px) => px,
        Length::Percent(pct) => cb_w * (pct / 100.0),
        _ => (cb_w - box_node.margin.left - box_node.margin.right).max(100.0),
    };

    box_node.border_box.x = containing_block.x + box_node.margin.left;
    box_node.border_box.y = containing_block.y + box_node.margin.top;
    box_node.border_box.width = container_w;

    let inner_origin_x = box_node.border_box.x + box_node.border.left + box_node.padding.left;
    let inner_origin_y = box_node.border_box.y + box_node.border.top + box_node.padding.top;
    let inner_w = (container_w - total_border_w - total_padding_w).max(0.0);

    // Collect all table rows whether directly under <table> or inside <tbody>/<thead>/<tfoot>
    let has_direct_rows = box_node
        .children
        .iter()
        .any(|c| c.box_type == BoxType::TableRow);
    let mut num_cols = 1;

    if has_direct_rows {
        for child in &box_node.children {
            if child.box_type == BoxType::TableRow {
                num_cols = num_cols.max(child.children.len());
            }
        }
    } else {
        for section in &box_node.children {
            for row in &section.children {
                if row.box_type == BoxType::TableRow {
                    num_cols = num_cols.max(row.children.len());
                }
            }
        }
    }

    let col_w = if num_cols > 0 {
        inner_w / num_cols as f64
    } else {
        inner_w
    };
    let mut cursor_y = inner_origin_y;

    if has_direct_rows {
        for row in &mut box_node.children {
            if row.box_type != BoxType::TableRow {
                continue;
            }
            layout_single_table_row(
                row,
                inner_origin_x,
                &mut cursor_y,
                inner_w,
                col_w,
                positioned_cb,
                ctx,
            );
        }
    } else {
        for section in &mut box_node.children {
            section.border_box.x = inner_origin_x;
            section.border_box.y = cursor_y;
            section.border_box.width = inner_w;
            let section_start_y = cursor_y;

            for row in &mut section.children {
                if row.box_type != BoxType::TableRow {
                    continue;
                }
                layout_single_table_row(
                    row,
                    inner_origin_x,
                    &mut cursor_y,
                    inner_w,
                    col_w,
                    positioned_cb,
                    ctx,
                );
            }
            section.border_box.height = cursor_y - section_start_y;
        }
    }

    let final_table_h = cursor_y - inner_origin_y;
    box_node.border_box.height = final_table_h + total_border_h + total_padding_h;
}

fn layout_single_table_row(
    row: &mut LayoutBox,
    inner_origin_x: f64,
    cursor_y: &mut f64,
    inner_w: f64,
    col_w: f64,
    positioned_cb: Rect,
    ctx: &LayoutContext,
) {
    row.border_box.x = inner_origin_x;
    row.border_box.y = *cursor_y;
    row.border_box.width = inner_w;

    let mut row_h = 30.0_f64;
    let mut cursor_x = inner_origin_x;

    for cell in &mut row.children {
        let cell_cb = Rect {
            x: cursor_x,
            y: *cursor_y,
            width: col_w,
            height: 0.0,
        };
        layout_box(cell, cell_cb, positioned_cb, ctx);
        cell.border_box.x = cursor_x;
        cell.border_box.y = *cursor_y;
        cell.border_box.width = col_w;
        row_h = row_h.max(cell.border_box.height);
        cursor_x += col_w;
    }

    for cell in &mut row.children {
        cell.border_box.height = row_h;
    }

    row.border_box.height = row_h;
    *cursor_y += row_h;
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
