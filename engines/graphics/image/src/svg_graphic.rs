//! Inline SVG serialization and vector decoding.
use std::sync::Arc;
use wega_dom::{DocumentView, NodeKey, NodeKind, NodeView};

pub fn svg_source<D: DocumentView>(doc: &D, key: NodeKey) -> Option<String> {
    fn escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('"', "&quot;")
    }
    fn write<D: DocumentView>(
        doc: &D,
        key: NodeKey,
        out: &mut String,
        depth: usize,
        nodes: &mut usize,
    ) -> Option<()> {
        if depth > 64 || *nodes >= 16384 || out.len() > 1024 * 1024 {
            return None;
        }
        *nodes += 1;
        let node = doc.node(key)?;
        if node.kind() == NodeKind::Text {
            out.push_str(&escape(node.text()?));
            return Some(());
        }
        let name = node.element_name()?;
        // usvg does not execute script; excluding it also avoids parsing its text.
        if matches!(name.local, "script" | "foreignObject") {
            return Some(());
        }
        out.push('<');
        out.push_str(name.local);
        if depth == 0 {
            out.push_str(" xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"");
        }
        for (name, value) in node.attributes() {
            if &*name.local == "xmlns" || name.prefix.as_deref() == Some("xmlns") {
                continue;
            }
            out.push(' ');
            if let Some(prefix) = &name.prefix {
                out.push_str(prefix);
                out.push(':');
            }
            out.push_str(&name.local);
            out.push_str("=\"");
            out.push_str(&escape(value));
            out.push('"');
        }
        out.push('>');
        for child in node.children() {
            if doc
                .node(child)
                .is_some_and(|n| matches!(n.kind(), NodeKind::Element | NodeKind::Text))
            {
                write(doc, child, out, depth + 1, nodes)?;
            }
        }
        out.push_str("</");
        out.push_str(name.local);
        out.push('>');
        Some(())
    }
    let mut xml = String::new();
    write(doc, key, &mut xml, 0, &mut 0)?;
    Some(xml)
}

#[derive(Debug, Clone)]
pub struct SvgGraphic {
    pub tree: Arc<usvg::Tree>,
    pub(crate) view_box: Option<usvg::NonZeroRect>,
    pub(crate) aspect: svgtypes::AspectRatio,
    pub degenerate: bool,
}

impl SvgGraphic {
    /// Attach a tree already decoded by the embedding browser's bounded decoder.
    /// The decoder accepts only authorized bytes and never opens files or sockets.
    pub fn from_decoded(
        tree: Arc<usvg::Tree>,
        view_box: Option<usvg::NonZeroRect>,
        aspect: svgtypes::AspectRatio,
        degenerate: bool,
    ) -> Self {
        Self {
            tree,
            view_box,
            aspect,
            degenerate,
        }
    }
    pub fn transform(&self, w: f64, h: f64) -> kurbo::Affine {
        fn map(v: usvg::NonZeroRect, a: svgtypes::AspectRatio, w: f64, h: f64) -> kurbo::Affine {
            use svgtypes::Align;
            let sx = w / v.width() as f64;
            let sy = h / v.height() as f64;
            let (sx, sy) = if a.align == Align::None {
                (sx, sy)
            } else {
                let k = if a.slice { sx.max(sy) } else { sx.min(sy) };
                (k, k)
            };
            let (x, y) = match a.align {
                Align::None | Align::XMinYMin => (0.0, 0.0),
                Align::XMidYMin => (0.5, 0.0),
                Align::XMaxYMin => (1.0, 0.0),
                Align::XMinYMid => (0.0, 0.5),
                Align::XMidYMid => (0.5, 0.5),
                Align::XMaxYMid => (1.0, 0.5),
                Align::XMinYMax => (0.0, 1.0),
                Align::XMidYMax => (0.5, 1.0),
                Align::XMaxYMax => (1.0, 1.0),
            };
            kurbo::Affine::new([
                sx,
                0.0,
                0.0,
                sy,
                (w - v.width() as f64 * sx) * x - v.x() as f64 * sx,
                (h - v.height() as f64 * sy) * y - v.y() as f64 * sy,
            ])
        }
        let size = self.tree.size();
        if let Some(v) = self.view_box {
            map(v, self.aspect, w, h)
                * map(v, self.aspect, size.width() as f64, size.height() as f64).inverse()
        } else {
            kurbo::Affine::scale_non_uniform(w / size.width() as f64, h / size.height() as f64)
        }
    }
}

/// Decode authorized inline SVG bytes with the shared bounded image decoder.
pub fn decode_svg(source: &[u8]) -> Option<SvgGraphic> {
    decode_svg_with_options(source, &Default::default())
}

pub fn decode_svg_with_options(source: &[u8], options: &usvg::Options) -> Option<SvgGraphic> {
    let svg = crate::SvgImageData::from_data(source, options).ok()?;
    Some(SvgGraphic::from_decoded(
        svg.tree,
        svg.intrinsic_dimensions.view_box,
        svg.intrinsic_dimensions.preserve_aspect_ratio,
        svg.intrinsic_dimensions.degenerate_view_box,
    ))
}
