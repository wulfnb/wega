//! Box edges, borders, transforms and shadows.
use crate::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectLengths {
    pub top: Length,
    pub right: Length,
    pub bottom: Length,
    pub left: Length,
}

impl Default for RectLengths {
    fn default() -> Self {
        Self {
            top: Length::Px(0.0),
            right: Length::Px(0.0),
            bottom: Length::Px(0.0),
            left: Length::Px(0.0),
        }
    }
}

impl RectLengths {
    pub fn parse_quad(s: &str) -> Self {
        let parts: Vec<&str> = s.split_whitespace().collect();
        match parts.len() {
            1 => {
                let v = Length::parse(parts[0]).unwrap_or_default();
                Self {
                    top: v,
                    right: v,
                    bottom: v,
                    left: v,
                }
            }
            2 => {
                let tb = Length::parse(parts[0]).unwrap_or_default();
                let lr = Length::parse(parts[1]).unwrap_or_default();
                Self {
                    top: tb,
                    right: lr,
                    bottom: tb,
                    left: lr,
                }
            }
            3 => {
                let top = Length::parse(parts[0]).unwrap_or_default();
                let lr = Length::parse(parts[1]).unwrap_or_default();
                let bottom = Length::parse(parts[2]).unwrap_or_default();
                Self {
                    top,
                    right: lr,
                    bottom,
                    left: lr,
                }
            }
            4.. => {
                let top = Length::parse(parts[0]).unwrap_or_default();
                let right = Length::parse(parts[1]).unwrap_or_default();
                let bottom = Length::parse(parts[2]).unwrap_or_default();
                let left = Length::parse(parts[3]).unwrap_or_default();
                Self {
                    top,
                    right,
                    bottom,
                    left,
                }
            }
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BorderSide {
    pub width: f64,
    pub color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub matrix: [f64; 6],
}

impl Transform {
    pub fn identity() -> Self {
        Self {
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }

    pub fn to_kurbo(&self) -> kurbo::Affine {
        kurbo::Affine::new(self.matrix)
    }

    pub fn parse(s: &str) -> Option<Self> {
        let mut rest = s.trim();
        let mut matrix = kurbo::Affine::IDENTITY;
        let mut count = 0;
        while !rest.is_empty() {
            count += 1;
            if count > 32 {
                return None;
            }
            let open = rest.find('(')?;
            let close = rest.find(')')?;
            let name = rest[..open].trim();
            let args = rest[open + 1..close].replace(',', " ");
            let values: Vec<_> = args.split_whitespace().collect();
            let number = |i: usize| values.get(i)?.parse::<f64>().ok().filter(|v| v.is_finite());
            let length = |i: usize| {
                let v = *values.get(i)?;
                v.strip_suffix("px")
                    .unwrap_or(v)
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
            };
            let angle = |i: usize| {
                let v = *values.get(i)?;
                if let Some(v) = v.strip_suffix("deg") {
                    v.parse::<f64>().ok().map(f64::to_radians)
                } else if let Some(v) = v.strip_suffix("rad") {
                    v.parse::<f64>().ok()
                } else if let Some(v) = v.strip_suffix("turn") {
                    v.parse::<f64>().ok().map(|v| v * std::f64::consts::TAU)
                } else if v == "0" {
                    Some(0.0)
                } else {
                    None
                }
            };
            let transform = match (name, values.len()) {
                ("matrix", 6) => kurbo::Affine::new([
                    number(0)?,
                    number(1)?,
                    number(2)?,
                    number(3)?,
                    number(4)?,
                    number(5)?,
                ]),
                ("translate", 1 | 2) => kurbo::Affine::translate((
                    length(0)?,
                    if values.len() == 2 { length(1)? } else { 0.0 },
                )),
                ("translateX", 1) => kurbo::Affine::translate((length(0)?, 0.0)),
                ("translateY", 1) => kurbo::Affine::translate((0.0, length(0)?)),
                ("scale", 1 | 2) => kurbo::Affine::scale_non_uniform(
                    number(0)?,
                    if values.len() == 2 {
                        number(1)?
                    } else {
                        number(0)?
                    },
                ),
                ("scaleX", 1) => kurbo::Affine::scale_non_uniform(number(0)?, 1.0),
                ("scaleY", 1) => kurbo::Affine::scale_non_uniform(1.0, number(0)?),
                ("rotate", 1) => kurbo::Affine::rotate(angle(0)?),
                ("skewX", 1) => kurbo::Affine::new([1.0, 0.0, angle(0)?.tan(), 1.0, 0.0, 0.0]),
                ("skewY", 1) => kurbo::Affine::new([1.0, angle(0)?.tan(), 0.0, 1.0, 0.0, 0.0]),
                _ => return None,
            };
            matrix *= transform;
            rest = rest[close + 1..].trim();
        }
        let matrix = matrix.as_coeffs();
        (count > 0 && matrix.iter().all(|v| v.is_finite())).then_some(Self { matrix })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    pub offset_x: f64,
    pub offset_y: f64,
    pub blur_radius: f64,
    pub spread: f64,
    pub color: Color,
}

impl BoxShadow {
    /// The first outer shadow layer; inset layers are not painted yet.
    pub fn parse(s: &str) -> Option<Self> {
        let mut depth = 0usize;
        let mut start = 0;
        let mut layers = Vec::new();
        for (i, ch) in s.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    layers.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        layers.push(&s[start..]);
        layers
            .into_iter()
            .filter(|l| !l.to_ascii_lowercase().contains("inset"))
            .find_map(Self::parse_layer)
    }
    fn parse_layer(s: &str) -> Option<Self> {
        let mut parser = cssparser::Parser::new(s.trim());
        let mut lengths = Vec::new();
        let mut color = Color::BLACK;
        while !parser.is_exhausted() {
            let start = parser.position();
            let token = parser.next().ok()?.clone();
            match token {
                cssparser::Token::Dimension { .. } => {
                    lengths.push(match Length::parse(parser.slice_from(start))? {
                        Length::Px(v) => v,
                        Length::Em(v) | Length::Rem(v) => v * 16.0,
                        _ => return None,
                    })
                }
                cssparser::Token::Number { value: 0.0, .. } => lengths.push(0.0),
                cssparser::Token::Function(_) => {
                    parser
                        .parse_nested_block(|p| {
                            while p.next().is_ok() {}
                            Ok::<_, cssparser::ParseError<()>>(())
                        })
                        .ok()?;
                    color = Color::parse(parser.slice_from(start))?;
                }
                cssparser::Token::Ident(ref i) if i.eq_ignore_ascii_case("currentcolor") => {}
                _ => color = Color::parse(parser.slice_from(start))?,
            }
        }
        if !(2..=4).contains(&lengths.len())
            || lengths.iter().any(|v| !v.is_finite())
            || lengths.get(2).is_some_and(|b| *b < 0.0)
        {
            return None;
        }
        Some(Self {
            offset_x: lengths[0],
            offset_y: lengths[1],
            blur_radius: *lengths.get(2).unwrap_or(&0.0),
            spread: *lengths.get(3).unwrap_or(&0.0),
            color,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Borders {
    pub top: BorderSide,
    pub right: BorderSide,
    pub bottom: BorderSide,
    pub left: BorderSide,
}
