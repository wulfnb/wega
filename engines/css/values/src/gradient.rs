//! Linear gradients.
use crate::*;

#[derive(Debug, Clone, PartialEq)]
pub struct LinearGradient {
    /// CSS gradient angle in degrees (0 = to top, 90 = to right).
    pub angle: f32,
    /// Color stops with optional positions as fractions of the gradient line.
    pub stops: Vec<(Color, Option<f32>)>,
}

impl LinearGradient {
    /// Stops with every position resolved, in increasing order.
    pub fn resolved_stops(&self) -> Vec<(f32, Color)> {
        let n = self.stops.len();
        let mut positions: Vec<Option<f32>> = self.stops.iter().map(|s| s.1).collect();
        if let Some(first) = positions.first_mut() {
            first.get_or_insert(0.0);
        }
        if let Some(last) = positions.last_mut() {
            last.get_or_insert(1.0);
        }
        // Clamp to be non-decreasing, then spread unpositioned stops evenly.
        let mut max = 0.0f32;
        for p in positions.iter_mut().flatten() {
            *p = p.max(max);
            max = *p;
        }
        let mut i = 0;
        while i < n {
            if positions[i].is_none() {
                let start = i - 1;
                let end = (i..n).find(|j| positions[*j].is_some()).unwrap_or(n - 1);
                let (a, b) = (
                    positions[start].unwrap_or(0.0),
                    positions[end].unwrap_or(1.0),
                );
                for (k, p) in positions.iter_mut().enumerate().take(end).skip(i) {
                    *p = Some(a + (b - a) * (k - start) as f32 / (end - start) as f32);
                }
                i = end;
            }
            i += 1;
        }
        positions
            .into_iter()
            .zip(&self.stops)
            .map(|(p, (c, _))| (p.unwrap_or(0.0), *c))
            .collect()
    }
    /// The color halfway along the gradient line.
    pub fn midpoint(&self) -> Color {
        let stops = self.resolved_stops();
        let mut previous = stops[0];
        for &(p, c) in &stops {
            if p >= 0.5 {
                let span = (p - previous.0).max(f32::EPSILON);
                let t = ((0.5 - previous.0) / span).clamp(0.0, 1.0);
                let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
                return Color {
                    r: mix(previous.1.r, c.r),
                    g: mix(previous.1.g, c.g),
                    b: mix(previous.1.b, c.b),
                    a: mix(previous.1.a, c.a),
                };
            }
            previous = (p, c);
        }
        previous.1
    }
}
