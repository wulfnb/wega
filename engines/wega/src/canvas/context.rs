//! Canvas 2D Context and Canvas Engine implementation using Skia raster surfaces.

use super::path::CanvasPath;
use super::state::{CanvasState, parse_composite_operation};
use super::style::CanvasStyle;
use skia_safe::paint::Style;
use skia_safe::{
    AlphaType, ClipOp, Color, ColorType, EncodedImageFormat, Font, ImageInfo, Matrix, Paint, Point, Rect, Surface,
    surfaces,
};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CanvasId(pub u64);

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub struct CanvasContext2D {
    pub id: CanvasId,
    pub width: u32,
    pub height: u32,
    pub surface: Surface,
    pub state_stack: Vec<CanvasState>,
    pub current_state: CanvasState,
    pub current_path: CanvasPath,
    pub origin_clean: bool,
    pub is_offscreen: bool,
    pub detached: bool,
}

impl CanvasContext2D {
    pub fn new(id: CanvasId, width: u32, height: u32, is_offscreen: bool) -> Result<Self, String> {
        let w = width.max(1) as i32;
        let h = height.max(1) as i32;
        let surface = surfaces::raster_n32_premul((w, h))
            .ok_or_else(|| format!("Failed to create Skia raster surface {}x{}", width, height))?;

        let mut ctx = Self {
            id,
            width,
            height,
            surface,
            state_stack: Vec::new(),
            current_state: CanvasState::default(),
            current_path: CanvasPath::new(),
            origin_clean: true,
            is_offscreen,
            detached: false,
        };
        ctx.clear_to_transparent();
        Ok(ctx)
    }

    pub fn clear_to_transparent(&mut self) {
        let canvas = self.surface.canvas();
        canvas.clear(Color::from_argb(0, 0, 0, 0));
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        if self.detached {
            return Err("InvalidStateError: Canvas is detached".into());
        }
        let w = width.max(1) as i32;
        let h = height.max(1) as i32;
        let surface = surfaces::raster_n32_premul((w, h))
            .ok_or_else(|| format!("Failed to resize Skia raster surface {}x{}", width, height))?;

        self.width = width;
        self.height = height;
        self.surface = surface;
        self.state_stack.clear();
        self.current_state = CanvasState::default();
        self.current_path.begin_path();
        self.clear_to_transparent();
        Ok(())
    }

    pub fn save(&mut self) {
        self.state_stack.push(self.current_state.clone());
        self.surface.canvas().save();
    }

    pub fn restore(&mut self) {
        if let Some(state) = self.state_stack.pop() {
            self.current_state = state;
            self.surface.canvas().restore();
        }
    }

    pub fn reset(&mut self) {
        self.state_stack.clear();
        self.current_state = CanvasState::default();
        self.current_path.begin_path();
        self.surface.canvas().restore_to_count(1);
        self.clear_to_transparent();
    }

    // Transformations
    pub fn scale(&mut self, sx: f32, sy: f32) {
        let m = Matrix::scale((sx, sy));
        self.current_state.matrix.pre_concat(&m);
        self.surface.canvas().scale((sx, sy));
    }

    pub fn rotate(&mut self, angle: f32) {
        let deg = angle.to_degrees();
        self.current_state.matrix.pre_rotate(deg, None);
        self.surface.canvas().rotate(deg, None);
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.current_state.matrix.pre_translate((dx, dy));
        self.surface.canvas().translate((dx, dy));
    }

    pub fn transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        let mut m = Matrix::default();
        m.set_all(a, c, e, b, d, f, 0.0, 0.0, 1.0);
        self.current_state.matrix.pre_concat(&m);
        self.surface.canvas().concat(&m);
    }

    pub fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        let mut m = Matrix::default();
        m.set_all(a, c, e, b, d, f, 0.0, 0.0, 1.0);
        self.current_state.matrix = m;
        self.surface.canvas().restore_to_count(1);
        self.surface.canvas().save();
        self.surface.canvas().concat(&m);
    }

    pub fn reset_transform(&mut self) {
        self.set_transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    }

    // Drawing rects
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let paint = self.build_paint(Style::Fill);
        self.surface
            .canvas()
            .draw_rect(Rect::from_xywh(x, y, w, h), &paint);
    }

    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let paint = self.build_paint(Style::Stroke);
        self.surface
            .canvas()
            .draw_rect(Rect::from_xywh(x, y, w, h), &paint);
    }

    pub fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let mut paint = Paint::default();
        paint.set_blend_mode(skia_safe::BlendMode::Clear);
        self.surface
            .canvas()
            .draw_rect(Rect::from_xywh(x, y, w, h), &paint);
    }

    // Paths
    pub fn begin_path(&mut self) {
        self.current_path.begin_path();
    }

    pub fn close_path(&mut self) {
        self.current_path.close_path();
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.current_path.move_to(x, y);
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        self.current_path.line_to(x, y);
    }

    pub fn quadratic_curve_to(&mut self, cpx: f32, cpy: f32, x: f32, y: f32) {
        self.current_path.quadratic_curve_to(cpx, cpy, x, y);
    }

    pub fn bezier_curve_to(&mut self, cp1x: f32, cp1y: f32, cp2x: f32, cp2y: f32, x: f32, y: f32) {
        self.current_path
            .bezier_curve_to(cp1x, cp1y, cp2x, cp2y, x, y);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.current_path.rect(x, y, w, h);
    }

    pub fn arc(
        &mut self,
        x: f32,
        y: f32,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
        counterclockwise: bool,
    ) {
        self.current_path
            .arc(x, y, radius, start_angle, end_angle, counterclockwise);
    }

    pub fn ellipse(
        &mut self,
        x: f32,
        y: f32,
        radius_x: f32,
        radius_y: f32,
        rotation: f32,
        start_angle: f32,
        end_angle: f32,
        counterclockwise: bool,
    ) {
        self.current_path.ellipse(
            x,
            y,
            radius_x,
            radius_y,
            rotation,
            start_angle,
            end_angle,
            counterclockwise,
        );
    }

    pub fn fill(&mut self) {
        let paint = self.build_paint(Style::Fill);
        let path = self.current_path.snapshot();
        self.surface.canvas().draw_path(&path, &paint);
    }

    pub fn stroke(&mut self) {
        let paint = self.build_paint(Style::Stroke);
        let path = self.current_path.snapshot();
        self.surface.canvas().draw_path(&path, &paint);
    }

    pub fn clip(&mut self) {
        let path = self.current_path.snapshot();
        self.surface
            .canvas()
            .clip_path(&path, ClipOp::Intersect, true);
    }

    pub fn is_point_in_path(&self, x: f32, y: f32) -> bool {
        let path = self.current_path.snapshot();
        path.contains(Point::new(x, y))
    }

    // Text
    pub fn fill_text(&mut self, text: &str, x: f32, y: f32, _max_width: Option<f32>) {
        let paint = self.build_paint(Style::Fill);
        let font = self.build_font();
        self.surface.canvas().draw_str(text, (x, y), &font, &paint);
    }

    pub fn stroke_text(&mut self, text: &str, x: f32, y: f32, _max_width: Option<f32>) {
        let paint = self.build_paint(Style::Stroke);
        let font = self.build_font();
        self.surface.canvas().draw_str(text, (x, y), &font, &paint);
    }

    pub fn measure_text(&self, text: &str) -> (f32, f32, f32) {
        let font = self.build_font();
        let (mut width, bounds) = font.measure_str(text, None);
        let mut ascent = bounds.top.abs();
        let mut descent = bounds.bottom.max(0.0);
        if width == 0.0 && !text.is_empty() {
            width = (text.len() as f32) * self.current_state.font_size * 0.6;
            ascent = self.current_state.font_size * 0.8;
            descent = self.current_state.font_size * 0.2;
        }
        (width, ascent, descent)
    }

    // Pixel manipulation
    pub fn get_image_data(
        &mut self,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
    ) -> Result<Vec<u8>, String> {
        if !self.origin_clean {
            return Err("SecurityError: The operation is insecure".into());
        }
        let info = ImageInfo::new((sw, sh), ColorType::RGBA8888, AlphaType::Unpremul, None);
        let mut pixels = vec![0u8; (sw * sh * 4) as usize];
        let read = self
            .surface
            .read_pixels(&info, &mut pixels, (sw * 4) as usize, (sx, sy));
        if !read {
            return Err("Failed to read pixels from canvas".into());
        }
        Ok(pixels)
    }

    pub fn put_image_data(
        &mut self,
        pixels: &[u8],
        dx: i32,
        dy: i32,
        sw: i32,
        sh: i32,
    ) -> Result<(), String> {
        let info = ImageInfo::new((sw, sh), ColorType::RGBA8888, AlphaType::Unpremul, None);
        let image = skia_safe::images::raster_from_data(
            &info,
            skia_safe::Data::new_copy(pixels),
            (sw * 4) as usize,
        )
        .ok_or("Failed to create raster image from pixels")?;

        let mut paint = Paint::default();
        paint.set_blend_mode(skia_safe::BlendMode::Src);
        self.surface
            .canvas()
            .draw_image(image, (dx as f32, dy as f32), Some(&paint));
        Ok(())
    }

    pub fn to_data_url(&mut self, _mime_type: &str) -> Result<String, String> {
        if !self.origin_clean {
            return Err("SecurityError: The operation is insecure".into());
        }
        let image = self.surface.image_snapshot();
        let data = image
            .encode(None, EncodedImageFormat::PNG, 100)
            .ok_or("Failed to encode canvas image to PNG")?;

        let bytes = data.as_bytes();
        let base64 = base64_encode(bytes);
        Ok(format!("data:image/png;base64,{}", base64))
    }

    pub fn to_blob_bytes(&mut self) -> Result<Vec<u8>, String> {
        if !self.origin_clean {
            return Err("SecurityError: The operation is insecure".into());
        }
        let image = self.surface.image_snapshot();
        let data = image
            .encode(None, EncodedImageFormat::PNG, 100)
            .ok_or("Failed to encode canvas image to PNG")?;

        Ok(data.as_bytes().to_vec())
    }

    pub fn draw_image(
        &mut self,
        source_pixels: &[u8],
        src_w: i32,
        src_h: i32,
        sx: f32,
        sy: f32,
        sw: f32,
        sh: f32,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
        origin_clean: bool,
    ) -> Result<(), String> {
        if !origin_clean {
            self.origin_clean = false;
        }

        let info = ImageInfo::new_n32_premul((src_w, src_h), None);
        let image = skia_safe::images::raster_from_data(
            &info,
            skia_safe::Data::new_copy(source_pixels),
            (src_w * 4) as usize,
        )
        .ok_or("Failed to create image from source pixels")?;

        let src_rect = Rect::from_xywh(sx, sy, sw, sh);
        let dst_rect = Rect::from_xywh(dx, dy, dw, dh);
        let paint = self.build_paint(Style::Fill);

        self.surface.canvas().draw_image_rect(
            image,
            Some((&src_rect, skia_safe::canvas::SrcRectConstraint::Fast)),
            dst_rect,
            &paint,
        );
        Ok(())
    }

    pub fn execute_command(
        &mut self,
        cmd: &str,
        args: &[serde_json::Value],
    ) -> Result<serde_json::Value, String> {
        let f =
            |idx: usize| -> f32 { args.get(idx).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32 };
        let s = |idx: usize| -> &str { args.get(idx).and_then(|v| v.as_str()).unwrap_or("") };
        let b = |idx: usize| -> bool { args.get(idx).and_then(|v| v.as_bool()).unwrap_or(false) };

        match cmd {
            "save" => {
                self.save();
                Ok(serde_json::Value::Null)
            }
            "restore" => {
                self.restore();
                Ok(serde_json::Value::Null)
            }
            "reset" => {
                self.reset();
                Ok(serde_json::Value::Null)
            }
            "scale" => {
                self.scale(f(0), f(1));
                Ok(serde_json::Value::Null)
            }
            "rotate" => {
                self.rotate(f(0));
                Ok(serde_json::Value::Null)
            }
            "translate" => {
                self.translate(f(0), f(1));
                Ok(serde_json::Value::Null)
            }
            "transform" => {
                self.transform(f(0), f(1), f(2), f(3), f(4), f(5));
                Ok(serde_json::Value::Null)
            }
            "setTransform" => {
                self.set_transform(f(0), f(1), f(2), f(3), f(4), f(5));
                Ok(serde_json::Value::Null)
            }
            "resetTransform" => {
                self.reset_transform();
                Ok(serde_json::Value::Null)
            }
            "clearRect" => {
                self.clear_rect(f(0), f(1), f(2), f(3));
                Ok(serde_json::Value::Null)
            }
            "fillRect" => {
                self.fill_rect(f(0), f(1), f(2), f(3));
                Ok(serde_json::Value::Null)
            }
            "strokeRect" => {
                self.stroke_rect(f(0), f(1), f(2), f(3));
                Ok(serde_json::Value::Null)
            }
            "beginPath" => {
                self.begin_path();
                Ok(serde_json::Value::Null)
            }
            "closePath" => {
                self.close_path();
                Ok(serde_json::Value::Null)
            }
            "moveTo" => {
                self.move_to(f(0), f(1));
                Ok(serde_json::Value::Null)
            }
            "lineTo" => {
                self.line_to(f(0), f(1));
                Ok(serde_json::Value::Null)
            }
            "quadraticCurveTo" => {
                self.quadratic_curve_to(f(0), f(1), f(2), f(3));
                Ok(serde_json::Value::Null)
            }
            "bezierCurveTo" => {
                self.bezier_curve_to(f(0), f(1), f(2), f(3), f(4), f(5));
                Ok(serde_json::Value::Null)
            }
            "rect" => {
                self.rect(f(0), f(1), f(2), f(3));
                Ok(serde_json::Value::Null)
            }
            "arc" => {
                self.arc(f(0), f(1), f(2), f(3), f(4), b(5));
                Ok(serde_json::Value::Null)
            }
            "ellipse" => {
                self.ellipse(f(0), f(1), f(2), f(3), f(4), f(5), f(6), b(7));
                Ok(serde_json::Value::Null)
            }
            "fill" => {
                self.fill();
                Ok(serde_json::Value::Null)
            }
            "stroke" => {
                self.stroke();
                Ok(serde_json::Value::Null)
            }
            "clip" => {
                self.clip();
                Ok(serde_json::Value::Null)
            }
            "fillStyle" => {
                if let Some(color) = CanvasStyle::parse_color(s(0)) {
                    self.current_state.fill_style = CanvasStyle::Color(color);
                }
                Ok(serde_json::Value::Null)
            }
            "strokeStyle" => {
                if let Some(color) = CanvasStyle::parse_color(s(0)) {
                    self.current_state.stroke_style = CanvasStyle::Color(color);
                }
                Ok(serde_json::Value::Null)
            }
            "lineWidth" => {
                let w = f(0);
                if w > 0.0 {
                    self.current_state.line_width = w;
                }
                Ok(serde_json::Value::Null)
            }
            "lineCap" => {
                match s(0).to_ascii_lowercase().as_str() {
                    "round" => self.current_state.line_cap = skia_safe::paint::Cap::Round,
                    "square" => self.current_state.line_cap = skia_safe::paint::Cap::Square,
                    _ => self.current_state.line_cap = skia_safe::paint::Cap::Butt,
                }
                Ok(serde_json::Value::Null)
            }
            "lineJoin" => {
                match s(0).to_ascii_lowercase().as_str() {
                    "round" => self.current_state.line_join = skia_safe::paint::Join::Round,
                    "bevel" => self.current_state.line_join = skia_safe::paint::Join::Bevel,
                    _ => self.current_state.line_join = skia_safe::paint::Join::Miter,
                }
                Ok(serde_json::Value::Null)
            }
            "miterLimit" => {
                let m = f(0);
                if m > 0.0 {
                    self.current_state.miter_limit = m;
                }
                Ok(serde_json::Value::Null)
            }
            "globalAlpha" => {
                let a = f(0);
                if (0.0..=1.0).contains(&a) {
                    self.current_state.global_alpha = a;
                }
                Ok(serde_json::Value::Null)
            }
            "globalCompositeOperation" => {
                self.current_state.global_composite_operation = parse_composite_operation(s(0));
                Ok(serde_json::Value::Null)
            }
            "font" => {
                let size = f(0);
                let family = s(1);
                if size > 0.0 {
                    self.current_state.font_size = size;
                }
                if !family.is_empty() {
                    self.current_state.font_family = family.to_string();
                }
                Ok(serde_json::Value::Null)
            }
            "fillText" => {
                let max_w = if args.len() > 3 && !args[3].is_null() {
                    Some(f(3))
                } else {
                    None
                };
                self.fill_text(s(0), f(1), f(2), max_w);
                Ok(serde_json::Value::Null)
            }
            "strokeText" => {
                let max_w = if args.len() > 3 && !args[3].is_null() {
                    Some(f(3))
                } else {
                    None
                };
                self.stroke_text(s(0), f(1), f(2), max_w);
                Ok(serde_json::Value::Null)
            }
            "isPointInPath" => Ok(serde_json::json!(self.is_point_in_path(f(0), f(1)))),
            _ => Err(format!("Unknown canvas command: {}", cmd)),
        }
    }

    fn build_paint(&self, style: Style) -> Paint {
        let mut paint = Paint::default();
        paint.set_style(style);
        paint.set_anti_alias(true);
        paint.set_blend_mode(self.current_state.global_composite_operation);

        match style {
            Style::Fill => {
                if let Some(shader) = self.current_state.fill_style.to_shader() {
                    paint.set_shader(shader);
                }
            }
            Style::Stroke => {
                paint.set_stroke_width(self.current_state.line_width);
                paint.set_stroke_cap(self.current_state.line_cap);
                paint.set_stroke_join(self.current_state.line_join);
                paint.set_stroke_miter(self.current_state.miter_limit);
                if let Some(shader) = self.current_state.stroke_style.to_shader() {
                    paint.set_shader(shader);
                }
            }
            _ => {}
        }

        paint.set_alpha_f(self.current_state.global_alpha);
        paint
    }

    fn build_font(&self) -> Font {
        let mut font = Font::default();
        font.set_size(self.current_state.font_size);
        font
    }
}

pub struct CanvasEngine {
    canvases: HashMap<CanvasId, CanvasContext2D>,
    next_id: u64,
    total_pixel_bytes: usize,
    max_pixel_bytes: usize,
}

impl Default for CanvasEngine {
    fn default() -> Self {
        Self::new(64 * 1024 * 1024)
    }
}

impl CanvasEngine {
    pub fn new(max_pixel_bytes: usize) -> Self {
        Self {
            canvases: HashMap::new(),
            next_id: 1,
            total_pixel_bytes: 0,
            max_pixel_bytes,
        }
    }

    pub fn total_pixel_bytes(&self) -> usize {
        self.total_pixel_bytes
    }

    pub fn create_canvas(
        &mut self,
        width: u32,
        height: u32,
        is_offscreen: bool,
    ) -> Result<CanvasId, String> {
        let bytes = (width as usize) * (height as usize) * 4;
        if self.total_pixel_bytes + bytes > self.max_pixel_bytes {
            return Err("QuotaExceededError: Canvas memory quota exceeded".into());
        }

        let id = CanvasId(self.next_id);
        self.next_id += 1;
        let ctx = CanvasContext2D::new(id, width, height, is_offscreen)?;
        self.canvases.insert(id, ctx);
        self.total_pixel_bytes += bytes;
        Ok(id)
    }

    pub fn resize_canvas(&mut self, id: CanvasId, width: u32, height: u32) -> Result<(), String> {
        let old_bytes = {
            let ctx = self.canvases.get(&id).ok_or("Canvas not found")?;
            (ctx.width as usize) * (ctx.height as usize) * 4
        };
        let new_bytes = (width as usize) * (height as usize) * 4;
        if self.total_pixel_bytes - old_bytes + new_bytes > self.max_pixel_bytes {
            return Err("QuotaExceededError: Canvas memory quota exceeded".into());
        }

        let ctx = self.canvases.get_mut(&id).ok_or("Canvas not found")?;
        ctx.resize(width, height)?;
        self.total_pixel_bytes = self.total_pixel_bytes - old_bytes + new_bytes;
        Ok(())
    }

    pub fn destroy_canvas(&mut self, id: CanvasId) -> bool {
        if let Some(ctx) = self.canvases.remove(&id) {
            let bytes = (ctx.width as usize) * (ctx.height as usize) * 4;
            self.total_pixel_bytes = self.total_pixel_bytes.saturating_sub(bytes);
            true
        } else {
            false
        }
    }

    pub fn get_mut(&mut self, id: CanvasId) -> Option<&mut CanvasContext2D> {
        self.canvases.get_mut(&id)
    }

    pub fn get(&self, id: CanvasId) -> Option<&CanvasContext2D> {
        self.canvases.get(&id)
    }
}
