//! Canvas 2D Path representation and Path2D primitives.

use skia_safe::{Path, PathBuilder, PathDirection, Point, Rect};

#[derive(Clone, Debug)]
pub struct CanvasPath {
    pub builder: PathBuilder,
}

impl Default for CanvasPath {
    fn default() -> Self {
        Self::new()
    }
}

impl CanvasPath {
    pub fn new() -> Self {
        Self {
            builder: PathBuilder::new(),
        }
    }

    pub fn snapshot(&self) -> Path {
        self.builder.snapshot()
    }

    pub fn begin_path(&mut self) {
        self.builder.reset();
    }

    pub fn close_path(&mut self) {
        self.builder.close();
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.builder.move_to(Point::new(x, y));
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(Point::new(x, y));
    }

    pub fn quadratic_curve_to(&mut self, cpx: f32, cpy: f32, x: f32, y: f32) {
        self.builder.quad_to(Point::new(cpx, cpy), Point::new(x, y));
    }

    pub fn bezier_curve_to(&mut self, cp1x: f32, cp1y: f32, cp2x: f32, cp2y: f32, x: f32, y: f32) {
        self.builder.cubic_to(
            Point::new(cp1x, cp1y),
            Point::new(cp2x, cp2y),
            Point::new(x, y),
        );
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.builder
            .add_rect(Rect::from_xywh(x, y, w, h), Some(PathDirection::CW), None);
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
        let diff = (end_angle - start_angle).abs();
        let rect = Rect::from_xywh(x - radius, y - radius, radius * 2.0, radius * 2.0);
        if diff >= std::f32::consts::TAU {
            self.builder.add_oval(rect, Some(PathDirection::CW), None);
            return;
        }

        let sweep = if counterclockwise {
            let mut s = (end_angle - start_angle).to_degrees();
            if s > 0.0 {
                s -= 360.0;
            }
            s
        } else {
            let mut s = (end_angle - start_angle).to_degrees();
            if s < 0.0 {
                s += 360.0;
            }
            s
        };
        self.builder
            .arc_to(rect, start_angle.to_degrees(), sweep, false);
    }

    pub fn ellipse(
        &mut self,
        x: f32,
        y: f32,
        radius_x: f32,
        radius_y: f32,
        _rotation: f32,
        start_angle: f32,
        end_angle: f32,
        counterclockwise: bool,
    ) {
        let diff = (end_angle - start_angle).abs();
        let rect = Rect::from_xywh(x - radius_x, y - radius_y, radius_x * 2.0, radius_y * 2.0);
        if diff >= std::f32::consts::TAU {
            self.builder.add_oval(rect, Some(PathDirection::CW), None);
            return;
        }

        let sweep = if counterclockwise {
            let mut s = (end_angle - start_angle).to_degrees();
            if s > 0.0 {
                s -= 360.0;
            }
            s
        } else {
            let mut s = (end_angle - start_angle).to_degrees();
            if s < 0.0 {
                s += 360.0;
            }
            s
        };
        self.builder
            .arc_to(rect, start_angle.to_degrees(), sweep, false);
    }
}
