//! Canvas 2D drawing state machine and stack.

use super::style::CanvasStyle;
use skia_safe::paint::{Cap, Join};
use skia_safe::{BlendMode, Color, Matrix};

#[derive(Clone, Debug)]
pub struct CanvasState {
    pub matrix: Matrix,
    pub fill_style: CanvasStyle,
    pub stroke_style: CanvasStyle,
    pub global_alpha: f32,
    pub global_composite_operation: BlendMode,
    pub line_width: f32,
    pub line_cap: Cap,
    pub line_join: Join,
    pub miter_limit: f32,
    pub line_dash: Vec<f32>,
    pub line_dash_offset: f32,
    pub shadow_color: Color,
    pub shadow_blur: f32,
    pub shadow_offset_x: f32,
    pub shadow_offset_y: f32,
    pub font_size: f32,
    pub font_family: String,
    pub text_align: String,
    pub text_baseline: String,
    pub image_smoothing_enabled: bool,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            matrix: Matrix::default(),
            fill_style: CanvasStyle::Color(Color::from_argb(255, 0, 0, 0)),
            stroke_style: CanvasStyle::Color(Color::from_argb(255, 0, 0, 0)),
            global_alpha: 1.0,
            global_composite_operation: BlendMode::SrcOver,
            line_width: 1.0,
            line_cap: Cap::Butt,
            line_join: Join::Miter,
            miter_limit: 10.0,
            line_dash: Vec::new(),
            line_dash_offset: 0.0,
            shadow_color: Color::from_argb(0, 0, 0, 0),
            shadow_blur: 0.0,
            shadow_offset_x: 0.0,
            shadow_offset_y: 0.0,
            font_size: 10.0,
            font_family: "sans-serif".into(),
            text_align: "start".into(),
            text_baseline: "alphabetic".into(),
            image_smoothing_enabled: true,
        }
    }
}

pub fn parse_composite_operation(op: &str) -> BlendMode {
    match op.trim().to_ascii_lowercase().as_str() {
        "source-over" => BlendMode::SrcOver,
        "source-in" => BlendMode::SrcIn,
        "source-out" => BlendMode::SrcOut,
        "source-atop" => BlendMode::SrcATop,
        "destination-over" => BlendMode::DstOver,
        "destination-in" => BlendMode::DstIn,
        "destination-out" => BlendMode::DstOut,
        "destination-atop" => BlendMode::DstATop,
        "lighter" => BlendMode::Plus,
        "copy" => BlendMode::Src,
        "xor" => BlendMode::Xor,
        "multiply" => BlendMode::Multiply,
        "screen" => BlendMode::Screen,
        "overlay" => BlendMode::Overlay,
        "darken" => BlendMode::Darken,
        "lighten" => BlendMode::Lighten,
        "color-dodge" => BlendMode::ColorDodge,
        "color-burn" => BlendMode::ColorBurn,
        "hard-light" => BlendMode::HardLight,
        "soft-light" => BlendMode::SoftLight,
        "difference" => BlendMode::Difference,
        "exclusion" => BlendMode::Exclusion,
        "hue" => BlendMode::Hue,
        "saturation" => BlendMode::Saturation,
        "color" => BlendMode::Color,
        "luminosity" => BlendMode::Luminosity,
        _ => BlendMode::SrcOver,
    }
}
