//! Colors: sRGB storage, named/hex/functional parsing, `color-mix()`, Lab/OKLab conversion.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };

    pub fn parse(s: &str) -> Option<Self> {
        let mut parser = cssparser::Parser::new(s.trim());
        parser
            .parse_entirely(|p| {
                Self::parse_token(p).map_err(|_| p.new_error_for_next_token::<()>())
            })
            .ok()
    }

    /// CSS Color 4 sRGB syntax: names, hex, `rgb()`/`hsl()`/`hwb()` in both
    /// legacy comma and modern space/slash forms. `currentcolor` and the
    /// wide-gamut functions are left to the caller.
    pub fn parse_css(p: &mut cssparser::Parser<'_>) -> Option<Self> {
        Self::parse_token(p).ok()
    }
    fn parse_token(p: &mut cssparser::Parser<'_>) -> Result<Self, ()> {
        use cssparser::Token;
        let rgba = |(r, g, b): (u8, u8, u8), a: f32| Self {
            r,
            g,
            b,
            a: (a.clamp(0.0, 1.0) * 255.0).round() as u8,
        };
        match p.next().map_err(|_| ())?.clone() {
            Token::Ident(name) => {
                if name.eq_ignore_ascii_case("transparent") {
                    return Ok(Self::TRANSPARENT);
                }
                cssparser::color::parse_named_color(&name.to_ascii_lowercase())
                    .map(|c| rgba(c, 1.0))
            }
            Token::Hash(h) | Token::IDHash(h) => cssparser::color::parse_hash_color(h.as_bytes())
                .map(|(r, g, b, a)| rgba((r, g, b), a)),
            Token::Function(name) => {
                let name = name.to_ascii_lowercase();
                p.parse_nested_block(|p| {
                    if name == "color-mix" {
                        color_mix(p)
                    } else {
                        color_function(&name, p)
                    }
                    .map_err(|_| p.new_error_for_next_token::<()>())
                })
                .map_err(|_| ())
            }
            _ => Err(()),
        }
    }
}

pub(crate) fn gamma(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

pub(crate) fn linear(v: f32) -> f32 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// OKLab to gamma-encoded sRGB (0..1, unclamped).
pub(crate) fn oklab_to_srgb(l: f32, a: f32, b: f32) -> [f32; 3] {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        gamma(4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_),
        gamma(-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_),
        gamma(-0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_),
    ]
}

/// CIE Lab (D50) to gamma-encoded sRGB via Bradford-adapted XYZ D65.
pub(crate) fn lab_to_srgb(l: f32, a: f32, b: f32) -> [f32; 3] {
    let fy = (l + 16.0) / 116.0;
    let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
    let inverse = |t: f32| {
        if t > 6.0 / 29.0 {
            t.powi(3)
        } else {
            3.0 * (6.0f32 / 29.0).powi(2) * (t - 4.0 / 29.0)
        }
    };
    let (x, y, z) = (0.964_22 * inverse(fx), inverse(fy), 0.825_21 * inverse(fz));
    // D50 -> D65
    let (x, y, z) = (
        0.955_576_6 * x - 0.023_039_3 * y + 0.063_163_6 * z,
        -0.028_289_5 * x + 1.009_941_6 * y + 0.021_007_7 * z,
        0.012_298_2 * x - 0.020_483 * y + 1.329_909_8 * z,
    );
    [
        gamma(3.240_97 * x - 1.537_383_2 * y - 0.498_610_76 * z),
        gamma(-0.969_243_6 * x + 1.875_967_5 * y + 0.041_555_06 * z),
        gamma(0.055_630_08 * x - 0.203_976_96 * y + 1.056_971_5 * z),
    ]
}

/// `color-mix(in <space>, <color> [p%], <color> [p%])`, mixed with
/// premultiplied alpha in linear-light sRGB (close to the OKLab results that
/// authoring tools such as Tailwind request for opacity variants).
pub(crate) fn color_mix(p: &mut cssparser::Parser<'_>) -> Result<Color, ()> {
    // The interpolation clause is optional here: Stylo's serialization omits
    // its default (`in oklab`) along with default 50% weights.
    if p.try_parse(|p| p.expect_ident_matching("in")).is_ok() {
        p.expect_ident().map_err(|_| ())?;
        // Optional hue interpolation method, e.g. `shorter hue`.
        let _ = p.try_parse(|p| {
            p.expect_ident()?;
            p.expect_ident_matching("hue")
        });
        p.expect_comma().map_err(|_| ())?;
    }
    let item = |p: &mut cssparser::Parser<'_>| -> Result<(Color, Option<f32>), ()> {
        let mut percent = p.try_parse(|p| p.expect_percentage()).ok();
        let color = Color::parse_token(p)?;
        if percent.is_none() {
            percent = p.try_parse(|p| p.expect_percentage()).ok();
        }
        Ok((color, percent))
    };
    let (a, pa) = item(p)?;
    p.expect_comma().map_err(|_| ())?;
    let (b, pb) = item(p)?;
    let (pa, pb) = match (pa, pb) {
        (Some(x), Some(y)) => (x, y),
        (Some(x), None) => (x, 1.0 - x),
        (None, Some(y)) => (1.0 - y, y),
        (None, None) => (0.5, 0.5),
    };
    let total = pa + pb;
    if total <= 0.0 {
        return Err(());
    }
    let (wa, wb) = (pa / total, pb / total);
    let alpha = |c: Color| c.a as f32 / 255.0;
    let mixed_alpha = alpha(a) * wa + alpha(b) * wb;
    let channel = |x: u8, y: u8| {
        let premultiplied =
            linear(x as f32 / 255.0) * alpha(a) * wa + linear(y as f32 / 255.0) * alpha(b) * wb;
        let v = if mixed_alpha > 0.0 {
            premultiplied / mixed_alpha
        } else {
            0.0
        };
        (gamma(v).clamp(0.0, 1.0) * 255.0).round() as u8
    };
    let scale = total.min(1.0);
    Ok(Color {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: ((mixed_alpha * scale).clamp(0.0, 1.0) * 255.0).round() as u8,
    })
}

/// One numeric channel: (value, was_percentage). `none` reads as zero.
pub(crate) fn channel(p: &mut cssparser::Parser<'_>) -> Result<(f32, bool), ()> {
    use cssparser::Token;
    match p.next().map_err(|_| ())?.clone() {
        Token::Number { value, .. } => Ok((value, false)),
        Token::Percentage { unit_value, .. } => Ok((unit_value, true)),
        Token::Dimension { value, unit, .. } => Ok((
            match unit.to_ascii_lowercase().as_str() {
                "deg" => value,
                "rad" => value.to_degrees(),
                "grad" => value * 0.9,
                "turn" => value * 360.0,
                _ => return Err(()),
            },
            false,
        )),
        Token::Ident(ref i) if i.eq_ignore_ascii_case("none") => Ok((0.0, false)),
        _ => Err(()),
    }
}

pub(crate) fn color_function(name: &str, p: &mut cssparser::Parser<'_>) -> Result<Color, ()> {
    let first = channel(p)?;
    let legacy = p.try_parse(|p| p.expect_comma()).is_ok();
    let second = channel(p)?;
    if legacy {
        p.expect_comma().map_err(|_| ())?;
    }
    let third = channel(p)?;
    let alpha = if legacy {
        p.try_parse(|p| p.expect_comma())
            .ok()
            .map(|_| channel(p))
            .transpose()?
    } else {
        p.try_parse(|p| p.expect_delim('/'))
            .ok()
            .map(|_| channel(p))
            .transpose()?
    }
    .map_or(1.0, |(v, _)| v);
    if !p.is_exhausted() {
        return Err(());
    }
    let unit = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (r, g, b) = match name {
        "rgb" | "rgba" => {
            let c = |(v, percent): (f32, bool)| {
                if percent {
                    unit(v)
                } else {
                    v.clamp(0.0, 255.0).round() as u8
                }
            };
            (c(first), c(second), c(third))
        }
        "hsl" | "hsla" => {
            let fraction = |(v, percent): (f32, bool)| if percent { v } else { v / 100.0 };
            let (h, s, l) = (first.0.rem_euclid(360.0), fraction(second), fraction(third));
            let (s, l) = (s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
            let f = |n: f32| {
                let k = (n + h / 30.0) % 12.0;
                let a = s * l.min(1.0 - l);
                l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
            };
            (unit(f(0.0)), unit(f(8.0)), unit(f(4.0)))
        }
        "hwb" => {
            let fraction = |(v, percent): (f32, bool)| if percent { v } else { v / 100.0 };
            let (h, mut w, mut bl) = (first.0.rem_euclid(360.0), fraction(second), fraction(third));
            if w + bl >= 1.0 {
                let gray = w / (w + bl);
                (unit(gray), unit(gray), unit(gray))
            } else {
                w = w.max(0.0);
                bl = bl.max(0.0);
                let f = |n: f32| {
                    let k = (n + h / 30.0) % 12.0;
                    let base = 0.5 - 0.5 * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0);
                    base * (1.0 - w - bl) + w
                };
                (unit(f(0.0)), unit(f(8.0)), unit(f(4.0)))
            }
        }
        "oklab" | "oklch" | "lab" | "lch" => {
            let lightness = if name.starts_with("ok") {
                // OKLab L is 0..1; percentages map 100% to 1.
                first.0
            } else if first.1 {
                first.0 * 100.0
            } else {
                first.0
            };
            let scale = match name {
                "oklab" | "oklch" => 0.4,
                "lab" => 125.0,
                _ => 150.0,
            };
            let axis = |(v, percent): (f32, bool)| if percent { v * scale } else { v };
            let (a, b) = if name.ends_with("ch") {
                let (chroma, hue) = (axis(second).max(0.0), third.0.to_radians());
                (chroma * hue.cos(), chroma * hue.sin())
            } else {
                (axis(second), axis(third))
            };
            let [r, g, b] = if name.starts_with("ok") {
                oklab_to_srgb(lightness, a, b)
            } else {
                lab_to_srgb(lightness, a, b)
            };
            (unit(r), unit(g), unit(b))
        }
        _ => return Err(()),
    };
    Ok(Color {
        r,
        g,
        b,
        a: unit(alpha),
    })
}
