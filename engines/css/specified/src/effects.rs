//! Shadows and filters: `box-shadow`, `text-shadow`, `filter`, `backdrop-filter`.
//!
//! Ported from Stylo `values/{specified,generics}/effects.rs` (MPL-2.0), Servo build (no
//! `url()` filters). Math functions and color functions are not ported yet.
use super::{Specified, color, components, length_percentage, single_token};
use cssparser::{Parser, ToCss, Token};

/// A component that is a length (`Ok(Some)`), not one (`Ok(None)`), or unported (`Err`).
fn length(part: &str) -> Result<Option<String>, ()> {
    match length_percentage(part, &[], true, false) {
        Specified::Parsed(l) => Ok(l),
        Specified::Unported => match single_token(part) {
            // An identifier is no length (it may be a color).
            Some(Token::Ident(_)) | Some(Token::Hash(_)) | Some(Token::IDHash(_)) => Ok(None),
            _ => Err(()),
        },
    }
}

/// A component that is a color, not one, or unported.
fn color_part(part: &str) -> Result<Option<String>, ()> {
    match color(part) {
        Specified::Parsed(c) => Ok(c),
        Specified::Unported => match single_token(part) {
            Some(Token::Function(_)) => Err(()),
            _ => Ok(None),
        },
    }
}

/// One shadow: `(color, x, y, blur, spread, inset)`, parsed in Stylo's order. `box_shadow`
/// allows `inset` and a spread.
fn shadow(value: &str, box_shadow: bool) -> Specified {
    let Some(parts) = components(value) else {
        return Specified::Parsed(None);
    };
    let mut i = 0;
    let (mut lengths, mut color, mut inset) = (None::<Vec<String>>, None, false);
    let unported = || Specified::Unported;
    while let Some(part) = parts.get(i) {
        if box_shadow && !inset && part.eq_ignore_ascii_case("inset") {
            inset = true;
            i += 1;
            continue;
        }
        if lengths.is_none() {
            let mut found = Vec::new();
            let mut j = i;
            while found.len() < if box_shadow { 4 } else { 3 } {
                let Some(part) = parts.get(j) else { break };
                match length(part) {
                    Ok(Some(l)) => {
                        // The blur radius is non-negative; a negative one ends the lengths.
                        if found.len() == 2 && l.starts_with('-') {
                            break;
                        }
                        found.push(l);
                        j += 1;
                    }
                    Ok(None) => break,
                    Err(()) => return unported(),
                }
            }
            if found.len() >= 2 {
                lengths = Some(found);
                i = j;
                continue;
            }
        }
        if color.is_none() {
            match color_part(part) {
                Ok(Some(c)) => {
                    color = Some(c);
                    i += 1;
                    continue;
                }
                Ok(None) => {}
                Err(()) => return unported(),
            }
        }
        break;
    }
    if i != parts.len() {
        return Specified::Parsed(None);
    }
    let Some(lengths) = lengths else {
        return Specified::Parsed(None);
    };
    let mut out: Vec<String> = color.into_iter().collect();
    out.extend(lengths);
    if inset {
        out.push("inset".into());
    }
    Specified::Parsed(Some(out.join(" ")))
}

/// `box-shadow` (`box_shadow`) or `text-shadow`: `none` or a comma-separated list of shadows.
pub fn shadows(value: &str, box_shadow: bool) -> Specified {
    if matches!(single_token(value), Some(Token::Ident(i)) if i.eq_ignore_ascii_case("none")) {
        return Specified::Parsed(Some("none".into()));
    }
    super::list(true, value, |v| shadow(v, box_shadow))
}

/// `text-shadow`'s shadow inside `drop-shadow()`: color first or last, no `inset`/spread.
fn drop_shadow(args: &str) -> Specified {
    shadow(args, false)
}

/// A filter amount: a number or percentage, `>= 0`, clamped to 1 (100%) when `clamp`.
fn factor(args: &str, clamp: bool) -> Result<Option<String>, ()> {
    if args.trim().is_empty() {
        return Ok(Some("1".into()));
    }
    match single_token(args) {
        Some(Token::Number { value, .. }) if value >= 0.0 => Ok(Some(
            if clamp { value.min(1.0) } else { value }.to_css_string(),
        )),
        Some(Token::Percentage { unit_value, .. }) if unit_value >= 0.0 => {
            let v = if clamp {
                unit_value.min(1.0)
            } else {
                unit_value
            };
            Ok(Some(format!("{}%", (v * 100.0).to_css_string())))
        }
        Some(Token::Function(_)) => Err(()),
        _ => Ok(None),
    }
}

/// One filter function, or `Err` if unported.
fn filter_function(name: &str, args: &str) -> Result<Option<String>, ()> {
    let inner = match name {
        "blur" => {
            if args.trim().is_empty() {
                Some("0px".to_owned())
            } else {
                match length_percentage(args, &[], false, false) {
                    Specified::Parsed(l) => l,
                    Specified::Unported => return Err(()),
                }
            }
        }
        "brightness" | "contrast" | "saturate" => factor(args, false)?,
        "grayscale" | "invert" | "opacity" | "sepia" => factor(args, true)?,
        "hue-rotate" => {
            if args.trim().is_empty() {
                Some("0deg".to_owned())
            } else {
                match single_token(args) {
                    Some(Token::Dimension { value, unit, .. })
                        if ["deg", "grad", "rad", "turn"]
                            .contains(&unit.to_ascii_lowercase().as_str()) =>
                    {
                        Some(format!(
                            "{}{}",
                            value.to_css_string(),
                            unit.to_ascii_lowercase()
                        ))
                    }
                    Some(Token::Number { value: 0.0, .. }) => Some("0deg".into()),
                    Some(Token::Function(_)) => return Err(()),
                    _ => None,
                }
            }
        }
        "drop-shadow" => match drop_shadow(args) {
            Specified::Parsed(s) => s,
            Specified::Unported => return Err(()),
        },
        _ => None,
    };
    Ok(inner.map(|inner| format!("{name}({inner})")))
}

/// `filter`, `backdrop-filter`: `none` or a space-separated list of filter functions.
pub fn filters(value: &str) -> Specified {
    if matches!(single_token(value), Some(Token::Ident(i)) if i.eq_ignore_ascii_case("none")) {
        return Specified::Parsed(Some("none".into()));
    }
    let mut parser = Parser::new(value);
    let mut out = Vec::new();
    while !parser.is_exhausted() {
        let name = match parser.next() {
            Ok(Token::Function(name)) => name.to_ascii_lowercase(),
            _ => return Specified::Parsed(None),
        };
        let args = parser.parse_nested_block(|p| {
            let start = p.position();
            while p.next().is_ok() {}
            Ok::<_, cssparser::ParseError<()>>(p.slice_from(start).to_owned())
        });
        let Ok(args) = args else {
            return Specified::Parsed(None);
        };
        match filter_function(&name, &args) {
            Ok(Some(css)) => out.push(css),
            Ok(None) => return Specified::Parsed(None),
            Err(()) => return Specified::Unported,
        }
    }
    if out.is_empty() {
        return Specified::Parsed(None);
    }
    Specified::Parsed(Some(out.join(" ")))
}
