//! Lengths and the style pass viewport used by viewport-relative units.
use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
    Px(f64),
    Percent(f64),
    Em(f64),
    Rem(f64),
    Calc([f64; 4]),
    /// `clamp(lo, value, hi)` over linear `[px, %, em, rem]` terms; `min()` and
    /// `max()` use an infinite bound.
    Clamp([[f32; 4]; 3]),
    Vw(f64),
    Vh(f64),
    #[default]
    Auto,
}

impl Length {
    pub fn parse(s: &str) -> Option<Self> {
        let lower = s.trim().to_ascii_lowercase();
        let s = lower.as_str();
        if s.eq_ignore_ascii_case("auto") {
            return Some(Self::Auto);
        }
        if s == "0" || s == "0px" {
            return Some(Self::Px(0.0));
        }
        let split = s
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
            .map(|i| {
                // An exponent `e` followed by a unit letter belongs to the unit (`em`, `ex`).
                if i > 0 && s[..i].ends_with(['e', 'E']) && !s[i..].starts_with(['+', '-']) {
                    i - 1
                } else {
                    i
                }
            })
            .unwrap_or(s.len());
        let (number, unit) = s.split_at(split);
        if let Ok(n) = number.trim().parse::<f64>()
            && n.is_finite()
        {
            // Font-relative `ex`/`ch` approximate half an em; small/large/dynamic
            // viewport units read the current viewport.
            return match unit.trim() {
                "px" => Some(Self::Px(n)),
                "%" => Some(Self::Percent(n)),
                "em" => Some(Self::Em(n)),
                "rem" => Some(Self::Rem(n)),
                "ex" | "ch" | "cap" | "ic" => Some(Self::Em(n * 0.5)),
                "lh" => Some(Self::Em(n * 1.25)),
                "rlh" => Some(Self::Rem(n * 1.25)),
                "vw" | "svw" | "lvw" | "dvw" | "vi" | "svi" | "lvi" | "dvi" | "vmax" | "svmax"
                | "lvmax" | "dvmax" => Some(Self::Vw(n)),
                "vh" | "svh" | "lvh" | "dvh" | "vb" | "svb" | "lvb" | "dvb" | "vmin" | "svmin"
                | "lvmin" | "dvmin" => Some(Self::Vh(n)),
                "pt" => Some(Self::Px(n * 96.0 / 72.0)),
                "pc" => Some(Self::Px(n * 16.0)),
                "in" => Some(Self::Px(n * 96.0)),
                "cm" => Some(Self::Px(n * 96.0 / 2.54)),
                "mm" => Some(Self::Px(n * 96.0 / 25.4)),
                "q" => Some(Self::Px(n * 96.0 / 101.6)),
                _ => None,
            };
        }
        if s.starts_with("calc(")
            || s.starts_with("min(")
            || s.starts_with("max(")
            || s.starts_with("clamp(")
        {
            return calc_length(s);
        }
        None
    }

    pub fn resolve(&self, parent_len: f64, font_size: f64, root_font_size: f64) -> f64 {
        let value = match *self {
            Self::Px(v) => v,
            Self::Percent(pct) => parent_len * (pct / 100.0),
            Self::Em(em) => em * font_size,
            Self::Rem(rem) => rem * root_font_size,
            Self::Calc(v) => {
                v[0] + v[1] * parent_len / 100.0 + v[2] * font_size + v[3] * root_font_size
            }
            Self::Clamp([lo, value, hi]) => {
                let linear = |v: [f32; 4]| {
                    let v = v.map(f64::from);
                    v[0] + v[1] * parent_len / 100.0 + v[2] * font_size + v[3] * root_font_size
                };
                linear(value).min(linear(hi)).max(linear(lo))
            }
            Self::Vw(_) | Self::Vh(_) => 0.0, // converted to px before layout
            Self::Auto => 0.0,
        };
        if value.is_nan() {
            0.0
        } else {
            value.clamp(-16_777_216.0, 16_777_216.0)
        }
    }
}

thread_local! {
    /// Viewport of the style pass in progress, for viewport-relative terms
    /// inside `calc()`/`clamp()` and font sizes.
    static VIEWPORT: std::cell::Cell<(f64, f64)> = const { std::cell::Cell::new((800.0, 600.0)) };
}

pub fn viewport() -> (f64, f64) {
    VIEWPORT.with(std::cell::Cell::get)
}

/// Resolve `vw`/`vh` against the current style pass's viewport.
pub fn viewport_px(l: Length) -> Length {
    let (w, h) = viewport();
    match l {
        Length::Vw(v) => Length::Px(v * w / 100.0),
        Length::Vh(v) => Length::Px(v * h / 100.0),
        l => l,
    }
}

/// Set the viewport for the style pass in progress.
pub fn set_viewport(width: f64, height: f64) {
    VIEWPORT.with(|v| v.set((width, height)));
}
