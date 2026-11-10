#![allow(deprecated)]
//! Canvas fill and stroke styles, colors, gradients, and patterns.

use serde::{Deserialize, Serialize};
use skia_safe::{Color, Point, Shader, TileMode, gradient_shader, shaders};

#[derive(Clone, Debug, PartialEq)]
pub enum CanvasStyle {
    Color(Color),
    LinearGradient {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        stops: Vec<(f32, Color)>,
    },
    RadialGradient {
        x0: f32,
        y0: f32,
        r0: f32,
        x1: f32,
        y1: f32,
        r1: f32,
        stops: Vec<(f32, Color)>,
    },
    ConicGradient {
        start_angle: f32,
        x: f32,
        y: f32,
        stops: Vec<(f32, Color)>,
    },
    Pattern {
        image_bytes: Vec<u8>,
        width: u32,
        height: u32,
        repetition: PatternRepetition,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternRepetition {
    Repeat,
    RepeatX,
    RepeatY,
    NoRepeat,
}

impl Default for CanvasStyle {
    fn default() -> Self {
        CanvasStyle::Color(Color::from_argb(255, 0, 0, 0))
    }
}

impl CanvasStyle {
    pub fn parse_color(s: &str) -> Option<Color> {
        let s = s.trim();
        if s.starts_with('#') {
            let hex = &s[1..];
            match hex.len() {
                3 => {
                    let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                    let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                    let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                    Some(Color::from_argb(255, r, g, b))
                }
                4 => {
                    let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                    let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                    let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                    let a = u8::from_str_radix(&hex[3..4], 16).ok()? * 17;
                    Some(Color::from_argb(a, r, g, b))
                }
                6 => {
                    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                    Some(Color::from_argb(255, r, g, b))
                }
                8 => {
                    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                    let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                    Some(Color::from_argb(a, r, g, b))
                }
                _ => None,
            }
        } else if s.starts_with("rgb") {
            let start = s.find('(')? + 1;
            let end = s.find(')')?;
            let inner = &s[start..end];
            let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
            if parts.len() == 3 {
                let r: u8 = parts[0].parse().ok()?;
                let g: u8 = parts[1].parse().ok()?;
                let b: u8 = parts[2].parse().ok()?;
                Some(Color::from_argb(255, r, g, b))
            } else if parts.len() == 4 {
                let r: u8 = parts[0].parse().ok()?;
                let g: u8 = parts[1].parse().ok()?;
                let b: u8 = parts[2].parse().ok()?;
                let a: f32 = parts[3].parse().ok()?;
                let a_u8 = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
                Some(Color::from_argb(a_u8, r, g, b))
            } else {
                None
            }
        } else {
            match s.to_ascii_lowercase().as_str() {
                "transparent" => Some(Color::from_argb(0, 0, 0, 0)),
                "black" => Some(Color::from_argb(255, 0, 0, 0)),
                "white" => Some(Color::from_argb(255, 255, 255, 255)),
                "red" => Some(Color::from_argb(255, 255, 0, 0)),
                "green" => Some(Color::from_argb(255, 0, 128, 0)),
                "lime" => Some(Color::from_argb(255, 0, 255, 0)),
                "blue" => Some(Color::from_argb(255, 0, 0, 255)),
                "yellow" => Some(Color::from_argb(255, 255, 255, 0)),
                "cyan" | "aqua" => Some(Color::from_argb(255, 0, 255, 255)),
                "magenta" | "fuchsia" => Some(Color::from_argb(255, 255, 0, 255)),
                "gray" | "grey" => Some(Color::from_argb(255, 128, 128, 128)),
                "orange" => Some(Color::from_argb(255, 255, 165, 0)),
                "purple" => Some(Color::from_argb(255, 128, 0, 128)),
                "navy" => Some(Color::from_argb(255, 0, 0, 128)),
                "silver" => Some(Color::from_argb(255, 192, 192, 192)),
                _ => None,
            }
        }
    }

    pub fn to_shader(&self) -> Option<Shader> {
        match self {
            CanvasStyle::Color(c) => Some(shaders::color(*c)),
            CanvasStyle::LinearGradient {
                x0,
                y0,
                x1,
                y1,
                stops,
            } => {
                let points = (Point::new(*x0, *y0), Point::new(*x1, *y1));
                let colors: Vec<Color> = stops.iter().map(|(_, c)| *c).collect();
                let pos: Vec<f32> = stops.iter().map(|(p, _)| *p).collect();
                gradient_shader::linear(
                    points,
                    colors.as_slice(),
                    Some(pos.as_slice()),
                    TileMode::Clamp,
                    None,
                    None,
                )
            }
            CanvasStyle::RadialGradient {
                x0,
                y0,
                r0,
                x1,
                y1,
                r1,
                stops,
            } => {
                let start_point = Point::new(*x0, *y0);
                let end_point = Point::new(*x1, *y1);
                let colors: Vec<Color> = stops.iter().map(|(_, c)| *c).collect();
                let pos: Vec<f32> = stops.iter().map(|(p, _)| *p).collect();
                gradient_shader::two_point_conical(
                    start_point,
                    *r0,
                    end_point,
                    *r1,
                    colors.as_slice(),
                    Some(pos.as_slice()),
                    TileMode::Clamp,
                    None,
                    None,
                )
            }
            CanvasStyle::ConicGradient {
                start_angle,
                x,
                y,
                stops,
            } => {
                let center = Point::new(*x, *y);
                let colors: Vec<Color> = stops.iter().map(|(_, c)| *c).collect();
                let pos: Vec<f32> = stops.iter().map(|(p, _)| *p).collect();
                gradient_shader::sweep(
                    center,
                    colors.as_slice(),
                    Some(pos.as_slice()),
                    TileMode::Clamp,
                    Some((
                        *start_angle * 180.0 / std::f32::consts::PI,
                        *start_angle * 180.0 / std::f32::consts::PI + 360.0,
                    )),
                    None,
                    None,
                )
            }
            CanvasStyle::Pattern { .. } => None,
        }
    }
}
