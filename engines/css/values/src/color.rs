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
                    } else if name == "color" {
                        predefined_color(p)
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
    let [r, g, b, a] = color_mix_unrounded(p)?;
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Ok(Color {
        r: byte(r),
        g: byte(g),
        b: byte(b),
        a: byte(a),
    })
}

/// A per-channel transfer function.
type Transfer = fn(f32) -> f32;

/// `color-mix()` as unrounded gamma-encoded sRGB and alpha (0..1), so nested mixes are
/// rounded once.
fn color_mix_unrounded(p: &mut cssparser::Parser<'_>) -> Result<[f32; 4], ()> {
    // The interpolation clause is optional here: Stylo's serialization omits
    // its default (`in oklab`) along with default 50% weights.
    // `in srgb` mixes the gamma-encoded values (CSS Color 5 §2.1); other spaces use
    // linear-light sRGB, which approximates OKLab, the default.
    let mut encoded = false;
    if p.try_parse(|p| p.expect_ident_matching("in")).is_ok() {
        encoded = p
            .expect_ident()
            .map_err(|_| ())?
            .eq_ignore_ascii_case("srgb");
        // Optional hue interpolation method, e.g. `shorter hue`.
        let _ = p.try_parse(|p| {
            p.expect_ident()?;
            p.expect_ident_matching("hue")
        });
        p.expect_comma().map_err(|_| ())?;
    }
    let item = |p: &mut cssparser::Parser<'_>| -> Result<([f32; 4], Option<f32>), ()> {
        let mut percent = p.try_parse(|p| p.expect_percentage()).ok();
        let nested = p.try_parse(|p| {
            p.expect_function_matching("color-mix")?;
            p.parse_nested_block(|p| {
                color_mix_unrounded(p).map_err(|_| p.new_error_for_next_token::<()>())
            })
        });
        let color = match nested {
            Ok(c) => c,
            Err(_) => {
                let c = Color::parse_token(p)?;
                [c.r, c.g, c.b, c.a].map(|v| v as f32 / 255.0)
            }
        };
        if percent.is_none() {
            percent = p.try_parse(|p| p.expect_percentage()).ok();
        }
        Ok((color, percent))
    };
    let (a, pa) = item(p)?;
    p.expect_comma().map_err(|_| ())?;
    let (b, pb) = item(p)?;
    if !p.is_exhausted() {
        return Err(());
    }
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
    let mixed_alpha = a[3] * wa + b[3] * wb;
    let (decode, encode): (Transfer, Transfer) = if encoded {
        (|v| v, |v| v)
    } else {
        (linear, gamma)
    };
    let channel = |i: usize| {
        let premultiplied = decode(a[i]) * a[3] * wa + decode(b[i]) * b[3] * wb;
        let v = if mixed_alpha > 0.0 {
            premultiplied / mixed_alpha
        } else {
            0.0
        };
        encode(v).clamp(0.0, 1.0)
    };
    Ok([
        channel(0),
        channel(1),
        channel(2),
        mixed_alpha * total.min(1.0),
    ])
}

/// One color function argument, typed so each function can apply its grammar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Channel {
    Number(f32),
    /// A percentage as a fraction (`50%` is `0.5`).
    Percent(f32),
    /// An angle in degrees.
    Angle(f32),
    None,
}

impl Channel {
    fn is_number_or_percent(self) -> bool {
        matches!(self, Channel::Number(_) | Channel::Percent(_))
    }
    fn is_hue(self) -> bool {
        matches!(self, Channel::Number(_) | Channel::Angle(_))
    }
    /// `(value, was_percentage)`; `none` reads as zero (CSS Color 4 §4.4).
    fn value(self) -> (f32, bool) {
        match self {
            Channel::Number(v) | Channel::Angle(v) => (v, false),
            Channel::Percent(v) => (v, true),
            Channel::None => (0.0, false),
        }
    }
}

/// One color function argument: a number, percentage, angle, `none`, or a math
/// function resolving to a number or a percentage.
pub(crate) fn channel(p: &mut cssparser::Parser<'_>) -> Result<Channel, ()> {
    use cssparser::Token;
    match p.next().map_err(|_| ())?.clone() {
        Token::Number { value, .. } if value.is_finite() => Ok(Channel::Number(value)),
        Token::Percentage { unit_value, .. } if unit_value.is_finite() => {
            Ok(Channel::Percent(unit_value))
        }
        Token::Dimension { value, unit, .. } if value.is_finite() => {
            angle_degrees(value, &unit).map(Channel::Angle).ok_or(())
        }
        Token::Ident(ref i) if i.eq_ignore_ascii_case("none") => Ok(Channel::None),
        Token::Function(ref f)
            if ["calc", "min", "max", "clamp"]
                .iter()
                .any(|m| f.eq_ignore_ascii_case(m)) =>
        {
            let name = f.to_ascii_lowercase();
            let quantity = p
                .parse_nested_block(|p| {
                    let one = |p: &mut cssparser::Parser<'_>| crate::calc::expression(p, 0);
                    if name == "calc" {
                        return one(p);
                    }
                    let args = p.parse_comma_separated(one)?;
                    math_fold(&name, &args).ok_or_else(|| p.new_error_for_next_token::<()>())
                })
                .map_err(|_| ())?;
            match quantity {
                crate::calc::Quantity::Number(n) => Ok(Channel::Number(n as f32)),
                crate::calc::Quantity::Length([0.0, percent, 0.0, 0.0]) => {
                    Ok(Channel::Percent(percent as f32 / 100.0))
                }
                _ => Err(()),
            }
        }
        _ => Err(()),
    }
}

/// `min()`/`max()`/`clamp()` over numbers or over pure percentages.
fn math_fold(name: &str, args: &[crate::calc::Quantity]) -> Option<crate::calc::Quantity> {
    use crate::calc::Quantity;
    let numbers: Option<Vec<f64>> = args
        .iter()
        .map(|q| match q {
            Quantity::Number(n) => Some(*n),
            _ => None,
        })
        .collect();
    let percents: Option<Vec<f64>> = args
        .iter()
        .map(|q| match q {
            Quantity::Length([0.0, p, 0.0, 0.0]) => Some(*p),
            _ => None,
        })
        .collect();
    let fold = |v: &[f64]| -> Option<f64> {
        match (name, v) {
            ("min", [_, ..]) => Some(v.iter().copied().fold(f64::INFINITY, f64::min)),
            ("max", [_, ..]) => Some(v.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
            ("clamp", [lo, x, hi]) => Some(x.min(*hi).max(*lo)),
            _ => None,
        }
    };
    if let Some(n) = numbers {
        fold(&n).map(Quantity::Number)
    } else {
        percents.and_then(|p| fold(&p).map(|v| Quantity::Length([0.0, v, 0.0, 0.0])))
    }
}

/// An angle in degrees, for the CSS Values 4 angle units only.
pub fn angle_degrees(value: f32, unit: &str) -> Option<f32> {
    Some(match unit.to_ascii_lowercase().as_str() {
        "deg" => value,
        "rad" => value.to_degrees(),
        "grad" => value * 0.9,
        "turn" => value * 360.0,
        _ => return None,
    })
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
    };
    if !p.is_exhausted() {
        return Err(());
    }
    // CSS Color 4 grammar for each function's arguments.
    let valid = match (name, legacy) {
        // Legacy rgb(): all numbers or all percentages; no `none`.
        ("rgb" | "rgba", true) => {
            matches!(
                (first, second, third),
                (Channel::Number(_), Channel::Number(_), Channel::Number(_))
                    | (
                        Channel::Percent(_),
                        Channel::Percent(_),
                        Channel::Percent(_)
                    )
            )
        }
        ("rgb" | "rgba", false) => [first, second, third]
            .iter()
            .all(|c| c.is_number_or_percent() || *c == Channel::None),
        // Legacy hsl(): a hue, then two percentages; no `none`.
        ("hsl" | "hsla", true) => {
            first.is_hue()
                && matches!(second, Channel::Percent(_))
                && matches!(third, Channel::Percent(_))
        }
        ("hsl" | "hsla" | "hwb", false) => {
            (first.is_hue() || first == Channel::None)
                && [second, third]
                    .iter()
                    .all(|c| c.is_number_or_percent() || *c == Channel::None)
        }
        ("lab" | "oklab", false) => [first, second, third]
            .iter()
            .all(|c| c.is_number_or_percent() || *c == Channel::None),
        ("lch" | "oklch", false) => {
            [first, second]
                .iter()
                .all(|c| c.is_number_or_percent() || *c == Channel::None)
                && (third.is_hue() || third == Channel::None)
        }
        // hwb(), lab() and the rest have no legacy comma syntax.
        _ => false,
    } && match alpha {
        None => true,
        Some(a) if legacy => a.is_number_or_percent(),
        Some(a) => a.is_number_or_percent() || a == Channel::None,
    };
    if !valid {
        return Err(());
    }
    let (first, second, third) = (first.value(), second.value(), third.value());
    let alpha = alpha.map_or(1.0, |a| a.value().0);
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
            // Saturation and lightness are not clamped (CSS Color 4 §7.1); the result is
            // gamut-clipped per channel below. Negative saturation means none.
            let s = s.max(0.0);
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

/// A predefined RGB or XYZ color space of `color()` (CSS Color 4 §10).
#[derive(Clone, Copy, PartialEq)]
enum Space {
    Srgb,
    SrgbLinear,
    DisplayP3,
    A98Rgb,
    ProphotoRgb,
    Rec2020,
    XyzD50,
    XyzD65,
}

type Matrix = [[f64; 3]; 3];

fn multiply(m: &Matrix, v: [f64; 3]) -> [f64; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

const LIN_SRGB_TO_XYZ: Matrix = [
    [0.41239079926595934, 0.357584339383878, 0.1804807884018343],
    [0.21263900587151027, 0.715168678767756, 0.07219231536073371],
    [0.01933081871559182, 0.11919477979462598, 0.9505321522496607],
];
const XYZ_TO_LIN_SRGB: Matrix = [
    [3.2409699419045226, -1.537383177570094, -0.4986107602930034],
    [-0.9692436362808796, 1.8759675015077202, 0.04155505740717559],
    [
        0.05563007969699366,
        -0.20397695888897652,
        1.0569715142428786,
    ],
];
const LIN_P3_TO_XYZ: Matrix = [
    [0.4865709486482162, 0.26566769316909306, 0.1982172852343625],
    [0.2289745640697488, 0.6917385218365064, 0.079286914093745],
    [0.0, 0.04511338185890264, 1.043944368900976],
];
const XYZ_TO_LIN_P3: Matrix = [
    [2.493496911941425, -0.9313836179191239, -0.40271078445071684],
    [
        -0.8294889695615747,
        1.7626640603183463,
        0.023624685841943577,
    ],
    [
        0.03584583024378447,
        -0.07617238926804182,
        0.9568845240076872,
    ],
];
const LIN_A98_TO_XYZ: Matrix = [
    [0.5766690429101305, 0.1855582379065463, 0.1882286462349947],
    [0.29734497525053605, 0.6273635662554661, 0.07529145849399788],
    [0.02703136138641234, 0.07068885253582723, 0.9913375368376388],
];
const XYZ_TO_LIN_A98: Matrix = [
    [
        2.0415879038107465,
        -0.5650069742788596,
        -0.34473135077832956,
    ],
    [-0.9692436362808795, 1.8759675015077202, 0.04155505740717557],
    [
        0.013444280632031142,
        -0.11836239223101838,
        1.0151749943912054,
    ],
];
const LIN_PROPHOTO_TO_XYZ_D50: Matrix = [
    [0.7977666449006423, 0.13518129740053308, 0.0313477341283922],
    [0.2880748288194013, 0.711835234241873, 0.00008993693872564],
    [0.0, 0.0, 0.8251046025104602],
];
const XYZ_D50_TO_LIN_PROPHOTO: Matrix = [
    [
        1.3457868816471583,
        -0.25557208737979464,
        -0.05110186497554526,
    ],
    [-0.5446307051249019, 1.5082477428451468, 0.02052744743642139],
    [0.0, 0.0, 1.2119675456389452],
];
const LIN_2020_TO_XYZ: Matrix = [
    [0.6369580483012914, 0.14461690358620832, 0.16888097516417208],
    [
        0.26270021201126714,
        0.6779980715188708,
        0.059301716469861966,
    ],
    [0.0, 0.028072693049087428, 1.060985057710791],
];
const XYZ_TO_LIN_2020: Matrix = [
    [
        1.7166511879712674,
        -0.35567078377639233,
        -0.25336628137365974,
    ],
    [-0.6666843518324892, 1.6164812366349395, 0.01576854581391113],
    [
        0.017639857445310783,
        -0.042770613257808524,
        0.9421031212354738,
    ],
];
/// Bradford chromatic adaptation.
const D50_TO_D65: Matrix = [
    [0.955473421488075, -0.02309845494876471, 0.06325924320057072],
    [
        -0.0283697093338637,
        1.0099953980813041,
        0.021041441191917323,
    ],
    [
        0.012314014864481998,
        -0.020507649298898964,
        1.330365926242124,
    ],
];
const D65_TO_D50: Matrix = [
    [
        1.0479297925449969,
        0.022946870601609652,
        -0.05019226628920524,
    ],
    [
        0.02962780877005599,
        0.9904344267538799,
        -0.017073799063418826,
    ],
    [
        -0.009243040646204504,
        0.015055191490298152,
        0.7518742814281371,
    ],
];

fn srgb_decode(v: f64) -> f64 {
    let a = v.abs();
    v.signum()
        * if a <= 0.04045 {
            a / 12.92
        } else {
            ((a + 0.055) / 1.055).powf(2.4)
        }
}
fn srgb_encode(v: f64) -> f64 {
    let a = v.abs();
    v.signum()
        * if a <= 0.0031308 {
            12.92 * a
        } else {
            1.055 * a.powf(1.0 / 2.4) - 0.055
        }
}
fn a98_decode(v: f64) -> f64 {
    v.signum() * v.abs().powf(563.0 / 256.0)
}
fn a98_encode(v: f64) -> f64 {
    v.signum() * v.abs().powf(256.0 / 563.0)
}
fn prophoto_decode(v: f64) -> f64 {
    let a = v.abs();
    v.signum()
        * if a <= 16.0 / 512.0 {
            a / 16.0
        } else {
            a.powf(1.8)
        }
}
fn prophoto_encode(v: f64) -> f64 {
    let a = v.abs();
    v.signum()
        * if a >= 1.0 / 512.0 {
            a.powf(1.0 / 1.8)
        } else {
            16.0 * a
        }
}
/// `rec2020` uses the BT.1886 display transfer, a pure 2.4 gamma (CSS Color 4 §10.7).
fn rec2020_decode(v: f64) -> f64 {
    v.signum() * v.abs().powf(2.4)
}
fn rec2020_encode(v: f64) -> f64 {
    v.signum() * v.abs().powf(1.0 / 2.4)
}

impl Space {
    fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "srgb" => Self::Srgb,
            "srgb-linear" => Self::SrgbLinear,
            "display-p3" => Self::DisplayP3,
            "a98-rgb" => Self::A98Rgb,
            "prophoto-rgb" => Self::ProphotoRgb,
            "rec2020" => Self::Rec2020,
            "xyz-d50" => Self::XyzD50,
            "xyz" | "xyz-d65" => Self::XyzD65,
            _ => return None,
        })
    }
    fn is_xyz(self) -> bool {
        matches!(self, Self::XyzD50 | Self::XyzD65)
    }
    /// Encoded components in this space to CIE XYZ (D65).
    fn to_xyz(self, c: [f64; 3]) -> [f64; 3] {
        match self {
            Self::Srgb => multiply(&LIN_SRGB_TO_XYZ, c.map(srgb_decode)),
            Self::SrgbLinear => multiply(&LIN_SRGB_TO_XYZ, c),
            Self::DisplayP3 => multiply(&LIN_P3_TO_XYZ, c.map(srgb_decode)),
            Self::A98Rgb => multiply(&LIN_A98_TO_XYZ, c.map(a98_decode)),
            Self::ProphotoRgb => multiply(
                &D50_TO_D65,
                multiply(&LIN_PROPHOTO_TO_XYZ_D50, c.map(prophoto_decode)),
            ),
            Self::Rec2020 => multiply(&LIN_2020_TO_XYZ, c.map(rec2020_decode)),
            Self::XyzD50 => multiply(&D50_TO_D65, c),
            Self::XyzD65 => c,
        }
    }
    /// CIE XYZ (D65) to encoded components in this space.
    fn encode_xyz(self, xyz: [f64; 3]) -> [f64; 3] {
        match self {
            Self::Srgb => multiply(&XYZ_TO_LIN_SRGB, xyz).map(srgb_encode),
            Self::SrgbLinear => multiply(&XYZ_TO_LIN_SRGB, xyz),
            Self::DisplayP3 => multiply(&XYZ_TO_LIN_P3, xyz).map(srgb_encode),
            Self::A98Rgb => multiply(&XYZ_TO_LIN_A98, xyz).map(a98_encode),
            Self::ProphotoRgb => {
                multiply(&XYZ_D50_TO_LIN_PROPHOTO, multiply(&D65_TO_D50, xyz)).map(prophoto_encode)
            }
            Self::Rec2020 => multiply(&XYZ_TO_LIN_2020, xyz).map(rec2020_encode),
            Self::XyzD50 => multiply(&D65_TO_D50, xyz),
            Self::XyzD65 => xyz,
        }
    }
}

/// `color([from <color>]? <colorspace> <c1> <c2> <c3> [/ <alpha>]?)`. In the relative
/// form the channel keywords (`r g b` or `x y z`, and `alpha`) stand for the origin
/// color's components in the target space. The result is gamut-clipped to sRGB.
fn predefined_color(p: &mut cssparser::Parser<'_>) -> Result<Color, ()> {
    let origin = if p.try_parse(|p| p.expect_ident_matching("from")).is_ok() {
        Some(Color::parse_token(p)?)
    } else {
        None
    };
    let space = Space::parse(&p.expect_ident().map_err(|_| ())?.clone()).ok_or(())?;
    let (names, alpha_name) = if space.is_xyz() {
        (["x", "y", "z"], "alpha")
    } else {
        (["r", "g", "b"], "alpha")
    };
    let origin_components = origin.map(|c| {
        let srgb = [c.r, c.g, c.b].map(|v| v as f64 / 255.0);
        (
            space.encode_xyz(Space::Srgb.to_xyz(srgb)),
            c.a as f64 / 255.0,
        )
    });
    // One component: a number, a percentage (of 1), `none`, or a channel keyword.
    let component = |p: &mut cssparser::Parser<'_>| -> Result<f64, ()> {
        let state = p.state();
        if let (Ok(ident), Some((values, alpha))) = (
            p.expect_ident().map(|i| i.to_ascii_lowercase()),
            origin_components,
        ) {
            if let Some(i) = names.iter().position(|n| *n == ident) {
                return Ok(values[i]);
            }
            if ident == alpha_name {
                return Ok(alpha);
            }
        }
        p.reset(&state);
        match channel(p)? {
            Channel::Number(v) => Ok(v as f64),
            Channel::Percent(v) => Ok(v as f64),
            Channel::None => Ok(0.0),
            Channel::Angle(_) => Err(()),
        }
    };
    let components = [component(p)?, component(p)?, component(p)?];
    let alpha = if p.try_parse(|p| p.expect_delim('/')).is_ok() {
        component(p)?
    } else {
        origin_components.map_or(1.0, |(_, a)| a)
    };
    if !p.is_exhausted() {
        return Err(());
    }
    let srgb = Space::Srgb.encode_xyz(space.to_xyz(components));
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Ok(Color {
        r: byte(srgb[0]),
        g: byte(srgb[1]),
        b: byte(srgb[2]),
        a: byte(alpha),
    })
}

#[cfg(test)]
mod tests {
    use super::Color;

    fn rgb(r: u8, g: u8, b: u8) -> Option<Color> {
        Some(Color { r, g, b, a: 255 })
    }

    #[test]
    fn legacy_rgb_rejects_mixed_numbers_and_percentages() {
        for invalid in [
            "rgb(255, 0, 0%)",
            "rgb(100%, 0%, 0)",
            "rgba(255, 1%, 0%, 1)",
            "rgb(255, none, 0)",
            "rgb(1deg, 0, 0)",
        ] {
            assert_eq!(Color::parse(invalid), None, "{invalid}");
        }
        assert_eq!(Color::parse("rgb(255, 0, 0)"), rgb(255, 0, 0));
        assert_eq!(Color::parse("rgb(100%, 0%, 0%)"), rgb(255, 0, 0));
        // The modern syntax may mix them, and accepts `none`.
        assert_eq!(Color::parse("rgb(255 0% none)"), rgb(255, 0, 0));
    }

    #[test]
    fn legacy_hsl_needs_a_hue_then_percentages() {
        for invalid in [
            "hsl(0, 255, 128)",
            "hsl(0%, 100%, 50%)",
            "hsl(0px, 100%, 50%)",
            "hsla(0, 100%, 50, 1)",
        ] {
            assert_eq!(Color::parse(invalid), None, "{invalid}");
        }
        assert_eq!(Color::parse("hsl(120, 100%, 25%)"), rgb(0, 128, 0));
        assert_eq!(Color::parse("hsl(120deg 100 25)"), rgb(0, 128, 0));
        assert_eq!(Color::parse("hwb(0, 0%, 0%)"), None);
    }

    #[test]
    fn channels_accept_math_functions() {
        assert_eq!(
            Color::parse("rgb(calc(5% + 15%), calc(40% / 2), calc(5% * 4))"),
            rgb(51, 51, 51)
        );
        assert_eq!(Color::parse("rgb(calc(255), min(0, 9), 0)"), rgb(255, 0, 0));
        assert_eq!(Color::parse("rgb(calc(10px), 0, 0)"), None);
    }

    #[test]
    fn predefined_color_spaces_convert_to_srgb() {
        assert_eq!(Color::parse("color(srgb 1 0 0)"), rgb(255, 0, 0));
        assert_eq!(Color::parse("color(display-p3 0 1 0)"), rgb(0, 255, 0));
        assert_eq!(
            Color::parse("color(xyz 0.9505 1 1.089)"),
            rgb(255, 255, 255)
        );
        // Round trips through the relative syntax keep the origin color.
        for space in [
            "srgb",
            "display-p3",
            "a98-rgb",
            "prophoto-rgb",
            "rec2020",
            "xyz-d50",
        ] {
            let keywords = if space.starts_with("xyz") {
                "x y z"
            } else {
                "r g b"
            };
            assert_eq!(
                Color::parse(&format!("color(from rgb(10 120 200) {space} {keywords})")),
                rgb(10, 120, 200),
                "{space}"
            );
        }
        assert_eq!(
            Color::parse("color(rec2020 0.3281959 0.543352 0.1865422)"),
            Color::parse("color(prophoto-rgb 0.23 0.43 0.13)")
        );
        assert_eq!(Color::parse("color(nonsense 1 0 0)"), None);
        assert_eq!(Color::parse("color(srgb 1 0)"), None);
    }

    #[test]
    fn color_mix_in_srgb_mixes_encoded_values() {
        assert_eq!(
            Color::parse(
                "color-mix(in srgb, color-mix(in srgb, rgb(0 0 255) 50%, color(srgb 0 1 0)), white)"
            ),
            Color::parse("color(srgb 0.5 0.75 0.75)")
        );
    }
}
