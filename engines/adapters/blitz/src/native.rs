//! Reference-owner mapping onto Wega native resource/control contracts.
use crate::{BlitzDocument, ToNodeId};
use wega_dom::{DocumentView, NodeKind, NodeView};
use wega_native::{
    css::{StyleSheet, parse_media_query_list},
    graphics::{SvgGraphic, decode_svg},
    layout::LayoutBox,
    resources::NativeResources,
};
impl NativeResources for BlitzDocument {
    fn prepare_controls(&mut self) {
        ensure_form_control_data(self);
    }
    fn sync_controls(&self, root: &mut LayoutBox) {
        wega_native::controls::sync(root, &|key| control(self, key));
    }
    fn decorate_controls(&self, root: &mut LayoutBox) {
        wega_native::controls::decorate(root, &|key| control(self, key));
    }

    fn native_generations(&self) -> (u64, u64) {
        (self.stylesheet_generation(), self.resource_generation())
    }
    fn native_fonts(
        &self,
    ) -> std::sync::Arc<std::sync::Mutex<wega_native::resources::FontContext>> {
        self.font_ctx()
    }
    fn native_images(&mut self, root: &mut LayoutBox) {
        images(root, self);
    }
    fn native_stylesheets(
        &self,
        allow_inline: &dyn Fn(Option<&str>, &str) -> bool,
    ) -> Vec<StyleSheet> {
        let d = self;
        let mut sheets = Vec::new();

        let root = d.root();
        let mut stack = vec![root];
        while let Some(k) = stack.pop() {
            if let Some(node) = d.node(k) {
                if node.kind() == NodeKind::Element
                    && !d.stylesheet_disabled(k.node_id())
                    && let Some(elem) = node.element_name()
                {
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
                        let allowed = allow_inline(node.attr("nonce"), &css_text);
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
                        && let Some((source, requested_url)) = d.stylesheet_source(k.node_id())
                        && node
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
                for c in node.children().rev() {
                    stack.push(c);
                }
            }
        }
        sheets
    }
}
fn raster(image: &wega_native::images::RasterImageData) -> peniko::ImageData {
    peniko::ImageData {
        data: image.data.clone(),
        format: peniko::ImageFormat::Rgba8,
        alpha_type: peniko::ImageAlphaType::Alpha,
        width: image.width,
        height: image.height,
    }
}

fn svg_graphic(svg: &wega_native::images::SvgImageData) -> SvgGraphic {
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
            .and_then(|_| wega_native::graphics::svg_source(doc, key))
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
fn ensure_form_control_data(doc: &mut BlitzDocument) {
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
                    ) && el.text_input_data().is_none()
                    {
                        let val = el
                            .attr(local_name!("value"))
                            .unwrap_or_default()
                            .to_string();
                        let mut data = TextInputData::new(false);
                        data.editor.set_text(&val);
                        el.special_data = SpecialElementData::TextInput(data);
                    }
                } else if tag.eq_ignore_ascii_case("textarea") && el.text_input_data().is_none() {
                    let val = el
                        .attr(local_name!("value"))
                        .map(str::to_string)
                        .unwrap_or_default();
                    let mut data = TextInputData::new(true);
                    data.editor.set_text(&val);
                    el.special_data = SpecialElementData::TextInput(data);
                }
            }
            stack.extend(node.children.iter().copied());
        }
    }
}

fn control(
    doc: &BlitzDocument,
    key: wega_dom::NodeKey,
) -> Option<wega_native::controls::ControlView<'_>> {
    let element = doc.get_node(key.node_id())?.element_data()?;
    let input = element.text_input_data();
    let selection = input
        .map(|input| {
            let selection = input.editor.raw_selection();
            (selection.anchor().index(), selection.focus().index())
        })
        .unwrap_or_default();
    Some(wega_native::controls::ControlView {
        value: input.map(|input| input.editor.text().to_string().into()),
        selection,
        checked: element.checkbox_input_checked(),
        focused: doc.get_focussed_node_id() == Some(key.node_id()),
        placeholder: element.attr(blitz_dom::local_name!("placeholder")),
    })
}
