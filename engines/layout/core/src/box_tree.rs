//! Build the box tree from the DOM and computed styles.
use crate::*;
use wega_dom::{DocumentView, NodeKey, NodeKind, NodeView};

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

pub fn build_box_node<D: DocumentView>(
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
