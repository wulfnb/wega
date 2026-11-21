//! Wega-owned Layout Engine.
//!
//! Converts DOM nodes and `ComputedStyle` into a Box Tree and performs
//! multi-pass layout calculations (Block formatting, Inline formatting,
//! Flexbox, Min/Max constraints, Positioning, and Overflow).

use super::css::*;
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
    pub svg: Option<super::graphics::SvgGraphic>,
    pub background_image: Option<peniko::ImageData>,
    pub background_svg: Option<super::graphics::SvgGraphic>,
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
    pub text_runs: Vec<super::text::GlyphRun>,
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

pub fn build_box_tree<D: DocumentView>(
    doc: &D,
    node_key: NodeKey,
    parent_style: Option<&ComputedStyle>,
    author_sheets: &[&StyleSheet],
    media_ctx: Option<&MediaContext>,
) -> Option<LayoutBox> {
    let mut pending = vec![(node_key, 0usize)];
    let mut node_count = 0usize;
    let mut text_bytes = 0usize;
    while let Some((key, depth)) = pending.pop() {
        node_count += 1;
        if depth > MAX_NATIVE_LAYOUT_DEPTH || node_count > 30_000 || text_bytes > 4 * 1024 * 1024 {
            return None;
        }
        if let Some(n) = doc.node(key) {
            text_bytes = text_bytes.saturating_add(n.text().map(str::len).unwrap_or(0));
            if text_bytes > 4 * 1024 * 1024 {
                return None;
            }
            pending.extend(n.children().map(|k| (k, depth + 1)));
        }
    }
    // Styles flow down; completed boxes flow up. Keep traversal frames on the
    // heap so accepted-depth documents do not exhaust the engine thread stack.
    use std::rc::Rc;
    let mut work = vec![(node_key, parent_style.cloned().map(Rc::new), false)];
    let mut built = std::collections::HashMap::new();
    while let Some((key, parent, exit)) = work.pop() {
        let Some(node) = doc.node(key) else {
            continue;
        };
        if !exit {
            let style = match node.kind() {
                NodeKind::Element => Rc::new(resolve_computed_style(
                    doc,
                    key,
                    parent.as_deref(),
                    author_sheets,
                    media_ctx,
                )),
                NodeKind::Document => Rc::new(ComputedStyle {
                    display: Display::Block,
                    ..Default::default()
                }),
                NodeKind::Text => parent.unwrap_or_else(|| Rc::new(ComputedStyle::default())),
                _ => continue,
            };
            if style.display == Display::None {
                continue;
            }
            work.push((key, Some(style.clone()), true));
            if !node.element_name().is_some_and(|n| n.local == "svg") {
                work.extend(
                    node.children()
                        .rev()
                        .map(|child| (child, Some(style.clone()), false)),
                );
            }
        } else {
            let children: Vec<LayoutBox> = node
                .children()
                .filter_map(|child| built.remove(&child))
                .flatten()
                .collect();
            let style = parent.unwrap();
            let style = Rc::try_unwrap(style).unwrap_or_else(|s| (*s).clone());
            if style.display == Display::Contents
                && node.kind() == NodeKind::Element
                && key != node_key
            {
                // `display: contents` boxes are replaced by their children.
                built.insert(key, children);
            } else if let Some(b) = build_box_node(doc, key, style, children) {
                built.insert(key, vec![*b]);
            }
        }
    }
    built
        .remove(&node_key)
        .and_then(|mut b| (b.len() == 1).then(|| b.remove(0)))
}
fn build_box_node<D: DocumentView>(
    doc: &D,
    node_key: NodeKey,
    style: ComputedStyle,
    mut raw_children: Vec<LayoutBox>,
) -> Option<Box<LayoutBox>> {
    let node = doc.node(node_key)?;
    match node.kind() {
        NodeKind::Element => {
            if style.display == Display::None {
                return None;
            }

            let mut box_type = match style.display {
                Display::Block
                | Display::ListItem
                | Display::FlowRoot
                | Display::Contents
                | Display::TableCaption
                | Display::TableRowGroup
                | Display::TableHeaderGroup
                | Display::TableFooterGroup => BoxType::Block,
                Display::Inline => BoxType::Inline,
                Display::InlineBlock => BoxType::InlineBlock,
                Display::Flex | Display::InlineFlex => BoxType::Flex,
                Display::Grid | Display::InlineGrid => BoxType::Grid,
                Display::Table | Display::InlineTable => BoxType::Table,
                Display::TableRow => BoxType::TableRow,
                Display::TableCell => BoxType::TableCell,
                Display::None | Display::TableColumn | Display::TableColumnGroup => return None,
            };
            // Block-in-inline: an inline that contains block-level boxes is
            // split around them in CSS 2.1 §9.2.1.1. Approximate the split by
            // making the inline itself block-level, so its block children
            // stack instead of being shaped as inline content.
            if box_type == BoxType::Inline && raw_children.iter().any(LayoutBox::is_block_level) {
                box_type = BoxType::Block;
            }
            // Floats are blockified (CSS 2.1 §9.7).
            let floating = style.float != Float::None
                && !matches!(style.position, Position::Absolute | Position::Fixed);
            if floating && matches!(box_type, BoxType::Inline | BoxType::InlineBlock) {
                box_type = BoxType::Block;
            }

            let mut layout_box = Box::new(LayoutBox::new(Some(node_key), box_type, style.clone()));
            layout_box.inline_level = !floating
                && matches!(
                    style.display,
                    Display::InlineFlex | Display::InlineGrid | Display::InlineTable
                );
            layout_box.is_root = node.element_name().is_some_and(|n| n.local == "html")
                && node
                    .parent()
                    .and_then(|p| doc.node(p))
                    .is_some_and(|n| n.kind() == NodeKind::Document);
            layout_box.is_body = node.element_name().is_some_and(|n| n.local == "body")
                && node
                    .parent()
                    .and_then(|p| doc.node(p))
                    .and_then(|n| n.element_name().map(|e| e.local == "html"))
                    .unwrap_or(false);

            if node
                .element_name()
                .is_some_and(|n| n.local.eq_ignore_ascii_case("br"))
            {
                layout_box.text_content = Some("\n".into());
            }
            if let Some(el_name) = node.element_name() {
                let tag: &str = el_name.local;
                if tag.eq_ignore_ascii_case("input") {
                    let input_type = node.attr("type").unwrap_or("text").to_ascii_lowercase();
                    if input_type == "checkbox" {
                        layout_box.form_state = Some(FormState {
                            kind: Some(FormControlKind::Checkbox),
                            checked: node.attr("checked").is_some(),
                            disabled: node.attr("disabled").is_some(),
                            ..Default::default()
                        });
                    } else if input_type == "radio" {
                        layout_box.form_state = Some(FormState {
                            kind: Some(FormControlKind::Radio),
                            checked: node.attr("checked").is_some(),
                            disabled: node.attr("disabled").is_some(),
                            ..Default::default()
                        });
                    } else if matches!(input_type.as_str(), "submit" | "button" | "reset") {
                        layout_box.form_state = Some(FormState {
                            kind: Some(FormControlKind::Button),
                            disabled: node.attr("disabled").is_some(),
                            ..Default::default()
                        });
                    } else {
                        let is_password = input_type == "password";
                        layout_box.form_state = Some(FormState {
                            kind: Some(FormControlKind::TextInput {
                                is_password,
                                is_textarea: false,
                            }),
                            disabled: node.attr("disabled").is_some(),
                            ..Default::default()
                        });
                    }
                } else if tag.eq_ignore_ascii_case("textarea") {
                    layout_box.form_state = Some(FormState {
                        kind: Some(FormControlKind::TextInput {
                            is_password: false,
                            is_textarea: true,
                        }),
                        disabled: node.attr("disabled").is_some(),
                        ..Default::default()
                    });
                } else if tag.eq_ignore_ascii_case("select") {
                    layout_box.form_state = Some(FormState {
                        kind: Some(FormControlKind::Select),
                        disabled: node.attr("disabled").is_some(),
                        ..Default::default()
                    });
                } else if tag.eq_ignore_ascii_case("button") {
                    layout_box.form_state = Some(FormState {
                        kind: Some(FormControlKind::Button),
                        disabled: node.attr("disabled").is_some(),
                        ..Default::default()
                    });
                }
            }
            layout_box.table_col_span = node
                .attr("colspan")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 256);
            layout_box.table_row_span = node
                .attr("rowspan")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 256);
            if node.element_name().is_some_and(|n| n.local == "svg") {
                return Some(layout_box);
            }
            if let Some(ref form) = layout_box.form_state {
                match form.kind {
                    Some(FormControlKind::TextInput {
                        is_password,
                        is_textarea,
                    }) => {
                        if raw_children.is_empty() {
                            let val = node.attr("value").unwrap_or("");
                            let ph = node.attr("placeholder");
                            let (disp, is_ph) = if !val.is_empty() {
                                if is_password {
                                    ("•".repeat(val.chars().count()), false)
                                } else {
                                    (val.to_string(), false)
                                }
                            } else if let Some(placeholder) = ph {
                                (placeholder.to_string(), true)
                            } else {
                                (String::new(), false)
                            };
                            if !disp.is_empty() {
                                let mut text_style = style.clone();
                                if is_textarea {
                                    text_style.white_space = WhiteSpace::PreWrap;
                                }
                                if is_ph {
                                    text_style.color = Color {
                                        r: 117,
                                        g: 117,
                                        b: 117,
                                        a: 255,
                                    };
                                }
                                let mut text_child =
                                    LayoutBox::new(Some(node_key), BoxType::Text, text_style);
                                text_child.text_content = Some(disp);
                                raw_children.push(text_child);
                            }
                        }
                    }
                    Some(FormControlKind::Button) => {
                        if raw_children.is_empty() {
                            let val = node.attr("value").unwrap_or(
                                if node
                                    .attr("type")
                                    .is_some_and(|t| t.eq_ignore_ascii_case("submit"))
                                {
                                    "Submit"
                                } else {
                                    ""
                                },
                            );
                            if !val.is_empty() {
                                let mut btn_text =
                                    LayoutBox::new(Some(node_key), BoxType::Text, style.clone());
                                btn_text.text_content = Some(val.to_string());
                                raw_children.push(btn_text);
                            }
                        }
                    }
                    Some(FormControlKind::Select) => {
                        let mut selected_text = None;
                        for ck in node.children() {
                            if let Some(cn) = doc.node(ck)
                                && cn
                                    .element_name()
                                    .is_some_and(|e| e.local.eq_ignore_ascii_case("option"))
                                && (doc.element_state(ck).checked || selected_text.is_none())
                            {
                                selected_text = cn.text().map(str::to_string).or_else(|| {
                                    cn.children()
                                        .next()
                                        .and_then(|tk| doc.node(tk))
                                        .and_then(|t| t.text().map(str::to_string))
                                });
                            }
                        }
                        raw_children.clear();
                        if let Some(opt_text) = selected_text {
                            let mut opt_child =
                                LayoutBox::new(Some(node_key), BoxType::Text, style.clone());
                            opt_child.text_content = Some(opt_text);
                            raw_children.push(opt_child);
                        }
                    }
                    _ => {}
                }
            }

            // Whitespace-only anonymous items between block/flex/grid/table children
            // do not generate boxes. Keep them inside an inline formatting context.
            let has_block = raw_children.iter().any(LayoutBox::is_block_level);
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

            // Flex and grid items: each contiguous run of text becomes an
            // anonymous block item, and inline children are blockified.
            if matches!(box_type, BoxType::Flex | BoxType::Grid) {
                let mut items = Vec::with_capacity(raw_children.len());
                let mut run: Vec<LayoutBox> = Vec::new();
                let flush = |run: &mut Vec<LayoutBox>, items: &mut Vec<LayoutBox>| {
                    if !run.is_empty() {
                        let mut anon =
                            LayoutBox::new(None, BoxType::AnonymousBlock, anonymous_style(&style));
                        anon.children = std::mem::take(run);
                        items.push(anon);
                    }
                };
                for mut child in raw_children.drain(..) {
                    if child.box_type == BoxType::Text {
                        run.push(child);
                        continue;
                    }
                    flush(&mut run, &mut items);
                    if !matches!(child.style.position, Position::Absolute | Position::Fixed) {
                        if child.box_type == BoxType::Inline && child.text_content.is_none() {
                            child.box_type = BoxType::Block;
                        }
                        child.inline_level = false;
                    }
                    items.push(child);
                }
                flush(&mut run, &mut items);
                raw_children = items;
            }

            // Anonymous block box generation for mixed inline and block siblings
            if box_type == BoxType::Block
                || box_type == BoxType::TableCell
                || box_type == BoxType::InlineBlock
            {
                let is_block_level = LayoutBox::is_block_level;
                let has_block_child = raw_children.iter().any(is_block_level);
                let has_inline_child = raw_children.iter().any(|c| !is_block_level(c));
                if has_block_child && has_inline_child {
                    let mut clustered = Vec::new();
                    let mut current_inlines = Vec::new();
                    for child in raw_children {
                        if !is_block_level(&child) {
                            current_inlines.push(child);
                        } else {
                            if !current_inlines.is_empty() {
                                let mut anon = LayoutBox::new(
                                    None,
                                    BoxType::AnonymousBlock,
                                    anonymous_style(&style),
                                );
                                anon.children = std::mem::take(&mut current_inlines);
                                clustered.push(anon);
                            }
                            clustered.push(child);
                        }
                    }
                    if !current_inlines.is_empty() {
                        let mut anon =
                            LayoutBox::new(None, BoxType::AnonymousBlock, anonymous_style(&style));
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
            let mut style = style;
            style.position = Position::Static;
            let mut text_box = LayoutBox::new(Some(node_key), BoxType::Text, style);
            text_box.text_content = Some(text.to_string());
            Some(Box::new(text_box))
        }
        NodeKind::Document => {
            let root_style = ComputedStyle {
                display: Display::Block,
                ..Default::default()
            };
            let mut root_box = Box::new(LayoutBox::new(
                Some(node_key),
                BoxType::Block,
                root_style.clone(),
            ));
            root_box.is_root = true;
            root_box.children = raw_children;
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
    fn units(b: &mut LayoutBox, ctx: &LayoutContext) {
        let cv = |l: &mut Length| {
            *l = match *l {
                Length::Vw(v) => Length::Px(v * ctx.viewport_width / 100.0),
                Length::Vh(v) => Length::Px(v * ctx.viewport_height / 100.0),
                Length::Px(v) => Length::Px(v.clamp(-16_777_216.0, 16_777_216.0)),
                l => l,
            };
        };
        let s = &mut b.style;
        for l in [
            &mut s.width,
            &mut s.height,
            &mut s.min_width,
            &mut s.max_width,
            &mut s.min_height,
            &mut s.max_height,
            &mut s.top,
            &mut s.left,
            &mut s.right,
            &mut s.bottom,
            &mut s.flex_basis,
            &mut s.transform_origin.0,
            &mut s.transform_origin.1,
        ] {
            cv(l);
        }
        for r in [&mut s.margin, &mut s.padding] {
            for l in [&mut r.top, &mut r.right, &mut r.bottom, &mut r.left] {
                cv(l);
            }
        }
        fn track(t: &mut GridTrack, ctx: &LayoutContext) {
            match t {
                GridTrack::Length(l) => {
                    *l = match *l {
                        Length::Vw(v) => Length::Px(v * ctx.viewport_width / 100.0),
                        Length::Vh(v) => Length::Px(v * ctx.viewport_height / 100.0),
                        Length::Px(v) => Length::Px(v.clamp(-16_777_216.0, 16_777_216.0)),
                        l => l,
                    }
                }
                GridTrack::MinMax(l, max) => {
                    let mut min = GridTrack::Length(*l);
                    track(&mut min, ctx);
                    if let GridTrack::Length(v) = min {
                        *l = v;
                    }
                    track(max, ctx);
                }
                _ => {}
            }
        }
        for t in s
            .grid_template_columns
            .iter_mut()
            .chain(&mut s.grid_template_rows)
        {
            track(t, ctx);
        }
        for c in &mut b.children {
            units(c, ctx);
        }
    }
    units(root, ctx);
    propagate_viewport_overflow(root);
    indefinite_percent_heights(root, true, ctx);
    let containing_block = Rect {
        x: 0.0,
        y: 0.0,
        width: ctx.viewport_width,
        height: ctx.viewport_height,
    };
    layout_box(root, containing_block, containing_block, ctx);
}

/// CSS 2.1 §10.5 / css-sizing-3 §5.1: a percentage `height` (or `min-height`,
/// `max-height`) whose containing block's height is not definite behaves as
/// `auto` (`0` / `none`). The root element resolves against the initial
/// containing block, which is the viewport. Absolutely positioned boxes keep
/// their percentages because they resolve against a positioned padding box,
/// which is sized before they are laid out.
///
/// Reference: Stylo/Taffy treat these as "indefinite" through `MaybeResolve`
/// (`stylo_taffy/convert.rs`, `taffy::util::resolve`).
fn indefinite_percent_heights(b: &mut LayoutBox, definite: bool, ctx: &LayoutContext) {
    for c in &mut b.children {
        let absolute = matches!(c.style.position, Position::Absolute | Position::Fixed);
        let s = &mut c.style;
        if c.is_root {
            // The html element (a child of the document box).
            for l in [&mut s.height, &mut s.min_height, &mut s.max_height] {
                if let Length::Percent(p) = *l {
                    *l = Length::Px(p * ctx.viewport_height / 100.0);
                }
            }
        } else if !definite && !absolute {
            if has_percent(s.height) {
                s.height = Length::Auto;
            }
            if has_percent(s.min_height) {
                s.min_height = Length::Px(0.0);
            }
            if has_percent(s.max_height) {
                s.max_height = Length::Auto;
            }
        }
        let s = &c.style;
        let child_definite = match s.height {
            Length::Auto => {
                absolute && s.top != Length::Auto && s.bottom != Length::Auto
                    || matches!(c.box_type, BoxType::TableCell)
                    // Anonymous wrappers are transparent for percentage resolution.
                    || (c.node.is_none() && definite && !c.is_root)
            }
            l => !has_percent(l) || definite || absolute,
        };
        indefinite_percent_heights(c, child_definite, ctx);
    }
}

/// css-overflow-3 §3.3: `overflow` on the root element, or on `body` when the
/// root's is `visible`, applies to the viewport, not to the element's box. So
/// neither box clips its content (Google sets `body{overflow:hidden;height:100%}`).
/// The viewport itself does not yet honour `hidden`: the page still scrolls.
fn propagate_viewport_overflow(root: &mut LayoutBox) {
    let visible = |b: &LayoutBox| {
        b.style.overflow_x == Overflow::Visible && b.style.overflow_y == Overflow::Visible
    };
    let Some(html) = root.children.iter_mut().find(|c| c.is_root) else {
        return;
    };
    let html_visible = visible(html);
    html.style.overflow_x = Overflow::Visible;
    html.style.overflow_y = Overflow::Visible;
    if !html_visible {
        return;
    }
    if let Some(body) = html.children.iter_mut().find(|c| c.is_body) {
        body.style.overflow_x = Overflow::Visible;
        body.style.overflow_y = Overflow::Visible;
    }
}

fn has_percent(l: Length) -> bool {
    match l {
        Length::Percent(_) => true,
        Length::Calc(terms) => terms[1] != 0.0,
        Length::Clamp(parts) => parts.iter().any(|t| t[1] != 0.0),
        _ => false,
    }
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
    box_node.flow_margin_top = collapsed_top(box_node, cb_w, ctx);
    let collapse_children = collapses_top(box_node, cb_w, ctx);

    let total_border_w = box_node.border.left + box_node.border.right;
    let total_padding_w = box_node.padding.left + box_node.padding.right;
    let total_border_h = box_node.border.top + box_node.border.bottom;
    let total_padding_h = box_node.padding.top + box_node.padding.bottom;

    let is_absolute =
        box_node.style.position == Position::Absolute || box_node.style.position == Position::Fixed;
    let effective_cb = if box_node.style.position == Position::Fixed {
        Rect {
            x: 0.0,
            y: 0.0,
            width: ctx.viewport_width,
            height: ctx.viewport_height,
        }
    } else if is_absolute {
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
                    intrinsic_width(box_node, false)
                        .min(effective_cb.width)
                        .max(intrinsic_width(box_node, true))
                }
            } else if !box_node.inline_level
                && !box_node.is_floating()
                && matches!(
                    box_node.box_type,
                    BoxType::Block
                        | BoxType::AnonymousBlock
                        | BoxType::TableCell
                        | BoxType::Flex
                        | BoxType::Grid
                )
            {
                (cb_w
                    - box_node.margin.left
                    - box_node.margin.right
                    - total_border_w
                    - total_padding_w)
                    .max(0.0)
            } else {
                intrinsic_width(box_node, false)
                    .min(cb_w)
                    .max(intrinsic_width(box_node, true))
            }
        }
        Length::Em(_)
        | Length::Rem(_)
        | Length::Calc(_)
        | Length::Clamp(_)
        | Length::Vw(_)
        | Length::Vh(_) => {
            let width = box_node.style.width.resolve(cb_w, fs, rfs);
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (width - total_border_w - total_padding_w).max(0.0)
            } else {
                width
            }
        }
    };

    if box_node.style.width == Length::Auto
        && let Some((iw, ih)) = box_node.replaced_size()
    {
        if box_node.style.height != Length::Auto {
            content_w =
                box_node.style.height.resolve(effective_cb.height, fs, rfs) * iw / ih.max(1.0);
        } else {
            content_w = iw;
        }
    }
    if let Some(ref form) = box_node.form_state
        && matches!(
            form.kind,
            Some(FormControlKind::Checkbox | FormControlKind::Radio)
        )
        && box_node.style.width == Length::Auto
    {
        content_w = 13.0;
    }
    // Apply min-width and max-width constraints
    content_w = clamp_dimension(
        content_w,
        content_constraint(
            box_node.style.min_width,
            total_border_w + total_padding_w,
            box_node.style.box_sizing,
            effective_cb.width,
            fs,
            rfs,
        ),
        content_constraint(
            box_node.style.max_width,
            total_border_w + total_padding_w,
            box_node.style.box_sizing,
            effective_cb.width,
            fs,
            rfs,
        ),
        effective_cb.width,
        fs,
        rfs,
    );

    let border_box_w = content_w + total_border_w + total_padding_w;

    // Auto margins use the final constrained width. A block whose auto width
    // was reduced by max-width still distributes its remaining inline space.
    let block_axis = matches!(
        box_node.box_type,
        BoxType::Block | BoxType::AnonymousBlock | BoxType::Flex | BoxType::Grid | BoxType::Table
    );
    if !is_absolute && (box_node.style.width != Length::Auto || block_axis) {
        let remaining =
            (cb_w - border_box_w - box_node.margin.left - box_node.margin.right).max(0.0);
        match (
            box_node.style.margin.left == Length::Auto,
            box_node.style.margin.right == Length::Auto,
        ) {
            (true, true) => {
                box_node.margin.left = remaining / 2.0;
                box_node.margin.right = remaining / 2.0;
            }
            (true, false) => box_node.margin.left = remaining,
            (false, true) => box_node.margin.right = remaining,
            _ => {}
        }
    }
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
        containing_block.y + box_node.flow_margin_top
    };

    box_node.border_box.x = x;
    box_node.border_box.y = y;
    box_node.border_box.width = border_box_w;
    box_node.border_box.height = if box_node.style.height == Length::Auto {
        0.0
    } else {
        let h = box_node.style.height.resolve(effective_cb.height, fs, rfs);
        if box_node.style.box_sizing == BoxSizing::BorderBox {
            h
        } else {
            h + total_border_h + total_padding_h
        }
    };

    // Determine positioned containing block for children
    let this_is_pos = box_node.style.position != Position::Static;
    let next_positioned_cb = if this_is_pos {
        box_node.padding_box()
    } else {
        positioned_cb
    };

    let is_inline_level = |c: &LayoutBox| {
        c.box_type == BoxType::Inline || c.box_type == BoxType::Text || c.is_atomic_inline()
    };
    let is_ifc = !box_node.children.is_empty() && box_node.children.iter().all(is_inline_level);

    let mut content_h = if let Some((iw, ih)) = box_node.replaced_size() {
        if box_node.style.height != Length::Auto {
            box_node.style.height.resolve(effective_cb.height, fs, rfs)
        } else {
            content_w * ih / iw.max(1.0)
        }
    } else {
        0.0
    };

    if is_ifc {
        let inner_x = x + box_node.border.left + box_node.padding.left;
        let inner_y = y + box_node.border.top + box_node.padding.top;
        let inner_cb = Rect {
            x: inner_x,
            y: inner_y,
            width: content_w,
            height: 0.0,
        };
        fn prepare_inline(b: &mut LayoutBox, cb: Rect, positioned: Rect, ctx: &LayoutContext) {
            if b.is_atomic_inline() || b.form_state.is_some() || b.replaced_size().is_some() {
                layout_box(b, cb, positioned, ctx);
            } else {
                for child in &mut b.children {
                    prepare_inline(child, cb, positioned, ctx);
                }
            }
        }
        for child in &mut box_node.children {
            prepare_inline(child, inner_cb, next_positioned_cb, ctx);
        }
        let lines =
            layout_inline_formatting_context(&mut box_node.children, content_w, &box_node.style);
        content_h = lines.iter().map(|l| l.rect.height).sum();
        box_node.lines = lines;
        assign_inline_geometry(&mut box_node.children, &box_node.lines, inner_x, inner_y);
    } else {
        let mut cursor_y = y + box_node.border.top + box_node.padding.top;
        let inner_cb_x = x + box_node.border.left + box_node.padding.left;
        let inner_cb = Rect {
            x: inner_cb_x,
            y: cursor_y,
            width: content_w,
            height: box_node.content_box().height,
        };
        let mut previous_bottom: f64 = 0.0;
        let mut first = true;
        // Margin boxes of floats placed in this block, with their side.
        let mut floats: Vec<(Rect, bool)> = Vec::new();
        let edges = |floats: &[(Rect, bool)], top: f64, bottom: f64| {
            let (mut left, mut right) = (inner_cb.x, inner_cb.x + inner_cb.width);
            for (f, is_left) in floats {
                if f.y < bottom && f.bottom() > top {
                    if *is_left {
                        left = left.max(f.right());
                    } else {
                        right = right.min(f.x);
                    }
                }
            }
            (left, right)
        };
        let clearance = |floats: &[(Rect, bool)], clear: (bool, bool)| {
            floats
                .iter()
                .filter(|(_, is_left)| if *is_left { clear.0 } else { clear.1 })
                .map(|(f, _)| f.bottom())
                .fold(f64::NEG_INFINITY, f64::max)
        };
        for child in &mut box_node.children {
            let child_is_out_of_flow = child.style.position == Position::Absolute
                || child.style.position == Position::Fixed;

            if child_is_out_of_flow {
                continue;
            }
            if child.is_floating() {
                let is_left = child.style.float == Float::Left;
                let probe = Rect {
                    x: inner_cb.x,
                    y: cursor_y,
                    width: inner_cb.width,
                    height: inner_cb.height,
                };
                layout_box(child, probe, next_positioned_cb, ctx);
                let w = child.border_box.width + child.margin.left + child.margin.right;
                let h = child.border_box.height + child.margin.top + child.margin.bottom;
                // A float's top is no higher than earlier floats or its clearance.
                let mut top = floats
                    .iter()
                    .map(|(f, _)| f.y)
                    .fold(cursor_y, f64::max)
                    .max(clearance(&floats, child.style.clear));
                let mut x = if is_left {
                    inner_cb.x
                } else {
                    inner_cb.x + inner_cb.width - w
                };
                for _ in 0..64 {
                    let (left, right) = edges(&floats, top, top + h.max(0.01));
                    let blocked = floats
                        .iter()
                        .any(|(f, _)| f.y < top + h.max(0.01) && f.bottom() > top);
                    if right - left >= w || !blocked {
                        x = if is_left { left } else { right - w };
                        break;
                    }
                    let next = floats
                        .iter()
                        .map(|(f, _)| f.bottom())
                        .filter(|b| *b > top)
                        .fold(f64::INFINITY, f64::min);
                    if !next.is_finite() {
                        break;
                    }
                    top = next;
                }
                let dx = x - (child.border_box.x - child.margin.left);
                let dy = top - (child.border_box.y - child.margin.top);
                translate_tree(child, dx, dy);
                floats.push((
                    Rect {
                        x,
                        y: top,
                        width: w,
                        height: h,
                    },
                    is_left,
                ));
                continue;
            }
            let cleared = clearance(&floats, child.style.clear);
            if cleared > cursor_y {
                cursor_y = cleared;
                previous_bottom = 0.0;
                first = true;
            }
            let top = collapsed_top(child, content_w, ctx);
            if !first {
                let collapsed =
                    previous_bottom.max(top).max(0.0) + previous_bottom.min(top).min(0.0);
                cursor_y += collapsed - previous_bottom - top;
            }
            // Content beside a float is narrowed by the part of the float that
            // reaches past the box's own margin (approximating short lines).
            let (left_edge, right_edge) = edges(&floats, cursor_y, cursor_y + 1.0);
            let margins = resolve_insets(
                &child.style.margin,
                inner_cb.width,
                child.style.font_size,
                ctx.font_size_root,
            );
            let intrude_left = (left_edge - inner_cb.x - margins.left).max(0.0);
            let intrude_right = (inner_cb.x + inner_cb.width - right_edge - margins.right).max(0.0);
            let child_cb = Rect {
                x: inner_cb.x + intrude_left,
                y: cursor_y - if first && collapse_children { top } else { 0.0 },
                width: (inner_cb.width - intrude_left - intrude_right).max(0.0),
                height: inner_cb.height,
            };
            layout_box(child, child_cb, next_positioned_cb, ctx);

            if !child_is_out_of_flow {
                let child_outer_h =
                    child.border_box.height + child.flow_margin_top + child.margin.bottom
                        - if first && collapse_children {
                            child.flow_margin_top
                        } else {
                            0.0
                        };
                cursor_y += child_outer_h;
                content_h = cursor_y - inner_cb.y;
                previous_bottom = child.margin.bottom;
                first = false;
            }
        }
        // Blocks grow to contain their floats. Strictly only formatting-context
        // roots do; this also stands in for clearfix pseudo-elements, which
        // are not generated yet.
        let float_bottom = floats
            .iter()
            .map(|(f, _)| f.bottom())
            .fold(f64::NEG_INFINITY, f64::max);
        if float_bottom.is_finite() {
            content_h = content_h.max(float_bottom - inner_cb.y);
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
            if let Some(ref form) = box_node.form_state {
                if matches!(
                    form.kind,
                    Some(FormControlKind::Checkbox | FormControlKind::Radio)
                ) {
                    13.0
                } else if !matches!(
                    form.kind,
                    Some(FormControlKind::TextInput {
                        is_textarea: true,
                        ..
                    })
                ) && content_h < fs * box_node.style.line_height_factor.unwrap_or(1.25)
                    && !is_absolute
                {
                    // Empty single-line controls keep one line of height.
                    fs * box_node.style.line_height_factor.unwrap_or(1.25)
                } else if is_absolute
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
            } else if is_absolute
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
        Length::Em(_)
        | Length::Rem(_)
        | Length::Calc(_)
        | Length::Clamp(_)
        | Length::Vw(_)
        | Length::Vh(_) => {
            let height = box_node.style.height.resolve(cb_h, fs, rfs);
            if box_node.style.box_sizing == BoxSizing::BorderBox {
                (height - total_border_h - total_padding_h).max(0.0)
            } else {
                height
            }
        }
    };

    // Apply min-height and max-height constraints
    final_content_h = clamp_dimension(
        final_content_h,
        content_constraint(
            box_node.style.min_height,
            total_border_h + total_padding_h,
            box_node.style.box_sizing,
            effective_cb.height,
            fs,
            rfs,
        ),
        content_constraint(
            box_node.style.max_height,
            total_border_h + total_padding_h,
            box_node.style.box_sizing,
            effective_cb.height,
            fs,
            rfs,
        ),
        effective_cb.height,
        fs,
        rfs,
    );

    let border_box_h = final_content_h + total_border_h + total_padding_h;
    box_node.border_box.height = border_box_h;
    if let Some(ref form) = box_node.form_state
        && matches!(
            form.kind,
            Some(
                FormControlKind::TextInput {
                    is_textarea: false,
                    ..
                } | FormControlKind::Button
                    | FormControlKind::Select
            )
        )
        && !box_node.lines.is_empty()
    {
        // Single-line controls center their text in the content box. Glyphs
        // are positioned individually, so move the runs, not just the line.
        let cb = box_node.content_box();
        let is_button = matches!(form.kind, Some(FormControlKind::Button));
        let lines_h: f64 = box_node.lines.iter().map(|l| l.rect.height).sum();
        let top = box_node.lines[0].rect.y;
        let dy = if cb.height > lines_h {
            (cb.height - lines_h) / 2.0 - top
        } else {
            0.0
        };
        let runs = || box_node.lines.iter().flat_map(|l| &l.text_runs);
        let left = runs().map(|r| r.rect.x).fold(f64::INFINITY, f64::min);
        let right = runs()
            .map(|r| r.rect.right())
            .fold(f64::NEG_INFINITY, f64::max);
        let dx = if is_button && left.is_finite() && cb.width > right - left {
            (cb.width - (right - left)) / 2.0 - left
        } else {
            0.0
        };
        if dx != 0.0 || dy != 0.0 {
            for line in &mut box_node.lines {
                line.rect.y += dy;
                for run in &mut line.text_runs {
                    run.rect.x += dx;
                    run.rect.y += dy;
                    for g in &mut run.glyphs {
                        g.x += dx as f32;
                        g.y += dy as f32;
                    }
                }
                for ib in &mut line.inline_boxes {
                    ib.rect.x += dx;
                    ib.rect.y += dy;
                }
            }
        }
    }

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

    // Move descendants with relative/bottom positioning; local line geometry
    // stays relative to its owning block.
    let dx = box_node.border_box.x - x;
    let dy = box_node.border_box.y - y;
    for child in &mut box_node.children {
        if !matches!(child.style.position, Position::Absolute | Position::Fixed) {
            translate_tree(child, dx, dy);
        }
    }
    let positioned = if this_is_pos {
        box_node.padding_box()
    } else {
        positioned_cb
    };
    let content = box_node.content_box();
    for child in &mut box_node.children {
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            layout_box(child, content, positioned, ctx);
        }
    }
    update_overflow(box_node);
}

// ---------------------------------------------------------------------------
// INLINE FORMATTING CONTEXT WITH FONT METRICS & TEXT WRAPPING
// ---------------------------------------------------------------------------

fn layout_inline_formatting_context(
    children: &mut [LayoutBox],
    avail_w: f64,
    style: &ComputedStyle,
) -> Vec<LineBox> {
    super::text::shape_inline_with_style(children, avail_w, style.text_align, Some(style))
}

// ---------------------------------------------------------------------------
// FLEXBOX ENGINE
// ---------------------------------------------------------------------------

// Initialize the container through the same block sizing/positioning code used
// elsewhere; layout algorithms below only decide the positions of its contents.
fn init_container(b: &mut LayoutBox, cb: Rect, positioned: Rect, ctx: &LayoutContext) -> Rect {
    // Inline-level containers and auto-width tables shrink to fit their contents. Measure while the
    // children are still attached; the container is then sized without them.
    let width = b.style.width;
    let absolute = matches!(b.style.position, Position::Absolute | Position::Fixed);
    if (b.inline_level || b.is_floating() || (b.box_type == BoxType::Table && !absolute))
        && width == Length::Auto
    {
        let padding = resolve_insets(
            &b.style.padding,
            cb.width,
            b.style.font_size,
            ctx.font_size_root,
        );
        let frame =
            padding.left + padding.right + b.style.borders.left.width + b.style.borders.right.width;
        let margin = resolve_insets(
            &b.style.margin,
            cb.width,
            b.style.font_size,
            ctx.font_size_root,
        );
        let available = (cb.width - frame - margin.left - margin.right).max(0.0);
        let fit = intrinsic_width(b, false)
            .min(available)
            .max(intrinsic_width(b, true));
        let extra = if b.style.box_sizing == BoxSizing::BorderBox {
            frame
        } else {
            0.0
        };
        b.style.width = Length::Px(fit + extra);
    }
    let children = std::mem::take(&mut b.children);
    layout_block_or_flow(b, cb, positioned, ctx);
    b.children = children;
    b.style.width = width;
    b.content_box()
}
fn translate_tree(b: &mut LayoutBox, dx: f64, dy: f64) {
    b.border_box.x += dx;
    b.border_box.y += dy;
    for r in &mut b.fragments {
        r.x += dx;
        r.y += dy;
    }
    if let Some(ref mut form) = b.form_state {
        if let Some(ref mut caret) = form.caret_rect {
            caret.x += dx;
            caret.y += dy;
        }
        for sel in &mut form.selection_rects {
            sel.x += dx;
            sel.y += dy;
        }
    }
    for c in &mut b.children {
        if c.style.position != Position::Fixed {
            translate_tree(c, dx, dy);
        }
    }
}
/// Min-content (`min`) or max-content width of `b`'s content box. The box's
/// own `width` is the caller's concern; descendants contribute their outer
/// sizes, honouring definite widths. Atomic inlines are measured from their
/// contents because their own geometry is not laid out yet.
fn intrinsic_width(b: &LayoutBox, min: bool) -> f64 {
    intrinsic_content(b, min, 0)
}
/// Shape as layout will: with the owning block's style as the root, so the
/// measured width matches the width the line breaker later sees.
fn shaped_width(children: &[LayoutBox], min: bool, owner: &ComputedStyle) -> f64 {
    super::text::shape_inline_with_style(
        children,
        if min { 0.0 } else { 1e7 },
        TextAlign::Left,
        Some(owner),
    )
    .iter()
    .map(|l| l.rect.width)
    .fold(0.0, f64::max)
}
fn definite(length: Length, b: &LayoutBox) -> Option<f64> {
    match length {
        Length::Auto | Length::Percent(_) => None,
        Length::Calc(v) if v[1] != 0.0 => None,
        Length::Clamp(v) if v.iter().any(|t| t[1] != 0.0) => None,
        l => Some(l.resolve(0.0, b.style.font_size, b.style.root_font_size)),
    }
}
fn has_atomic_inline(children: &[LayoutBox]) -> bool {
    children.iter().any(|c| {
        c.is_atomic_inline()
            || c.form_state.is_some()
            || (c.box_type == BoxType::Inline && has_atomic_inline(&c.children))
    })
}
fn intrinsic_content(b: &LayoutBox, min: bool, depth: usize) -> f64 {
    if let Some((iw, _)) = b.replaced_size() {
        return iw;
    }
    if b.box_type == BoxType::Text || b.text_content.is_some() {
        return shaped_width(std::slice::from_ref(b), min, &b.style);
    }
    if b.children.is_empty() {
        return 0.0;
    }
    // Controls whose content is generated text (an input's value, a select's
    // label) are measured as that text; a <button> with element content is
    // measured like any other box.
    if b.form_state.is_some() && b.children.iter().all(|c| c.box_type == BoxType::Text) {
        return shaped_width(&b.children, min, &b.style);
    }
    if depth > 96 {
        return shaped_width(&b.children, min, &b.style);
    }
    let in_flow = || {
        b.children
            .iter()
            .filter(|c| !matches!(c.style.position, Position::Absolute | Position::Fixed))
    };
    let row_flex = b.box_type == BoxType::Flex
        && matches!(
            b.style.flex_direction,
            FlexDirection::Row | FlexDirection::RowReverse
        );
    if row_flex || b.box_type == BoxType::TableRow {
        let sizes: Vec<f64> = in_flow().map(|c| outer_intrinsic(c, min, depth)).collect();
        return if min && b.style.flex_wrap {
            sizes.into_iter().fold(0.0, f64::max)
        } else {
            let gaps = b.style.column_gap * sizes.len().saturating_sub(1) as f64;
            sizes.into_iter().sum::<f64>() + gaps
        };
    }
    if b.box_type == BoxType::Grid && b.style.grid_template_columns.len() > 1 {
        let columns = b.style.grid_template_columns.len();
        let sizes: Vec<f64> = in_flow().map(|c| outer_intrinsic(c, min, depth)).collect();
        let gaps = b.style.column_gap * (columns.min(sizes.len()).saturating_sub(1)) as f64;
        return sizes
            .chunks(columns)
            .map(|row| row.iter().sum::<f64>())
            .fold(0.0, f64::max)
            + gaps;
    }
    let inline_level = |c: &LayoutBox| {
        matches!(c.box_type, BoxType::Inline | BoxType::Text) || c.is_atomic_inline()
    };
    if b.box_type != BoxType::Flex && b.children.iter().all(inline_level) {
        if !has_atomic_inline(&b.children) {
            return shaped_width(&b.children, min, &b.style);
        }
        return inline_pieces(&b.children, min, depth);
    }
    in_flow()
        .map(|c| outer_intrinsic(c, min, depth))
        .fold(0.0, f64::max)
}
/// Inline content containing atomic boxes: text runs are shaped on their own
/// and atoms contribute their outer sizes. Max-content sums the pieces on one
/// line; min-content is the widest unbreakable piece.
fn inline_pieces(children: &[LayoutBox], min: bool, depth: usize) -> f64 {
    let mut total = 0.0;
    let mut widest: f64 = 0.0;
    for c in children {
        if matches!(c.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        let w = if c.is_atomic_inline() || c.form_state.is_some() || c.replaced_size().is_some() {
            outer_intrinsic(c, min, depth)
        } else if c.box_type == BoxType::Inline && has_atomic_inline(&c.children) {
            let frame = resolve_insets(
                &c.style.padding,
                0.0,
                c.style.font_size,
                c.style.root_font_size,
            );
            let margin = resolve_insets(
                &c.style.margin,
                0.0,
                c.style.font_size,
                c.style.root_font_size,
            );
            let edges = frame.left
                + frame.right
                + margin.left
                + margin.right
                + c.style.borders.left.width
                + c.style.borders.right.width;
            inline_pieces(&c.children, min, depth + 1) + if min { 0.0 } else { edges }
        } else {
            shaped_width(std::slice::from_ref(c), min, &c.style)
        };
        total += w;
        widest = widest.max(w);
    }
    if min { widest } else { total }
}
/// Margin-box contribution of a child to its parent's intrinsic width.
fn outer_intrinsic(c: &LayoutBox, min: bool, depth: usize) -> f64 {
    if c.box_type == BoxType::Text {
        return shaped_width(std::slice::from_ref(c), min, &c.style);
    }
    let fs = c.style.font_size;
    let rfs = c.style.root_font_size;
    let padding = resolve_insets(&c.style.padding, 0.0, fs, rfs);
    let margin = resolve_insets(&c.style.margin, 0.0, fs, rfs);
    let frame =
        padding.left + padding.right + c.style.borders.left.width + c.style.borders.right.width;
    let content = match definite(c.style.width, c) {
        Some(w) if c.style.box_sizing == BoxSizing::BorderBox => (w - frame).max(0.0),
        Some(w) => w,
        None => {
            let mut w = intrinsic_content(c, min, depth + 1);
            let border_box = c.style.box_sizing == BoxSizing::BorderBox;
            if let Some(max) = definite(c.style.max_width, c) {
                w = w.min(if border_box {
                    (max - frame).max(0.0)
                } else {
                    max
                });
            }
            if let Some(lo) = definite(c.style.min_width, c) {
                w = w.max(if border_box {
                    (lo - frame).max(0.0)
                } else {
                    lo
                });
            }
            w
        }
    };
    content + frame + margin.left.max(0.0) + margin.right.max(0.0)
}
fn border_main_limit(
    b: &LayoutBox,
    main: f64,
    is_row: bool,
    min: bool,
    ctx: &LayoutContext,
) -> f64 {
    let length = match (is_row, min) {
        (true, true) => b.style.min_width,
        (true, false) => b.style.max_width,
        (false, true) => b.style.min_height,
        (false, false) => b.style.max_height,
    };
    if length == Length::Auto {
        return if min {
            // Flexbox 4.5: scrollable computed overflow has a zero auto
            // minimum. A definite preferred size also caps the content minimum.
            // Otherwise a sidebar's unbreakable text can displace the main page.
            if matches!(
                b.style.overflow_x,
                Overflow::Hidden | Overflow::Auto | Overflow::Scroll
            ) || matches!(
                b.style.overflow_y,
                Overflow::Hidden | Overflow::Auto | Overflow::Scroll
            ) {
                0.0
            } else if is_row {
                let content = intrinsic_width(b, true)
                    + b.padding.left
                    + b.padding.right
                    + b.border.left
                    + b.border.right;
                if b.style.width != Length::Auto {
                    let extra = if b.style.box_sizing == BoxSizing::BorderBox {
                        0.0
                    } else {
                        b.padding.left + b.padding.right + b.border.left + b.border.right
                    };
                    content.min(
                        b.style
                            .width
                            .resolve(main, b.style.font_size, ctx.font_size_root)
                            + extra,
                    )
                } else {
                    content
                }
            } else {
                b.border_box.height
            }
        } else {
            f64::INFINITY
        };
    }
    let value = length.resolve(main, b.style.font_size, ctx.font_size_root);
    let extra = if b.style.box_sizing == BoxSizing::BorderBox {
        0.0
    } else if is_row {
        b.padding.left + b.padding.right + b.border.left + b.border.right
    } else {
        b.padding.top + b.padding.bottom + b.border.top + b.border.bottom
    };
    (value + extra).max(0.0)
}
fn forced_size(
    b: &mut LayoutBox,
    cb: Rect,
    positioned: Rect,
    ctx: &LayoutContext,
    width: Option<f64>,
    height: Option<f64>,
) {
    let old = (b.style.width, b.style.height);
    if let Some(w) = width {
        let padding = resolve_insets(
            &b.style.padding,
            cb.width,
            b.style.font_size,
            ctx.font_size_root,
        );
        let extra = if b.style.box_sizing == BoxSizing::BorderBox {
            0.0
        } else {
            padding.left + padding.right + b.style.borders.left.width + b.style.borders.right.width
        };
        b.style.width = Length::Px((w - extra).max(0.0));
    }
    if let Some(h) = height {
        let padding = resolve_insets(
            &b.style.padding,
            cb.width,
            b.style.font_size,
            ctx.font_size_root,
        );
        let extra = if b.style.box_sizing == BoxSizing::BorderBox {
            0.0
        } else {
            padding.top + padding.bottom + b.style.borders.top.width + b.style.borders.bottom.width
        };
        b.style.height = Length::Px((h - extra).max(0.0));
    }
    layout_box(b, cb, positioned, ctx);
    b.style.width = old.0;
    b.style.height = old.1;
}
fn layout_flex(b: &mut LayoutBox, cb: Rect, positioned: Rect, ctx: &LayoutContext) {
    let inner = init_container(b, cb, positioned, ctx);
    let is_row = matches!(
        b.style.flex_direction,
        FlexDirection::Row | FlexDirection::RowReverse
    );
    let reverse = matches!(
        b.style.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let definite_height = b.style.height != Length::Auto;
    let main_available = if is_row {
        inner.width
    } else if definite_height {
        inner.height
    } else {
        f64::INFINITY
    };
    let main_gap = if is_row {
        b.style.column_gap
    } else {
        b.style.row_gap
    };
    let cross_gap = if is_row {
        b.style.row_gap
    } else {
        b.style.column_gap
    };
    let positioned = if b.style.position != Position::Static {
        b.padding_box()
    } else {
        positioned
    };
    let mut bases = vec![0.0; b.children.len()];
    let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
    let mut used = 0.0;
    for (i, c) in b.children.iter_mut().enumerate() {
        if matches!(c.style.position, Position::Absolute | Position::Fixed) {
            layout_box(c, inner, positioned, ctx);
            continue;
        }
        let natural = if is_row && c.style.width == Length::Auto {
            let padding = resolve_insets(
                &c.style.padding,
                inner.width,
                c.style.font_size,
                ctx.font_size_root,
            );
            Some(
                intrinsic_width(c, false)
                    + padding.left
                    + padding.right
                    + c.style.borders.left.width
                    + c.style.borders.right.width,
            )
        } else {
            None
        };
        forced_size(c, inner, positioned, ctx, natural, None);
        // Percentage bases resolve to content when the container main size
        // is indefinite (including 0% from flex:1). Resolving against zero
        // collapses nested auto-height columns and overlaps following content.
        let indefinite_basis = !main_available.is_finite()
            && match c.style.flex_basis {
                Length::Percent(_) => true,
                Length::Calc(v) => v[1] != 0.0,
                Length::Clamp(v) => v.iter().any(|t| t[1] != 0.0),
                _ => false,
            };
        let base = if c.style.flex_basis != Length::Auto && !indefinite_basis {
            c.style.flex_basis.resolve(
                if is_row { inner.width } else { inner.height },
                c.style.font_size,
                ctx.font_size_root,
            ) + if c.style.box_sizing == BoxSizing::ContentBox {
                if is_row {
                    c.padding.left + c.padding.right + c.border.left + c.border.right
                } else {
                    c.padding.top + c.padding.bottom + c.border.top + c.border.bottom
                }
            } else {
                0.0
            }
        } else if is_row {
            c.border_box.width
        } else {
            c.border_box.height
        };
        bases[i] = base;
        let outer = base
            + if is_row {
                c.margin.left + c.margin.right
            } else {
                c.margin.top + c.margin.bottom
            };
        if b.style.flex_wrap
            && !lines.last().unwrap().is_empty()
            && used + main_gap + outer > main_available
        {
            lines.push(Vec::new());
            used = 0.0;
        }
        if !lines.last().unwrap().is_empty() {
            used += main_gap;
        }
        lines.last_mut().unwrap().push(i);
        used += outer;
    }
    let mut cross_cursor = if is_row { inner.y } else { inner.x };
    let mut total_main: f64 = 0.0;
    let mut sizes = bases.clone();
    let mut frozen = vec![false; b.children.len()];
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let margin_sum: f64 = line
            .iter()
            .map(|i| {
                let c = &b.children[*i];
                if is_row {
                    c.margin.left + c.margin.right
                } else {
                    c.margin.top + c.margin.bottom
                }
            })
            .sum();
        let available = if main_available.is_finite() {
            main_available
        } else {
            line.iter().map(|i| bases[*i]).sum::<f64>()
                + margin_sum
                + main_gap * (line.len() - 1) as f64
        };
        let target_space = (available - margin_sum - main_gap * (line.len() - 1) as f64).max(0.0);
        let growing = target_space > line.iter().map(|i| bases[*i]).sum::<f64>();
        for i in &line {
            sizes[*i] = bases[*i];
            frozen[*i] = false;
        }
        // Freeze at min/max constraints, then redistribute only among flexible
        // items. Each pass freezes at least one item or finishes.
        for _ in 0..=line.len() {
            let free = target_space
                - line
                    .iter()
                    .map(|i| if frozen[*i] { sizes[*i] } else { bases[*i] })
                    .sum::<f64>();
            let factors: f64 = line
                .iter()
                .filter(|i| !frozen[**i])
                .map(|i| {
                    let c = &b.children[*i];
                    if growing {
                        c.style.flex_grow
                    } else {
                        c.style.flex_shrink * bases[*i]
                    }
                })
                .sum();
            let mut violation = false;
            for i in &line {
                if frozen[*i] {
                    continue;
                }
                let c = &b.children[*i];
                let factor = if growing {
                    c.style.flex_grow
                } else {
                    c.style.flex_shrink * bases[*i]
                };
                let proposed = bases[*i]
                    + if factors > 0.0 {
                        free * factor / factors
                    } else {
                        0.0
                    };
                let min = border_main_limit(c, available, is_row, true, ctx);
                let max = border_main_limit(c, available, is_row, false, ctx).max(min);
                sizes[*i] = proposed.clamp(min, max);
                if (sizes[*i] - proposed).abs() > 0.001 {
                    frozen[*i] = true;
                    violation = true;
                }
            }
            if !violation {
                break;
            }
        }
        let occupied = line.iter().map(|i| sizes[*i]).sum::<f64>()
            + margin_sum
            + main_gap * (line.len() - 1) as f64;
        let free = (available - occupied).max(0.0);
        let n = line.len() as f64;
        let (offset, extra) = match b.style.justify_content {
            JustifyContent::FlexEnd => (free, 0.0),
            JustifyContent::Center => (free / 2.0, 0.0),
            JustifyContent::SpaceBetween if n > 1.0 => (0.0, free / (n - 1.0)),
            JustifyContent::SpaceAround => (free / (2.0 * n), free / n),
            JustifyContent::SpaceEvenly => (free / (n + 1.0), free / (n + 1.0)),
            _ => (0.0, 0.0),
        };
        let mut cross: f64 = if !b.style.flex_wrap {
            if is_row && definite_height {
                inner.height
            } else if !is_row {
                inner.width
            } else {
                0.0
            }
        } else {
            0.0
        };
        for i in &line {
            let c = &mut b.children[*i];
            forced_size(
                c,
                inner,
                positioned,
                ctx,
                if is_row { Some(sizes[*i]) } else { None },
                if is_row { None } else { Some(sizes[*i]) },
            );
            cross = cross.max(if is_row {
                c.border_box.height + c.margin.top + c.margin.bottom
            } else {
                c.border_box.width + c.margin.left + c.margin.right
            });
        }
        let origin = if is_row { inner.x } else { inner.y };
        let mut cursor = if reverse {
            origin + available - offset
        } else {
            origin + offset
        };
        for i in line {
            let c = &mut b.children[i];
            let auto_cross = if is_row {
                c.style.height == Length::Auto
            } else {
                c.style.width == Length::Auto
            };
            if b.style.align_items == AlignItems::Stretch && auto_cross {
                let size = if is_row {
                    (cross - c.margin.top - c.margin.bottom).max(0.0)
                } else {
                    (cross - c.margin.left - c.margin.right).max(0.0)
                };
                forced_size(
                    c,
                    inner,
                    positioned,
                    ctx,
                    if is_row { Some(sizes[i]) } else { Some(size) },
                    if is_row { Some(size) } else { Some(sizes[i]) },
                );
            }
            let (before, after, cross_before, cross_after) = if is_row {
                (c.margin.left, c.margin.right, c.margin.top, c.margin.bottom)
            } else {
                (c.margin.top, c.margin.bottom, c.margin.left, c.margin.right)
            };
            let cross_size = if is_row {
                c.border_box.height
            } else {
                c.border_box.width
            };
            let extra_cross = (cross - cross_size - cross_before - cross_after).max(0.0);
            let cross_pos = cross_cursor
                + cross_before
                + match b.style.align_items {
                    AlignItems::Center => extra_cross / 2.0,
                    AlignItems::FlexEnd => extra_cross,
                    _ => 0.0,
                };
            let main = if reverse {
                cursor -= after + sizes[i];
                cursor
            } else {
                cursor + before
            };
            let (x, y) = if is_row {
                (main, cross_pos)
            } else {
                (cross_pos, main)
            };
            translate_tree(c, x - c.border_box.x, y - c.border_box.y);
            if reverse {
                cursor -= before + main_gap + extra;
            } else {
                cursor += before + sizes[i] + after + main_gap + extra;
            }
        }
        cross_cursor += cross + cross_gap;
        total_main = total_main.max(occupied);
    }
    if !definite_height {
        let height = if is_row {
            (cross_cursor - inner.y - cross_gap).max(0.0)
        } else {
            total_main
        };
        b.border_box.height =
            height + b.border.top + b.border.bottom + b.padding.top + b.padding.bottom;
    }
    update_overflow(b);
}

// ---------------------------------------------------------------------------
// GRID & TABLE LAYOUT BASICS
// ---------------------------------------------------------------------------

fn track_sizes(
    tracks: &[GridTrack],
    mins: &[f64],
    maxs: &[f64],
    available: Option<f64>,
    gap: f64,
    ctx: &LayoutContext,
    fs: f64,
) -> Vec<f64> {
    let count = mins.len();
    let mut sizes = vec![0.0; count];
    let mut flex = vec![0.0; count];
    let mut autos = Vec::new();
    for i in 0..count {
        let track = tracks
            .get(i)
            .cloned()
            .unwrap_or(GridTrack::Length(Length::Auto));
        let (min, max) = match track {
            GridTrack::MinMax(min, max) => (min, *max),
            other => (Length::Auto, other),
        };
        let minimum = if min == Length::Auto {
            mins[i]
        } else {
            min.resolve(available.unwrap_or(0.0), fs, ctx.font_size_root)
        };
        match max {
            GridTrack::Fraction(fr) => {
                sizes[i] = minimum;
                flex[i] = fr;
            }
            GridTrack::Length(Length::Auto) => {
                sizes[i] = maxs[i].max(minimum);
                autos.push(i);
            }
            GridTrack::Length(length) => {
                sizes[i] = length
                    .resolve(available.unwrap_or(0.0), fs, ctx.font_size_root)
                    .max(if min == Length::Auto { 0.0 } else { minimum })
            }
            GridTrack::MinMax(_, _) => sizes[i] = minimum, // nested minmax is rejected by syntax
            GridTrack::AutoRepeat(_) => sizes[i] = minimum, // expanded before sizing
        }
    }
    if let Some(available) = available {
        let room = (available - gap * count.saturating_sub(1) as f64).max(0.0);
        let minimums = sizes.clone();
        let mut frozen = vec![false; count];
        for _ in 0..=count {
            let fixed: f64 = (0..count)
                .filter(|i| flex[*i] == 0.0 || frozen[*i])
                .map(|i| sizes[i])
                .sum();
            let factor: f64 = (0..count).filter(|i| !frozen[*i]).map(|i| flex[i]).sum();
            if factor == 0.0 {
                break;
            }
            let unit = ((room - fixed) / factor).max(0.0);
            let mut violation = false;
            for i in 0..count {
                if flex[i] > 0.0 && !frozen[i] {
                    if unit * flex[i] < minimums[i] {
                        sizes[i] = minimums[i];
                        frozen[i] = true;
                        violation = true;
                    } else {
                        sizes[i] = unit * flex[i];
                    }
                }
            }
            if !violation {
                break;
            }
        }
        let total: f64 = sizes.iter().sum();
        if total > room && !autos.is_empty() {
            // Auto tracks start at max-content but give back space down to
            // their min-content contribution when the grid would overflow.
            let slack: f64 = autos.iter().map(|&i| (sizes[i] - mins[i]).max(0.0)).sum();
            if slack > 0.0 {
                let shrink = ((total - room) / slack).min(1.0);
                for &i in &autos {
                    sizes[i] -= (sizes[i] - mins[i]).max(0.0) * shrink;
                }
            }
        } else if flex.iter().all(|f| *f == 0.0) && !autos.is_empty() {
            let extra = ((room - total) / autos.len() as f64).max(0.0);
            for i in autos {
                sizes[i] += extra;
            }
        }
    }
    sizes
}
fn update_overflow(b: &mut LayoutBox) {
    let content = b.content_box();
    let mut width = content.width;
    let mut height = content.height;
    for c in &b.children {
        if c.style.position == Position::Fixed {
            continue;
        }
        width = width.max(c.border_box.right() - content.x + c.margin.right);
        height = height.max(c.border_box.bottom() - content.y + c.margin.bottom);
        if c.style.overflow_x == Overflow::Visible {
            width = width.max(c.border_box.x - content.x + c.scroll_width);
        }
        if c.style.overflow_y == Overflow::Visible {
            height = height.max(c.border_box.y - content.y + c.scroll_height);
        }
    }
    for line in &b.lines {
        width = width.max(line.rect.x + line.rect.width);
        height = height.max(line.rect.y + line.rect.height);
    }
    b.scroll_width = width + b.padding.left + b.padding.right;
    b.scroll_height = height + b.padding.top + b.padding.bottom;
}
/// Expand `repeat(auto-fill, …)` to as many repetitions as fit `available`.
fn expand_auto_repeat(
    tracks: &[GridTrack],
    available: f64,
    gap: f64,
    fs: f64,
    rfs: f64,
) -> Vec<GridTrack> {
    let size = |t: &GridTrack| match t {
        GridTrack::Length(l) | GridTrack::MinMax(l, _) if *l != Length::Auto => {
            l.resolve(available, fs, rfs).max(0.0)
        }
        GridTrack::MinMax(_, max) => match **max {
            GridTrack::Length(l) if l != Length::Auto => l.resolve(available, fs, rfs).max(0.0),
            _ => 0.0,
        },
        _ => 0.0,
    };
    let fixed: f64 = tracks
        .iter()
        .filter(|t| !matches!(t, GridTrack::AutoRepeat(_)))
        .map(|t| size(t) + gap)
        .sum();
    let mut out = Vec::new();
    for t in tracks {
        if let GridTrack::AutoRepeat(items) = t {
            let unit: f64 = items.iter().map(|t| size(t) + gap).sum();
            let count = if unit > gap * items.len() as f64 {
                (((available - fixed + gap) / unit).floor() as usize).clamp(1, 256)
            } else {
                1
            };
            for _ in 0..count {
                out.extend(items.iter().cloned());
            }
        } else {
            out.push(t.clone());
        }
    }
    out.truncate(256);
    out
}
/// Turn named areas and end lines into the start/span form placement uses.
fn resolve_grid_item(c: &mut LayoutBox, areas: &[Vec<String>], cols: usize, rows: usize) {
    let find = |name: &str| {
        let mut bounds: Option<(usize, usize, usize, usize)> = None;
        for (r, row) in areas.iter().enumerate() {
            for (col, cell) in row.iter().enumerate() {
                if cell == name {
                    let b = bounds.get_or_insert((r, r, col, col));
                    *b = (b.0.min(r), b.1.max(r), b.2.min(col), b.3.max(col));
                }
            }
        }
        bounds
    };
    let style = &mut c.style;
    if let Some(name) = style.grid_row_name.clone()
        && let Some((r0, r1, _, _)) = find(&name)
    {
        style.grid_row_start = Some(r0 as i32 + 1);
        style.grid_row_span = r1 - r0 + 1;
    }
    if let Some(name) = style.grid_column_name.clone()
        && let Some((_, _, c0, c1)) = find(&name)
    {
        style.grid_column_start = Some(c0 as i32 + 1);
        style.grid_column_span = c1 - c0 + 1;
    }
    let line = |n: i32, count: usize| if n < 0 { count as i32 + 2 + n } else { n };
    if let Some(end) = style.grid_column_end {
        let end = line(end, cols);
        match style.grid_column_start {
            Some(start) => {
                let start = line(start, cols);
                let (a, b) = (start.min(end), start.max(end));
                style.grid_column_start = Some(a.max(1));
                style.grid_column_span = ((b - a).max(1) as usize).min(256);
            }
            None => {
                style.grid_column_start = Some((end - style.grid_column_span as i32).max(1));
            }
        }
    } else if let Some(start) = style.grid_column_start {
        style.grid_column_start = Some(line(start, cols).max(1));
    }
    if let Some(end) = style.grid_row_end {
        let end = line(end, rows);
        match style.grid_row_start {
            Some(start) => {
                let start = line(start, rows);
                let (a, b) = (start.min(end), start.max(end));
                style.grid_row_start = Some(a.max(1));
                style.grid_row_span = ((b - a).max(1) as usize).min(256);
            }
            None => {
                style.grid_row_start = Some((end - style.grid_row_span as i32).max(1));
            }
        }
    } else if let Some(start) = style.grid_row_start {
        style.grid_row_start = Some(line(start, rows).max(1));
    }
}
fn layout_grid(b: &mut LayoutBox, cb: Rect, positioned: Rect, ctx: &LayoutContext) {
    let inner = init_container(b, cb, positioned, ctx);
    let positioned = if b.style.position != Position::Static {
        b.padding_box()
    } else {
        positioned
    };
    let columns = expand_auto_repeat(
        &b.style.grid_template_columns,
        inner.width,
        b.style.column_gap,
        b.style.font_size,
        ctx.font_size_root,
    );
    let areas = b.style.grid_template_areas.clone();
    let template_cols = columns.len().max(areas.first().map_or(0, Vec::len));
    let template_rows = b.style.grid_template_rows.len().max(areas.len());
    for c in &mut b.children {
        resolve_grid_item(c, &areas, template_cols, template_rows);
    }
    let cols = template_cols
        .max(
            b.children
                .iter()
                .map(|c| {
                    c.style.grid_column_start.unwrap_or(1).max(1) as usize
                        + c.style.grid_column_span
                        - 1
                })
                .max()
                .unwrap_or(1),
        )
        .clamp(1, 256);
    let mut occupied = std::collections::HashSet::new();
    let mut cursor = 0;
    let mut places = Vec::new();
    let mut rows = template_rows.max(1);
    for (i, c) in b.children.iter_mut().enumerate() {
        if matches!(c.style.position, Position::Absolute | Position::Fixed) {
            layout_box(c, inner, positioned, ctx);
            continue;
        }
        let cs = c.style.grid_column_span.min(cols);
        let rs = c.style.grid_row_span.clamp(1, 256);
        let explicit_col = c
            .style
            .grid_column_start
            .map(|c| (c.max(1) as usize - 1).min(cols - cs));
        let explicit_row = c
            .style
            .grid_row_start
            .map(|r| (r.max(1) as usize - 1).min(4095));
        let mut row = explicit_row.unwrap_or(cursor / cols);
        let mut col = explicit_col.unwrap_or(cursor % cols).min(cols - cs);
        for _ in 0..4096 * cols {
            let free =
                (row..row + rs).all(|r| (col..col + cs).all(|c| !occupied.contains(&(r, c))));
            if free || (explicit_col.is_some() && explicit_row.is_some()) {
                break;
            }
            if explicit_col.is_some() {
                row += 1;
            } else {
                col += 1;
                if col + cs > cols {
                    col = 0;
                    row += 1;
                }
            }
            if row >= 4096 {
                break;
            }
        }
        if row >= 4096 {
            continue;
        }
        for r in row..row + rs {
            for c in col..col + cs {
                occupied.insert((r, c));
            }
        }
        if explicit_row.is_none() {
            cursor = row * cols + col + cs;
        }
        rows = rows.max(row + rs);
        places.push((i, row, col, rs, cs));
    }
    let mut mins = vec![0.0_f64; cols];
    let mut maxs = mins.clone();
    for (i, _, col, _, span) in &places {
        let c = &b.children[*i];
        let min = intrinsic_width(c, true) / *span as f64;
        let max = intrinsic_width(c, false) / *span as f64;
        for j in *col..col + span {
            mins[j] = mins[j].max(min);
            maxs[j] = maxs[j].max(max);
        }
    }
    let widths = track_sizes(
        &columns,
        &mins,
        &maxs,
        Some(inner.width),
        b.style.column_gap,
        ctx,
        b.style.font_size,
    );
    let mut heights = vec![0.0_f64; rows];
    for (i, row, col, rs, cs) in &places {
        let width =
            widths[*col..col + cs].iter().sum::<f64>() + b.style.column_gap * (cs - 1) as f64;
        let c = &mut b.children[*i];
        forced_size(
            c,
            Rect { width, ..inner },
            positioned,
            ctx,
            Some(width),
            None,
        );
        let height = (c.border_box.height + c.margin.top + c.margin.bottom
            - b.style.row_gap * (rs - 1) as f64)
            .max(0.0)
            / *rs as f64;
        for h in heights.iter_mut().skip(*row).take(*rs) {
            *h = h.max(height);
        }
    }
    let row_sizes = track_sizes(
        &b.style.grid_template_rows,
        &heights,
        &heights,
        if b.style.height == Length::Auto {
            None
        } else {
            Some(inner.height)
        },
        b.style.row_gap,
        ctx,
        b.style.font_size,
    );
    for (i, row, col, rs, cs) in places {
        let x = inner.x + widths[..col].iter().sum::<f64>() + b.style.column_gap * col as f64;
        let y = inner.y + row_sizes[..row].iter().sum::<f64>() + b.style.row_gap * row as f64;
        let w = widths[col..col + cs].iter().sum::<f64>() + b.style.column_gap * (cs - 1) as f64;
        let h = row_sizes[row..row + rs].iter().sum::<f64>() + b.style.row_gap * (rs - 1) as f64;
        let c = &mut b.children[i];
        let stretch_w = c.style.width == Length::Auto;
        let stretch_h = c.style.height == Length::Auto;
        forced_size(
            c,
            Rect {
                x,
                y,
                width: w,
                height: h,
            },
            positioned,
            ctx,
            stretch_w.then_some((w - c.margin.left - c.margin.right).max(0.0)),
            stretch_h.then_some((h - c.margin.top - c.margin.bottom).max(0.0)),
        );
    }
    if b.style.height == Length::Auto {
        b.border_box.height = row_sizes.iter().sum::<f64>()
            + b.style.row_gap * rows.saturating_sub(1) as f64
            + b.border.top
            + b.border.bottom
            + b.padding.top
            + b.padding.bottom;
    }
    update_overflow(b);
}

fn row_paths(b: &LayoutBox, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    for (i, c) in b.children.iter().enumerate() {
        path.push(i);
        if c.box_type == BoxType::TableRow {
            out.push(path.clone());
        } else if matches!(c.box_type, BoxType::Block | BoxType::Inline) {
            row_paths(c, path, out);
        }
        path.pop();
    }
}
fn at_path<'a>(b: &'a mut LayoutBox, path: &[usize]) -> &'a mut LayoutBox {
    if let Some((first, rest)) = path.split_first() {
        at_path(&mut b.children[*first], rest)
    } else {
        b
    }
}
fn layout_table(b: &mut LayoutBox, cb: Rect, positioned: Rect, ctx: &LayoutContext) {
    let inner = init_container(b, cb, positioned, ctx);
    let spacing = b.style.border_spacing;
    let mut paths = Vec::new();
    row_paths(b, &mut Vec::new(), &mut paths);
    if paths.is_empty() {
        return;
    }
    let count = paths.len();
    let mut places = Vec::new();
    let mut occupied = std::collections::HashSet::new();
    let mut cols = 0;
    for (r, path) in paths.iter().enumerate() {
        let row = at_path(b, path);
        let mut col = 0;
        for (i, cell) in row.children.iter().enumerate() {
            if cell.box_type != BoxType::TableCell {
                continue;
            }
            let cs = cell.table_col_span;
            let rs = cell.table_row_span.min(count - r);
            while (col..col + cs).any(|c| occupied.contains(&(r, c))) && col < 256 {
                col += 1;
            }
            if col + cs > 256 {
                continue;
            }
            for rr in r..r + rs {
                for cc in col..col + cs {
                    occupied.insert((rr, cc));
                }
            }
            places.push((r, i, col, rs, cs));
            cols = cols.max(col + cs);
            col += cs;
        }
    }
    if cols == 0 {
        return;
    }
    let mut mins = vec![0.0_f64; cols];
    let mut maxs = mins.clone();
    // Columns with an authored width keep it; only auto columns absorb space.
    let mut sized = vec![false; cols];
    for (r, i, col, _, cs) in &places {
        let c = &at_path(b, &paths[*r]).children[*i];
        let padding = resolve_insets(
            &c.style.padding,
            inner.width,
            c.style.font_size,
            ctx.font_size_root,
        );
        let extra =
            padding.left + padding.right + c.style.borders.left.width + c.style.borders.right.width;
        let explicit = if c.style.width == Length::Auto {
            0.0
        } else {
            c.style
                .width
                .resolve(inner.width, c.style.font_size, ctx.font_size_root)
        };
        let min = (intrinsic_width(c, true) + extra).max(explicit) / *cs as f64;
        let max = (intrinsic_width(c, false) + extra).max(explicit) / *cs as f64;
        if c.style.width != Length::Auto && *cs == 1 {
            sized[*col] = true;
        }
        for cc in *col..col + cs {
            mins[cc] = mins[cc].max(min);
            maxs[cc] = maxs[cc].max(max);
        }
    }
    let room = (inner.width - spacing * (cols + 1) as f64).max(0.0);
    let min_sum: f64 = mins.iter().sum();
    let max_sum: f64 = maxs.iter().sum();
    let mut widths = mins.clone();
    if room > min_sum {
        if max_sum > room {
            let factor = (room - min_sum) / (max_sum - min_sum).max(0.001);
            for c in 0..cols {
                widths[c] += factor * (maxs[c] - mins[c]);
            }
        } else {
            // Spare width goes to auto columns in proportion to their
            // max-content widths (all columns if every one is sized).
            let extra = room - max_sum;
            let pool: Vec<usize> = (0..cols).filter(|c| !sized[*c]).collect();
            let pool = if pool.is_empty() {
                (0..cols).collect()
            } else {
                pool
            };
            let weight: f64 = pool.iter().map(|c| maxs[*c]).sum();
            widths.clone_from(&maxs);
            for &c in &pool {
                widths[c] += if weight > 0.0 {
                    extra * maxs[c] / weight
                } else {
                    extra / pool.len() as f64
                };
            }
        }
    }
    let mut heights = vec![0.0_f64; count];
    for (r, i, col, rs, cs) in &places {
        let c = &mut at_path(b, &paths[*r]).children[*i];
        let width = widths[*col..col + cs].iter().sum::<f64>() + spacing * (cs - 1) as f64;
        forced_size(
            c,
            Rect { width, ..inner },
            positioned,
            ctx,
            Some(width),
            None,
        );
        let height = (c.border_box.height - spacing * (rs - 1) as f64).max(0.0) / *rs as f64;
        for h in heights.iter_mut().skip(*r).take(*rs) {
            *h = h.max(height);
        }
    }
    for (r, path) in paths.iter().enumerate() {
        let row = at_path(b, path);
        if row.style.height != Length::Auto {
            heights[r] = heights[r].max(row.style.height.resolve(
                inner.height,
                row.style.font_size,
                ctx.font_size_root,
            ));
        }
        row.border_box = Rect {
            x: inner.x + spacing,
            y: inner.y + spacing + heights[..r].iter().sum::<f64>() + spacing * r as f64,
            width: widths.iter().sum::<f64>() + spacing * cols.saturating_sub(1) as f64,
            height: heights[r],
        };
    }
    for (r, i, col, rs, cs) in places {
        let row_y = at_path(b, &paths[r]).border_box.y;
        let c = &mut at_path(b, &paths[r]).children[i];
        let x = inner.x + spacing + widths[..col].iter().sum::<f64>() + spacing * col as f64;
        let w = widths[col..col + cs].iter().sum::<f64>() + spacing * (cs - 1) as f64;
        let h = heights[r..r + rs].iter().sum::<f64>() + spacing * (rs - 1) as f64;
        forced_size(
            c,
            Rect {
                x,
                y: row_y,
                width: w,
                height: h,
            },
            positioned,
            ctx,
            Some(w),
            Some(h),
        );
    }
    // Section boxes enclose their rows, so their backgrounds and geometry agree.
    for section in &mut b.children {
        if section.box_type == BoxType::TableRow || section.children.is_empty() {
            continue;
        }
        let first = section.children.first().unwrap().border_box;
        let last = section.children.last().unwrap().border_box;
        section.border_box = Rect {
            x: inner.x,
            y: first.y,
            width: inner.width,
            height: (last.bottom() - first.y).max(0.0),
        };
    }
    b.border_box.height = heights.iter().sum::<f64>()
        + spacing * (count + 1) as f64
        + b.padding.top
        + b.padding.bottom
        + b.border.top
        + b.border.bottom;
    update_overflow(b);
}

// ---------------------------------------------------------------------------
// HIT TESTING & GEOMETRY QUERIES
// ---------------------------------------------------------------------------

pub fn hit_test_node(box_node: &LayoutBox, point: Point) -> Option<NodeKey> {
    super::display_list::DisplayList::build(box_node, &[], 1.0).hit_test(point)
}

fn assign_inline_geometry(children: &mut [LayoutBox], lines: &[LineBox], x: f64, y: f64) {
    fn keys(b: &LayoutBox, out: &mut std::collections::HashSet<NodeKey>) {
        if let Some(k) = b.node {
            out.insert(k);
        }
        for c in &b.children {
            keys(c, out);
        }
    }
    for child in children {
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        if child.is_atomic_inline() || child.form_state.is_some() || child.replaced_size().is_some()
        {
            if let Some(ibox) = lines
                .iter()
                .flat_map(|l| &l.inline_boxes)
                .find(|ib| child.node.is_some_and(|k| k.as_u64() == ib.id))
            {
                let target_x = x + ibox.rect.x + child.margin.left;
                let target_y = y + ibox.rect.y + child.margin.top;
                let dx = target_x - child.border_box.x;
                let dy = target_y - child.border_box.y;
                translate_tree(child, dx, dy);
                child.fragments = vec![child.border_box];
            }
        } else {
            let mut descendants = std::collections::HashSet::new();
            keys(child, &mut descendants);
            let mut frags: Vec<Rect> = lines
                .iter()
                .flat_map(|l| &l.text_runs)
                .filter(|r| r.node.is_some_and(|k| descendants.contains(&k)))
                .map(|r| Rect {
                    x: x + r.rect.x,
                    y: y + r.rect.y,
                    ..r.rect
                })
                .collect();
            if child.box_type == BoxType::Inline && !frags.is_empty() {
                let pad_l = child.padding.left + child.border.left;
                let pad_r = child.padding.right + child.border.right;
                let pad_t = child.padding.top + child.border.top;
                let pad_b = child.padding.bottom + child.border.bottom;
                let len = frags.len();
                for (idx, f) in frags.iter_mut().enumerate() {
                    f.y -= pad_t;
                    f.height += pad_t + pad_b;
                    if idx == 0 {
                        f.x -= pad_l;
                        f.width += pad_l;
                    }
                    if idx == len - 1 {
                        f.width += pad_r;
                    }
                }
            }
            child.fragments = frags;
            if let Some(first) = child.fragments.first().copied() {
                child.border_box = child.fragments.iter().fold(first, |a, b| {
                    let left = a.x.min(b.x);
                    let top = a.y.min(b.y);
                    Rect {
                        x: left,
                        y: top,
                        width: a.right().max(b.right()) - left,
                        height: a.bottom().max(b.bottom()) - top,
                    }
                });
            }
            assign_inline_geometry(&mut child.children, lines, x, y);
        }
    }
}

fn anonymous_style(parent: &ComputedStyle) -> ComputedStyle {
    ComputedStyle {
        display: Display::Block,
        color: parent.color,
        visible: parent.visible,
        pointer_events: parent.pointer_events,
        font_size: parent.font_size,
        line_height: parent.line_height,
        line_height_factor: parent.line_height_factor,
        font_family: parent.font_family.clone(),
        font_weight: parent.font_weight,
        font_italic: parent.font_italic,
        white_space: parent.white_space,
        text_align: parent.text_align,
        custom_properties: parent.custom_properties.clone(),
        ..Default::default()
    }
}

fn collapse_margins(a: f64, b: f64) -> f64 {
    a.max(b).max(0.0) + a.min(b).min(0.0)
}
fn collapses_top(b: &LayoutBox, width: f64, ctx: &LayoutContext) -> bool {
    !b.is_root
        && matches!(b.box_type, BoxType::Block | BoxType::AnonymousBlock)
        && b.style.overflow_x == Overflow::Visible
        && b.style.overflow_y == Overflow::Visible
        && b.style.borders.top.width == 0.0
        && b.style
            .padding
            .top
            .resolve(width, b.style.font_size, ctx.font_size_root)
            == 0.0
        && b.children
            .iter()
            .find(|c| !matches!(c.style.position, Position::Absolute | Position::Fixed))
            .is_some_and(|c| matches!(c.box_type, BoxType::Block | BoxType::AnonymousBlock))
}
fn collapsed_top(b: &LayoutBox, width: f64, ctx: &LayoutContext) -> f64 {
    let own = b
        .style
        .margin
        .top
        .resolve(width, b.style.font_size, ctx.font_size_root);
    if collapses_top(b, width, ctx) {
        let child = b
            .children
            .iter()
            .find(|c| !matches!(c.style.position, Position::Absolute | Position::Fixed))
            .unwrap();
        collapse_margins(own, collapsed_top(child, width, ctx))
    } else {
        own
    }
}

fn content_constraint(
    length: Length,
    extra: f64,
    sizing: BoxSizing,
    parent: f64,
    fs: f64,
    rfs: f64,
) -> Length {
    if length == Length::Auto || sizing == BoxSizing::ContentBox {
        length
    } else {
        Length::Px((length.resolve(parent, fs, rfs) - extra).max(0.0))
    }
}
