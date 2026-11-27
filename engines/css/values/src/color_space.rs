//! Predefined `color()` spaces (CSS Color 4 §10): matrices, transfer functions and
//! relative `color(from …)` parsing. Child module of `color`.
use super::*;

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
pub(super) fn predefined_color(p: &mut cssparser::Parser<'_>) -> Result<Color, ()> {
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
