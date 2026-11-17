//! Temporary boundary for Blitz-owned CSSOM, authorized decoded resources and
//! editing state. It exposes no reference computed styles or layout. The native
//! renderer and V8 continue to share exactly one authoritative DOM.
use super::{
    css::{StyleSheet, parse_media_query_list},
    graphics::SvgGraphic,
    layout::LayoutBox,
};
use wega_adapter_blitz::{BlitzDocument, ToNodeId};
use wega_dom::layout::Rect;
use wega_dom::{DocumentView, NodeKind, NodeView};
type Csp = std::sync::Arc<std::sync::RwLock<wega_security::csp::Csp>>;
pub(crate) trait NativeResources {
    fn native_generations(&self) -> (u64, u64);
    fn native_fonts(&self) -> std::sync::Arc<std::sync::Mutex<parley::FontContext>>;
    fn native_stylesheets(&self, csp: Option<&Csp>) -> Vec<StyleSheet>;
    fn native_images(&mut self, root: &mut LayoutBox);
}
impl NativeResources for BlitzDocument {
    fn native_generations(&self) -> (u64, u64) {
        (self.stylesheet_generation(), self.resource_generation())
    }
    fn native_fonts(&self) -> std::sync::Arc<std::sync::Mutex<parley::FontContext>> {
        self.font_ctx()
    }
    fn native_images(&mut self, root: &mut LayoutBox) {
        images(root, self);
    }
    fn native_stylesheets(&self, csp: Option<&Csp>) -> Vec<StyleSheet> {
        let d = self;
        let mut sheets = Vec::new();

        let root = d.root();
        let mut stack = vec![root];
        while let Some(k) = stack.pop() {
            if let Some(node) = d.node(k) {
                if node.kind() == NodeKind::Element && !d.stylesheet_disabled(k.node_id()) {
                    if let Some(elem) = node.element_name() {
                        let media_queries = node
                            .attr("media")
                            .filter(|m| !m.trim().is_empty())
                            .map(parse_media_query_list);

                        if elem.local.eq_ignore_ascii_case("style") {
                            let mut css_text = String::new();
                            for child_k in node.children() {
                                if let Some(text) = d.node(child_k).and_then(NodeView::text) {
                                    css_text.push_str(text);
                                }
                            }
                            let allowed = csp.is_none_or(|c| {
                                c.read()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .allows_inline("style-src-elem", node.attr("nonce"), &css_text)
                            });
                            if allowed {
                                let mut sheet = StyleSheet::parse(
                                    &d.native_stylesheet_text(k.node_id()).unwrap_or(css_text),
                                );
                                if let Some(ref mq) = media_queries {
                                    for r in &mut sheet.rules {
                                        r.media.push(mq.clone());
                                    }
                                }
                                sheets.push(sheet);
                            }
                        } else if elem.local.eq_ignore_ascii_case("link")
                            && node.attr("rel").is_some_and(|r| {
                                r.split_ascii_whitespace()
                                    .any(|r| r.eq_ignore_ascii_case("stylesheet"))
                            })
                        {
                            if let Some((source, requested_url)) = d.stylesheet_source(k.node_id())
                            {
                                if node
                                    .attr("href")
                                    .and_then(|href| d.url().join(href).ok())
                                    .is_some_and(|u| u.as_str() == requested_url)
                                {
                                    let mut sheet = StyleSheet::parse(
                                        &d.native_stylesheet_text(k.node_id())
                                            .unwrap_or_else(|| source.to_owned()),
                                    );
                                    if let Some(ref mq) = media_queries {
                                        for r in &mut sheet.rules {
                                            r.media.push(mq.clone());
                                        }
                                    }
                                    sheets.push(sheet);
                                }
                            }
                        }
                    }
                }
                for c in node.children().rev() {
                    stack.push(c);
                }
            }
        }
        sheets
    }
}
pub(crate) fn decode_svg(source: &[u8]) -> Option<SvgGraphic> {
    let svg = blitz_dom::node::SvgImageData::from_data(source, &usvg::Options::default()).ok()?;
    Some(svg_graphic(&svg))
}
pub fn raster(image: &blitz_dom::node::RasterImageData) -> peniko::ImageData {
    peniko::ImageData {
        data: image.data.clone(),
        format: peniko::ImageFormat::Rgba8,
        alpha_type: peniko::ImageAlphaType::Alpha,
        width: image.width,
        height: image.height,
    }
}

fn svg_graphic(svg: &blitz_dom::node::SvgImageData) -> SvgGraphic {
    SvgGraphic::from_decoded(
        svg.tree.clone(),
        svg.intrinsic_dimensions.view_box,
        svg.intrinsic_dimensions.preserve_aspect_ratio,
        svg.intrinsic_dimensions.degenerate_view_box,
    )
}
fn images(b: &mut LayoutBox, doc: &mut BlitzDocument) {
    if let Some(source) = b.node.and_then(|key| {
        doc.node(key)
            .and_then(NodeView::element_name)
            .filter(|n| n.local == "svg")
            .and_then(|_| super::graphics::svg_source(doc, key))
    }) {
        b.svg = decode_svg(source.as_bytes());
    }
    if let Some(image) = b
        .node
        .and_then(|k| doc.get_node(k.node_id()))
        .and_then(|n| n.element_data())
        .and_then(|e| e.image_data())
    {
        match image {
            blitz_dom::node::ImageData::Raster(image) => b.image = Some(raster(image)),
            blitz_dom::node::ImageData::Svg(svg) => b.svg = Some(svg_graphic(svg)),
            _ => {}
        }
    }
    if let Some(url) = b
        .style
        .background_image
        .as_ref()
        .and_then(|u| doc.url().join(u).ok())
    {
        match doc.native_image(url) {
            Some(blitz_dom::node::ImageData::Raster(image)) => {
                b.background_image = Some(raster(&image))
            }
            Some(blitz_dom::node::ImageData::Svg(svg)) => {
                b.background_svg = Some(svg_graphic(&svg))
            }
            _ => {}
        }
    }
    for c in &mut b.children {
        images(c, doc);
    }
}
pub(crate) fn ensure_form_control_data(doc: &mut BlitzDocument) {
    use blitz_dom::local_name;
    use blitz_dom::node::{SpecialElementData, TextInputData};
    let root = doc.root_node().id;
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if let Some(node) = doc.get_node_mut(id) {
            if let Some(el) = node.element_data_mut() {
                let tag: &str = el.name.local.as_ref();
                if tag.eq_ignore_ascii_case("input") {
                    let kind = el
                        .attr(local_name!("type"))
                        .unwrap_or("text")
                        .to_ascii_lowercase();
                    if matches!(kind.as_str(), "checkbox" | "radio") {
                        if el.checkbox_input_checked().is_none() {
                            let checked = el.attr(local_name!("checked")).is_some();
                            el.set_checkbox_input_checked(checked);
                        }
                    } else if !matches!(
                        kind.as_str(),
                        "button" | "submit" | "reset" | "image" | "file"
                    ) {
                        if el.text_input_data().is_none() {
                            let val = el
                                .attr(local_name!("value"))
                                .unwrap_or_default()
                                .to_string();
                            let mut data = TextInputData::new(false);
                            data.editor.set_text(&val);
                            el.special_data = SpecialElementData::TextInput(data);
                        }
                    }
                } else if tag.eq_ignore_ascii_case("textarea") {
                    if el.text_input_data().is_none() {
                        let val = el
                            .attr(local_name!("value"))
                            .map(str::to_string)
                            .unwrap_or_default();
                        let mut data = TextInputData::new(true);
                        data.editor.set_text(&val);
                        el.special_data = SpecialElementData::TextInput(data);
                    }
                }
            }
            stack.extend(node.children.iter().copied());
        }
    }
}

pub(crate) fn sync_form_state(b: &mut LayoutBox, doc: &BlitzDocument) {
    use super::layout::FormControlKind;
    if let Some(node_key) = b.node {
        if let Some(node) = doc.get_node(node_key.node_id()) {
            if let Some(el) = node.element_data() {
                let is_focused = doc.get_focussed_node_id() == Some(node_key.node_id());
                if let Some(ref mut form) = b.form_state {
                    form.is_focused = is_focused;
                    if let Some(checked) = el.checkbox_input_checked() {
                        form.checked = checked;
                    }
                    if let Some(input_data) = el.text_input_data() {
                        let editor_text = input_data.editor.text().to_string();
                        let is_pwd = matches!(
                            form.kind,
                            Some(FormControlKind::TextInput {
                                is_password: true,
                                ..
                            })
                        );
                        let ph = node.attr(blitz_dom::local_name!("placeholder"));
                        let (disp, is_ph) = if !editor_text.is_empty() {
                            if is_pwd {
                                ("•".repeat(editor_text.chars().count()), false)
                            } else {
                                (editor_text.clone(), false)
                            }
                        } else if let Some(placeholder) = ph {
                            (placeholder.to_string(), true)
                        } else {
                            (String::new(), false)
                        };
                        b.children.clear();
                        if !disp.is_empty() {
                            let mut child_style = b.style.clone();
                            if is_ph {
                                child_style.color = super::css::Color {
                                    r: 117,
                                    g: 117,
                                    b: 117,
                                    a: 255,
                                };
                            }
                            let mut text_child = LayoutBox::new(
                                Some(node_key),
                                super::layout::BoxType::Text,
                                child_style,
                            );
                            text_child.text_content = Some(disp);
                            b.children.push(text_child);
                        }
                    }
                }
            }
        }
    }
    for child in &mut b.children {
        sync_form_state(child, doc);
    }
}

pub(crate) fn compute_form_decorations(b: &mut LayoutBox, doc: &BlitzDocument) {
    use super::layout::FormControlKind;
    if let Some(node_key) = b.node {
        if let Some(node) = doc.get_node(node_key.node_id()) {
            if let Some(el) = node.element_data() {
                if let Some(input_data) = el.text_input_data() {
                    let (is_pwd, is_focused) = if let Some(ref form) = b.form_state {
                        (
                            matches!(
                                form.kind,
                                Some(FormControlKind::TextInput {
                                    is_password: true,
                                    ..
                                })
                            ),
                            form.is_focused,
                        )
                    } else {
                        (false, false)
                    };
                    if b.form_state.is_some() {
                        let sel = input_data.editor.raw_selection();
                        let (a, f) = (sel.anchor().index(), sel.focus().index());
                        let (sel_min, sel_max) = (a.min(f), a.max(f));
                        let cb = b.content_box();
                        let mut caret_rect = None;
                        let mut selection_rects = Vec::new();

                        if b.lines.is_empty() {
                            if is_focused {
                                let h = (16.0_f64).min(cb.height);
                                caret_rect = Some(Rect {
                                    x: cb.x + 2.0,
                                    y: cb.y + ((cb.height - h) / 2.0).max(0.0),
                                    width: 1.5,
                                    height: h,
                                });
                            }
                        } else {
                            let line = &b.lines[0];
                            let get_x = |idx: usize| -> f64 {
                                if line.text_runs.is_empty() {
                                    return cb.x;
                                }
                                let run = &line.text_runs[0];
                                if idx >= run.glyphs.len() {
                                    cb.x + run.rect.x + run.rect.width
                                } else {
                                    cb.x + run.rect.x + run.glyphs[idx].x as f64
                                }
                            };
                            let full_text = input_data.editor.text().to_string();
                            let sel_min_char = if is_pwd {
                                full_text
                                    .get(..sel_min)
                                    .map(|s| s.chars().count())
                                    .unwrap_or(0)
                            } else {
                                sel_min
                            };
                            let sel_max_char = if is_pwd {
                                full_text
                                    .get(..sel_max)
                                    .map(|s| s.chars().count())
                                    .unwrap_or(0)
                            } else {
                                sel_max
                            };
                            let focus_char = if is_pwd {
                                full_text.get(..f).map(|s| s.chars().count()).unwrap_or(0)
                            } else {
                                f
                            };
                            if is_focused {
                                let cx = get_x(focus_char);
                                caret_rect = Some(Rect {
                                    x: cx,
                                    y: cb.y + line.rect.y,
                                    width: 1.5,
                                    height: line.rect.height,
                                });
                            }
                            if sel_min_char < sel_max_char {
                                let x0 = get_x(sel_min_char);
                                let x1 = get_x(sel_max_char);
                                selection_rects.push(Rect {
                                    x: x0,
                                    y: cb.y + line.rect.y,
                                    width: (x1 - x0).max(1.0),
                                    height: line.rect.height,
                                });
                            }
                        }

                        if let Some(ref mut form) = b.form_state {
                            form.caret_rect = caret_rect;
                            form.selection_rects = selection_rects;
                        }
                    }
                }
            }
        }
    }
    for child in &mut b.children {
        compute_form_decorations(child, doc);
    }
}
