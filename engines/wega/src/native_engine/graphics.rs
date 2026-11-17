//! SVGs use the existing bounded decoder. No file or network resolver is enabled.
use super::css::Length;
use std::sync::Arc;
use wega_dom::{DocumentView, NodeKey, NodeKind, NodeView};

pub fn image_url(value: &str) -> Option<String> {
    let mut parser = cssparser::Parser::new(value);
    parser.expect_url_or_string().ok().map(|s| s.to_string())
}
pub fn position(value: &str) -> Option<(Length, Length)> {
    let mut p = value.split_whitespace();
    let first = p.next()?;
    let second = p.next().unwrap_or("center");
    let length = |v: &str| match v {
        "left" | "top" => Some(Length::Percent(0.0)),
        "center" => Some(Length::Percent(50.0)),
        "right" | "bottom" => Some(Length::Percent(100.0)),
        _ => Length::parse(v),
    };
    if p.next().is_some() {
        return None;
    }
    if matches!(first, "top" | "bottom") {
        Some((length(second)?, length(first)?))
    } else {
        Some((length(first)?, length(second)?))
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum BackgroundSize {
    Auto,
    Contain,
    Cover,
    Explicit(Length, Length),
}
impl BackgroundSize {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "auto" => Self::Auto,
            "contain" => Self::Contain,
            "cover" => Self::Cover,
            _ => {
                let mut p = value.split_whitespace();
                Self::Explicit(
                    Length::parse(p.next()?)?,
                    Length::parse(p.next().unwrap_or("auto"))?,
                )
            }
        })
    }
    pub fn resolve(&self, w: f64, h: f64, iw: f64, ih: f64, fs: f64) -> (f64, f64) {
        match self {
            Self::Auto => (iw, ih),
            Self::Contain | Self::Cover => {
                let a = w / iw;
                let b = h / ih;
                let k = if matches!(self, Self::Cover) {
                    a.max(b)
                } else {
                    a.min(b)
                };
                (iw * k, ih * k)
            }
            Self::Explicit(x, y) => {
                let xa = *x == Length::Auto;
                let ya = *y == Length::Auto;
                let x = x.resolve(w, fs, 16.0);
                let y = y.resolve(h, fs, 16.0);
                if xa && ya {
                    (iw, ih)
                } else if xa {
                    (y * iw / ih, y)
                } else if ya {
                    (x, x * ih / iw)
                } else {
                    (x, y)
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClipPath {
    Polygon(Vec<(Length, Length)>),
    Circle(Length, (Length, Length)),
    Inset([Length; 4]),
}
impl ClipPath {
    pub fn parse(value: &str) -> Option<Self> {
        let (name, args) = value.split_once('(')?;
        let args = args.strip_suffix(')')?;
        match name {
            "polygon" => {
                let points: Option<Vec<_>> = args
                    .split(',')
                    .map(|point| {
                        let mut p = point.split_whitespace();
                        let pair = (Length::parse(p.next()?)?, Length::parse(p.next()?)?);
                        p.next().is_none().then_some(pair)
                    })
                    .collect();
                let points = points?;
                (points.len() >= 3 && points.len() <= 256).then_some(Self::Polygon(points))
            }
            "circle" => {
                let (r, p) = args.split_once(" at ").unwrap_or((args, "50% 50%"));
                Some(Self::Circle(Length::parse(r.trim())?, position(p)?))
            }
            "inset" => {
                let p: Vec<_> = args
                    .split_whitespace()
                    .map(Length::parse)
                    .collect::<Option<_>>()?;
                if p.is_empty() || p.len() > 4 {
                    return None;
                }
                Some(Self::Inset([
                    p[0],
                    *p.get(1).unwrap_or(&p[0]),
                    *p.get(2).unwrap_or(&p[0]),
                    *p.get(3).or(p.get(1)).unwrap_or(&p[0]),
                ]))
            }
            _ => None,
        }
    }
    pub fn path(&self, r: wega_dom::layout::Rect, fs: f64) -> kurbo::BezPath {
        use kurbo::Shape;
        let point = |p: &(Length, Length)| {
            kurbo::Point::new(
                r.x + p.0.resolve(r.width, fs, 16.0),
                r.y + p.1.resolve(r.height, fs, 16.0),
            )
        };
        match self {
            Self::Polygon(points) => {
                let mut path = kurbo::BezPath::new();
                path.move_to(point(&points[0]));
                for p in &points[1..] {
                    path.line_to(point(p));
                }
                path.close_path();
                path
            }
            Self::Circle(radius, center) => kurbo::Circle::new(
                point(center),
                radius
                    .resolve(
                        (r.width * r.width + r.height * r.height).sqrt() / 2f64.sqrt(),
                        fs,
                        16.0,
                    )
                    .max(0.0),
            )
            .to_path(0.1),
            Self::Inset(p) => kurbo::Rect::new(
                r.x + p[3].resolve(r.width, fs, 16.0),
                r.y + p[0].resolve(r.height, fs, 16.0),
                r.right() - p[1].resolve(r.width, fs, 16.0),
                r.bottom() - p[2].resolve(r.height, fs, 16.0),
            )
            .to_path(0.1),
        }
    }
}

pub fn inline_svg<D: DocumentView>(doc: &D, key: NodeKey) -> Option<SvgGraphic> {
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
    let svg =
        blitz_dom::node::SvgImageData::from_data(xml.as_bytes(), &usvg::Options::default()).ok()?;
    Some(SvgGraphic::from(&svg))
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

#[derive(Debug, Clone)]
pub struct SvgGraphic {
    pub tree: Arc<usvg::Tree>,
    view_box: Option<usvg::NonZeroRect>,
    aspect: svgtypes::AspectRatio,
    pub degenerate: bool,
}
impl From<&blitz_dom::node::SvgImageData> for SvgGraphic {
    fn from(svg: &blitz_dom::node::SvgImageData) -> Self {
        Self {
            tree: svg.tree.clone(),
            view_box: svg.intrinsic_dimensions.view_box,
            aspect: svg.intrinsic_dimensions.preserve_aspect_ratio,
            degenerate: svg.intrinsic_dimensions.degenerate_view_box,
        }
    }
}
impl SvgGraphic {
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
