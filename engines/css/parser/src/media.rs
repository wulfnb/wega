//! Media queries.
use wega_css_values::*;

#[derive(Debug, Clone, PartialEq)]
pub struct MediaQuery {
    pub negated: bool,
    pub media_type: Option<MediaType>,
    pub features: Vec<MediaFeature>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    All,
    Screen,
    Print,
    Speech,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MediaFeature {
    MinWidth(Length),
    MaxWidth(Length),
    Width(Length),
    MinHeight(Length),
    MaxHeight(Length),
    Height(Length),
    Orientation(Orientation),
    MinResolution(f64),
    MaxResolution(f64),
    Resolution(f64),
    PrefersColorScheme(ColorScheme),
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorScheme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy)]
pub struct MediaContext {
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub device_pixel_ratio: f64,
    pub dark_mode: bool,
}

impl Default for MediaContext {
    fn default() -> Self {
        Self {
            viewport_width: 800.0,
            viewport_height: 600.0,
            device_pixel_ratio: 1.0,
            dark_mode: false,
        }
    }
}

impl MediaQuery {
    pub fn matches(&self, ctx: &MediaContext) -> bool {
        let type_matches = match self.media_type {
            None | Some(MediaType::All | MediaType::Screen) => true,
            Some(MediaType::Print | MediaType::Speech) => false,
        };
        if !type_matches {
            return self.negated;
        }
        for feature in &self.features {
            let feat_matches = match feature {
                MediaFeature::MinWidth(len) => {
                    let px = len.resolve(ctx.viewport_width, 16.0, 16.0);
                    ctx.viewport_width >= px
                }
                MediaFeature::MaxWidth(len) => {
                    let px = len.resolve(ctx.viewport_width, 16.0, 16.0);
                    ctx.viewport_width <= px
                }
                MediaFeature::Width(len) => {
                    let px = len.resolve(ctx.viewport_width, 16.0, 16.0);
                    (ctx.viewport_width - px).abs() < 0.5
                }
                MediaFeature::MinHeight(len) => {
                    let px = len.resolve(ctx.viewport_height, 16.0, 16.0);
                    ctx.viewport_height >= px
                }
                MediaFeature::MaxHeight(len) => {
                    let px = len.resolve(ctx.viewport_height, 16.0, 16.0);
                    ctx.viewport_height <= px
                }
                MediaFeature::Height(len) => {
                    let px = len.resolve(ctx.viewport_height, 16.0, 16.0);
                    (ctx.viewport_height - px).abs() < 0.5
                }
                MediaFeature::Orientation(o) => {
                    let current = if ctx.viewport_width > ctx.viewport_height {
                        Orientation::Landscape
                    } else {
                        Orientation::Portrait
                    };
                    *o == current
                }
                MediaFeature::MinResolution(dppx) => ctx.device_pixel_ratio >= *dppx,
                MediaFeature::MaxResolution(dppx) => ctx.device_pixel_ratio <= *dppx,
                MediaFeature::Resolution(dppx) => (ctx.device_pixel_ratio - *dppx).abs() < 0.01,
                MediaFeature::PrefersColorScheme(cs) => match cs {
                    ColorScheme::Light => !ctx.dark_mode,
                    ColorScheme::Dark => ctx.dark_mode,
                },
                MediaFeature::Unknown(_) => false,
            };
            if !feat_matches {
                return self.negated;
            }
        }
        !self.negated
    }
}

pub fn matches_media(queries: &[MediaQuery], ctx: &MediaContext) -> bool {
    if queries.is_empty() {
        return true;
    }
    queries.iter().any(|q| q.matches(ctx))
}

/// Index of the `)` closing the `(` at the start of `s`.
pub(crate) fn closing_paren(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Media Queries 4 range syntax: `width >= 48rem`, `40em < width <= 64em`.
pub(crate) fn media_range(feature: &str) -> Option<Vec<MediaFeature>> {
    // Split into operands and comparison operators.
    let mut parts: Vec<String> = vec![String::new()];
    let mut ops = Vec::new();
    let mut chars = feature.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, '<' | '>' | '=') {
            let mut op = c.to_string();
            if c != '=' && chars.peek() == Some(&'=') {
                op.push(chars.next()?);
            }
            ops.push(op);
            parts.push(String::new());
        } else {
            parts.last_mut()?.push(c);
        }
    }
    let parts: Vec<String> = parts
        .iter()
        .map(|p| p.trim().to_ascii_lowercase())
        .collect();
    if ops.is_empty() || ops.len() != parts.len() - 1 || ops.len() > 2 {
        return None;
    }
    let feature_at = parts.iter().position(|p| p == "width" || p == "height")?;
    let is_width = parts[feature_at] == "width";
    // Strict comparisons shift the bound by a hundredth of a pixel.
    let shifted = |l: Length, delta: f64| match l {
        Length::Px(v) => Length::Px(v + delta),
        Length::Em(v) => Length::Calc([delta, 0.0, v, 0.0]),
        Length::Rem(v) => Length::Calc([delta, 0.0, 0.0, v]),
        Length::Calc(mut v) => {
            v[0] += delta;
            Length::Calc(v)
        }
        other => other,
    };
    let mut out = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        // Normalize to `feature OP value`.
        let (value, op) = if i == feature_at {
            (&parts[i + 1], op.clone())
        } else if i + 1 == feature_at {
            let flipped = match op.as_str() {
                "<" => ">",
                "<=" => ">=",
                ">" => "<",
                ">=" => "<=",
                other => other,
            };
            (&parts[i], flipped.to_owned())
        } else {
            return None;
        };
        let length = Length::parse(value)?;
        let feature = match (op.as_str(), is_width) {
            (">=", true) => MediaFeature::MinWidth(length),
            (">", true) => MediaFeature::MinWidth(shifted(length, 0.01)),
            ("<=", true) => MediaFeature::MaxWidth(length),
            ("<", true) => MediaFeature::MaxWidth(shifted(length, -0.01)),
            ("=", true) => MediaFeature::Width(length),
            (">=", false) => MediaFeature::MinHeight(length),
            (">", false) => MediaFeature::MinHeight(shifted(length, 0.01)),
            ("<=", false) => MediaFeature::MaxHeight(length),
            ("<", false) => MediaFeature::MaxHeight(shifted(length, -0.01)),
            ("=", false) => MediaFeature::Height(length),
            _ => return None,
        };
        out.push(feature);
    }
    Some(out)
}

pub fn parse_media_query_list(s: &str) -> Vec<MediaQuery> {
    s.split(',')
        .filter_map(|part| {
            let p = part.trim();
            if p.is_empty() {
                return None;
            }
            let mut negated = false;
            let mut media_type = None;
            let mut features = Vec::new();

            let mut remaining = p;
            while let Some(start) = remaining.find('(') {
                let prefix = remaining[..start].trim();
                if !prefix.is_empty() {
                    for word in prefix.split_whitespace() {
                        let w = word.to_ascii_lowercase();
                        if w == "not" {
                            negated = true;
                        } else if w == "only" {
                        } else if w == "all" {
                            media_type = Some(MediaType::All);
                        } else if w == "screen" {
                            media_type = Some(MediaType::Screen);
                        } else if w == "print" {
                            media_type = Some(MediaType::Print);
                        } else if w == "speech" {
                            media_type = Some(MediaType::Speech);
                        } else if w != "and" {
                            features.push(MediaFeature::Unknown(w));
                        }
                    }
                }
                if let Some(end) = closing_paren(&remaining[start..]) {
                    let feat_str = &remaining[start + 1..start + end];
                    if let Some((name, val)) = feat_str.split_once(':') {
                        let name = name.trim().to_ascii_lowercase();
                        let val = val.trim();
                        let feat = match name.as_str() {
                            "min-width" => Length::parse(val).map(MediaFeature::MinWidth),
                            "max-width" => Length::parse(val).map(MediaFeature::MaxWidth),
                            "width" => Length::parse(val).map(MediaFeature::Width),
                            "min-height" => Length::parse(val).map(MediaFeature::MinHeight),
                            "max-height" => Length::parse(val).map(MediaFeature::MaxHeight),
                            "height" => Length::parse(val).map(MediaFeature::Height),
                            "orientation" => match val.to_ascii_lowercase().as_str() {
                                "landscape" => {
                                    Some(MediaFeature::Orientation(Orientation::Landscape))
                                }
                                "portrait" => {
                                    Some(MediaFeature::Orientation(Orientation::Portrait))
                                }
                                _ => None,
                            },
                            "prefers-color-scheme" => match val.to_ascii_lowercase().as_str() {
                                "dark" => Some(MediaFeature::PrefersColorScheme(ColorScheme::Dark)),
                                "light" => {
                                    Some(MediaFeature::PrefersColorScheme(ColorScheme::Light))
                                }
                                _ => None,
                            },
                            "min-resolution" | "max-resolution" | "resolution" => {
                                let num = if let Some(dppx) =
                                    val.strip_suffix("dppx").or_else(|| val.strip_suffix("x"))
                                {
                                    dppx.trim().parse::<f64>().ok()
                                } else if let Some(dpi) = val.strip_suffix("dpi") {
                                    dpi.trim().parse::<f64>().ok().map(|d| d / 96.0)
                                } else {
                                    val.parse::<f64>().ok()
                                };
                                num.map(|n| match name.as_str() {
                                    "min-resolution" => MediaFeature::MinResolution(n),
                                    "max-resolution" => MediaFeature::MaxResolution(n),
                                    _ => MediaFeature::Resolution(n),
                                })
                            }
                            _ => Some(MediaFeature::Unknown(name.clone())),
                        };
                        if let Some(f) = feat {
                            features.push(f);
                        } else {
                            features.push(MediaFeature::Unknown(name));
                        }
                    } else if let Some(range) = media_range(feat_str) {
                        features.extend(range);
                    } else {
                        features.push(MediaFeature::Unknown(feat_str.into()));
                    }
                    remaining = &remaining[start + end + 1..];
                } else {
                    features.push(MediaFeature::Unknown(remaining.into()));
                    break;
                }
            }
            let tail = remaining.trim();
            if !tail.is_empty() {
                for word in tail.split_whitespace() {
                    let w = word.to_ascii_lowercase();
                    if w == "not" {
                        negated = true;
                    } else if w == "only" {
                    } else if w == "all" {
                        media_type = Some(MediaType::All);
                    } else if w == "screen" {
                        media_type = Some(MediaType::Screen);
                    } else if w == "print" {
                        media_type = Some(MediaType::Print);
                    } else if w == "speech" {
                        media_type = Some(MediaType::Speech);
                    } else if w != "and" {
                        features.push(MediaFeature::Unknown(w));
                    }
                }
            }
            Some(MediaQuery {
                negated,
                media_type,
                features,
            })
        })
        .collect()
}
