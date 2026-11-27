//! Transforms: `transform` (function list), `rotate`, `scale`, `translate`,
//! `transform-origin`.
//!
//! Ported from Stylo `values/specified/transform.rs`, `values/generics/transform.rs` and
//! `values/specified/angle.rs` (MPL-2.0). Math functions in arguments are not ported yet.
use super::{Specified, length_percentage};
use cssparser::{ParseError, Parser, ToCss, Token};

/// A failure while parsing: invalid, or a part not ported yet.
#[derive(Debug)]
enum Fail {
    Invalid,
    Unported,
}

type R<T> = Result<T, Fail>;

impl From<cssparser::BasicParseError> for Fail {
    fn from(_: cssparser::BasicParseError) -> Self {
        Fail::Invalid
    }
}

impl From<ParseError<()>> for Fail {
    fn from(_: ParseError<()>) -> Self {
        Fail::Invalid
    }
}

fn n(v: f32) -> String {
    v.to_css_string()
}

/// A literal number.
fn number(p: &mut Parser<'_>) -> R<f32> {
    match p.next()?.clone() {
        Token::Number { value, .. } => Ok(value),
        Token::Function(_) => Err(Fail::Unported),
        _ => Err(Fail::Invalid),
    }
}

/// `<number> | <percentage>` as a number (`50%` is `0.5`).
fn number_or_percentage(p: &mut Parser<'_>) -> R<f32> {
    match p.next()?.clone() {
        Token::Number { value, .. } => Ok(value),
        Token::Percentage { unit_value, .. } => Ok(unit_value),
        Token::Function(_) => Err(Fail::Unported),
        _ => Err(Fail::Invalid),
    }
}

/// An angle: value and lowercase unit; a unitless zero is `0deg` when `unitless_zero`.
fn angle(p: &mut Parser<'_>, unitless_zero: bool) -> R<(f32, String)> {
    match p.next()?.clone() {
        Token::Dimension { value, unit, .. } => {
            let unit = unit.to_ascii_lowercase();
            if ["deg", "grad", "rad", "turn"].contains(&unit.as_str()) {
                Ok((value, unit))
            } else {
                Err(Fail::Invalid)
            }
        }
        Token::Number { value, .. } if value == 0.0 && unitless_zero => Ok((0.0, "deg".into())),
        Token::Function(_) => Err(Fail::Unported),
        _ => Err(Fail::Invalid),
    }
}

fn angle_css((value, unit): &(f32, String)) -> String {
    format!("{}{unit}", n(*value))
}

/// One token's worth of value text, for the length parsers.
fn token_text(p: &mut Parser<'_>) -> R<String> {
    let start = p.position();
    if let Token::Function(_) = p.next()? {
        return Err(Fail::Unported);
    }
    Ok(p.slice_from(start).trim().to_owned())
}

/// A length-percentage (`percent`) or length, negative values allowed.
fn length(p: &mut Parser<'_>, percent: bool) -> R<String> {
    let text = token_text(p)?;
    match length_percentage(&text, &[], true, percent) {
        Specified::Parsed(Some(css)) => Ok(css),
        Specified::Parsed(None) => Err(Fail::Invalid),
        Specified::Unported => Err(Fail::Unported),
    }
}

/// A non-negative length.
fn non_negative_length(p: &mut Parser<'_>) -> R<String> {
    let css = length(p, false)?;
    if css.starts_with('-') {
        return Err(Fail::Invalid);
    }
    Ok(css)
}

/// Whether a serialized length-percentage is zero and not a percentage.
fn zero_no_percent(css: &str) -> bool {
    !css.ends_with('%')
        && css.trim_start_matches('-').starts_with('0')
        && css
            .trim_start_matches(['-', '0', '.'])
            .chars()
            .all(|c| c.is_ascii_alphabetic())
}

fn comma(p: &mut Parser<'_>) -> R<()> {
    p.expect_comma()?;
    Ok(())
}

fn numbers(p: &mut Parser<'_>, count: usize) -> R<Vec<String>> {
    let mut out = Vec::new();
    for i in 0..count {
        if i > 0 {
            comma(p)?;
        }
        out.push(n(number(p)?));
    }
    Ok(out)
}

/// One transform function, serialized as Stylo's `TransformOperation`.
fn function(name: &str, p: &mut Parser<'_>) -> R<String> {
    let optional_comma = |p: &mut Parser<'_>| p.try_parse(|p| p.expect_comma()).is_ok();
    Ok(match name {
        "matrix" => format!("matrix({})", numbers(p, 6)?.join(", ")),
        "matrix3d" => format!("matrix3d({})", numbers(p, 16)?.join(", ")),
        "translate" => {
            let x = length(p, true)?;
            if optional_comma(p) {
                let y = length(p, true)?;
                if zero_no_percent(&y) {
                    format!("translate({x})")
                } else {
                    format!("translate({x}, {y})")
                }
            } else {
                format!("translate({x})")
            }
        }
        "translatex" => format!("translateX({})", length(p, true)?),
        "translatey" => format!("translateY({})", length(p, true)?),
        "translatez" => format!("translateZ({})", length(p, false)?),
        "translate3d" => {
            let x = length(p, true)?;
            comma(p)?;
            let y = length(p, true)?;
            comma(p)?;
            let z = length(p, false)?;
            format!("translate3d({x}, {y}, {z})")
        }
        "scale" => {
            let x = number_or_percentage(p)?;
            let y = if optional_comma(p) {
                number_or_percentage(p)?
            } else {
                x
            };
            if x == y {
                format!("scale({})", n(x))
            } else {
                format!("scale({}, {})", n(x), n(y))
            }
        }
        "scalex" => format!("scaleX({})", n(number_or_percentage(p)?)),
        "scaley" => format!("scaleY({})", n(number_or_percentage(p)?)),
        "scalez" => format!("scaleZ({})", n(number_or_percentage(p)?)),
        "scale3d" => {
            let x = number_or_percentage(p)?;
            comma(p)?;
            let y = number_or_percentage(p)?;
            comma(p)?;
            let z = number_or_percentage(p)?;
            format!("scale3d({}, {}, {})", n(x), n(y), n(z))
        }
        "rotate" => format!("rotate({})", angle_css(&angle(p, true)?)),
        "rotatex" => format!("rotateX({})", angle_css(&angle(p, true)?)),
        "rotatey" => format!("rotateY({})", angle_css(&angle(p, true)?)),
        "rotatez" => format!("rotateZ({})", angle_css(&angle(p, true)?)),
        "rotate3d" => {
            let axis = numbers(p, 3)?;
            comma(p)?;
            format!(
                "rotate3d({}, {})",
                axis.join(", "),
                angle_css(&angle(p, true)?)
            )
        }
        "skew" => {
            let x = angle(p, true)?;
            if optional_comma(p) {
                let y = angle(p, true)?;
                if y.0 == 0.0 {
                    format!("skew({})", angle_css(&x))
                } else {
                    format!("skew({}, {})", angle_css(&x), angle_css(&y))
                }
            } else {
                format!("skew({})", angle_css(&x))
            }
        }
        "skewx" => format!("skewX({})", angle_css(&angle(p, true)?)),
        "skewy" => format!("skewY({})", angle_css(&angle(p, true)?)),
        "perspective" => {
            if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
                "perspective(none)".into()
            } else {
                format!("perspective({})", non_negative_length(p)?)
            }
        }
        _ => return Err(Fail::Invalid),
    })
}

fn finish(result: R<String>) -> Specified {
    match result {
        Ok(css) => Specified::Parsed(Some(css)),
        Err(Fail::Invalid) => Specified::Parsed(None),
        Err(Fail::Unported) => Specified::Unported,
    }
}

/// Run `f` over the whole value; leftover input is invalid.
fn entirely(value: &str, f: impl FnOnce(&mut Parser<'_>) -> R<String>) -> Specified {
    let mut parser = Parser::new(value);
    let result = f(&mut parser).and_then(|css| {
        if parser.is_exhausted() {
            Ok(css)
        } else {
            Err(Fail::Invalid)
        }
    });
    finish(result)
}

/// `transform`: `none` or a space-separated list of transform functions.
pub fn transform(value: &str) -> Specified {
    entirely(value, |p| {
        if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
            return Ok("none".into());
        }
        let mut out = Vec::new();
        while !p.is_exhausted() {
            let name = match p.next()?.clone() {
                Token::Function(name) => name.to_ascii_lowercase(),
                _ => return Err(Fail::Invalid),
            };
            let mut inner = Err(Fail::Invalid);
            p.parse_nested_block(|p| {
                inner = function(&name, p).and_then(|css| {
                    if p.is_exhausted() {
                        Ok(css)
                    } else {
                        Err(Fail::Invalid)
                    }
                });
                while p.next().is_ok() {}
                Ok::<_, ParseError<()>>(())
            })?;
            out.push(inner?);
        }
        if out.is_empty() {
            return Err(Fail::Invalid);
        }
        Ok(out.join(" "))
    })
}

/// `rotate`: `none`, an angle, or an axis (`x`, `y`, `z` or three numbers) and an angle in
/// either order, serialized as Stylo does (axis-aligned axes as keywords or a bare angle).
pub fn rotate(value: &str) -> Specified {
    entirely(value, |p| {
        if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
            return Ok("none".into());
        }
        let first =
            p.try_parse(|p| angle(p, false).map_err(|_| p.new_error_for_next_token::<()>()));
        let axis = p
            .try_parse(|p| {
                let ident = p.expect_ident()?.to_ascii_lowercase();
                match ident.as_str() {
                    "x" => Ok([1.0, 0.0, 0.0]),
                    "y" => Ok([0.0, 1.0, 0.0]),
                    "z" => Ok([0.0, 0.0, 1.0]),
                    _ => Err(p.new_error_for_next_token::<()>()),
                }
            })
            .or_else(|_| {
                p.try_parse(|p| {
                    let mut v = [0.0; 3];
                    for slot in &mut v {
                        *slot = number(p).map_err(|_| p.new_error_for_next_token::<()>())?;
                    }
                    Ok::<_, ParseError<()>>(v)
                })
            })
            .ok();
        let (value, unit) = match first {
            Ok(a) => a,
            Err(_) => angle(p, false)?,
        };
        let Some([x, y, z]) = axis else {
            return Ok(angle_css(&(value, unit)));
        };
        // `|a × b|² ≈ 0` with euclid's f32 epsilon, as Stylo's `is_parallel_to`.
        let parallel = |[ax, ay, az]: [f32; 3]| {
            let c = [y * az - z * ay, z * ax - x * az, x * ay - y * ax];
            (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).abs() <= 1e-6
        };
        let signed =
            |negative: bool| angle_css(&(if negative { -value } else { value }, unit.clone()));
        Ok(if x == 0.0 && y == 0.0 && z == 0.0 {
            format!("{} {} {} {}", n(x), n(y), n(z), signed(false))
        } else if parallel([1.0, 0.0, 0.0]) {
            format!("x {}", signed(x < 0.0))
        } else if parallel([0.0, 1.0, 0.0]) {
            format!("y {}", signed(y < 0.0))
        } else if parallel([0.0, 0.0, 1.0]) {
            signed(z < 0.0)
        } else {
            format!("{} {} {} {}", n(x), n(y), n(z), signed(false))
        })
    })
}

/// `scale`: `none` or one to three numbers or percentages (as numbers); `y` is written when it
/// differs from `x` or `z` is not 1, `z` when it is not 1.
pub fn scale(value: &str) -> Specified {
    entirely(value, |p| {
        if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
            return Ok("none".into());
        }
        let factor = |p: &mut Parser<'_>| {
            p.try_parse(|p| number_or_percentage(p).map_err(|_| p.new_error_for_next_token::<()>()))
        };
        let x = number_or_percentage(p)?;
        let (y, z) = match factor(p) {
            Ok(y) => (y, factor(p).unwrap_or(1.0)),
            Err(_) => (x, 1.0),
        };
        let mut out = n(x);
        if z != 1.0 || x != y {
            out.push(' ');
            out.push_str(&n(y));
        }
        if z != 1.0 {
            out.push(' ');
            out.push_str(&n(z));
        }
        Ok(out)
    })
}

/// `translate`: `none` or `<length-percentage> [<length-percentage> <length>?]?`; trailing
/// zeros are omitted (a zero percentage `y` is kept).
pub fn translate(value: &str) -> Specified {
    entirely(value, |p| {
        if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
            return Ok("none".into());
        }
        let x = length(p, true)?;
        let y = p
            .try_parse(|p| length(p, true).map_err(|_| p.new_error_for_next_token::<()>()))
            .ok();
        let z = y.as_ref().and_then(|_| {
            p.try_parse(|p| length(p, false).map_err(|_| p.new_error_for_next_token::<()>()))
                .ok()
        });
        let z_zero = z.as_deref().is_none_or(zero_no_percent);
        let y_zero = y.as_deref().is_none_or(zero_no_percent);
        let mut out = x;
        if !(y_zero && z_zero) {
            out.push(' ');
            out.push_str(y.as_deref().unwrap_or("0px"));
        }
        if !z_zero {
            out.push(' ');
            out.push_str(z.as_deref().unwrap_or("0px"));
        }
        Ok(out)
    })
}

/// Whether `value` has a function at the top level (math functions are not ported yet).
fn has_function(value: &str) -> bool {
    let mut parser = Parser::new(value);
    while let Ok(token) = parser.next() {
        if matches!(token, Token::Function(_)) {
            return true;
        }
    }
    false
}

/// [`rotate`], [`scale`], [`translate`] or [`transform_origin`], left to the fallback when the
/// value has a math function.
pub fn individual(value: &str, parse: fn(&str) -> Specified) -> Specified {
    if has_function(value) {
        return Specified::Unported;
    }
    parse(value)
}

/// `transform-origin`: an x component, a y component and a depth length (Stylo
/// `TransformOrigin`), written as all three.
pub fn transform_origin(value: &str) -> Specified {
    #[derive(Clone)]
    enum C {
        Center,
        Length(String),
        Side(&'static str),
    }
    fn css(c: &C) -> String {
        match c {
            C::Center => "center".into(),
            C::Length(l) => l.clone(),
            C::Side(s) => (*s).into(),
        }
    }
    fn word(p: &mut Parser<'_>, words: &[&'static str]) -> Option<&'static str> {
        p.try_parse(|p| {
            let ident = p.expect_ident()?.to_ascii_lowercase();
            words
                .iter()
                .find(|w| **w == ident)
                .copied()
                .ok_or_else(|| p.new_error_for_next_token::<()>())
        })
        .ok()
    }
    fn lp(p: &mut Parser<'_>, percent: bool) -> Option<String> {
        p.try_parse(|p| length(p, percent).map_err(|_| p.new_error_for_next_token::<()>()))
            .ok()
    }
    /// `OriginComponent`: `center`, a length-percentage, or a side keyword.
    fn component(p: &mut Parser<'_>, sides: &[&'static str]) -> Option<C> {
        if word(p, &["center"]).is_some() {
            return Some(C::Center);
        }
        if let Some(l) = lp(p, true) {
            return Some(C::Length(l));
        }
        word(p, sides).map(C::Side)
    }
    const H: [&str; 2] = ["left", "right"];
    const V: [&str; 2] = ["top", "bottom"];
    if has_function(value) {
        return Specified::Unported;
    }
    entirely(value, |p| {
        let depth = |p: &mut Parser<'_>| lp(p, false).unwrap_or_else(|| "0px".into());
        let (x, y, z) = match p
            .try_parse(|p| component(p, &H).ok_or_else(|| p.new_error_for_next_token::<()>()))
        {
            Ok(C::Center) => {
                if let Some(y) = component(p, &V) {
                    let z = depth(p);
                    (C::Center, y, z)
                } else if let Some(side) = word(p, &H) {
                    let z = depth(p);
                    (C::Side(side), C::Center, z)
                } else {
                    (C::Center, C::Center, "0px".into())
                }
            }
            Ok(x) => match component(p, &V) {
                Some(y) => {
                    let z = depth(p);
                    (x, y, z)
                }
                None => (x, C::Center, "0px".into()),
            },
            Err(_) => {
                let y = C::Side(word(p, &V).ok_or(Fail::Invalid)?);
                if let Some(side) = word(p, &H) {
                    let z = depth(p);
                    (C::Side(side), y, z)
                } else if word(p, &["center"]).is_some() {
                    let z = depth(p);
                    (C::Center, y, z)
                } else {
                    (C::Center, y, "0px".into())
                }
            }
        };
        Ok(format!("{} {} {z}", css(&x), css(&y)))
    })
}
