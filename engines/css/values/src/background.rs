//! Background and mask image, position and size values; `clip-path` shapes.
use crate::*;

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
                // `<length-percentage [0,∞]> | auto`, once or twice.
                let size = |v: &str| {
                    Length::parse(v).filter(|l| match l {
                        Length::Px(v) | Length::Percent(v) | Length::Em(v) | Length::Rem(v) => {
                            *v >= 0.0
                        }
                        _ => true,
                    })
                };
                let mut p = value.split_whitespace();
                let size = Self::Explicit(size(p.next()?)?, size(p.next().unwrap_or("auto"))?);
                if p.next().is_some() {
                    return None;
                }
                size
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
    pub fn path(&self, r: kurbo::Rect, fs: f64) -> kurbo::BezPath {
        use kurbo::Shape;
        let r = Bounds {
            x: r.x0,
            y: r.y0,
            width: r.width(),
            height: r.height(),
        };
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

/// The reference box of a `clip-path` shape.
pub(crate) struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
impl Bounds {
    fn right(&self) -> f64 {
        self.x + self.width
    }
    fn bottom(&self) -> f64 {
        self.y + self.height
    }
}
