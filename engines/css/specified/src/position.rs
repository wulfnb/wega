//! `<position>` (`object-position`, `perspective-origin`): one to four components, the
//! three-value form rejected.
//!
//! Ported from Stylo `values/specified/position.rs` (`Position::parse_three_value_quirky`,
//! `ToCss for Position`, MPL-2.0): the same alternatives tried in the same order.
use super::{Specified, components, length_percentage};

#[derive(Clone, PartialEq)]
enum Component {
    Center,
    Length(String),
    /// A side keyword with an optional offset.
    Side(&'static str, Option<String>),
}

impl Component {
    fn count(&self) -> usize {
        match self {
            Component::Side(_, Some(_)) => 2,
            _ => 1,
        }
    }
    fn css(&self) -> String {
        match self {
            Component::Center => "center".into(),
            Component::Length(lp) => lp.clone(),
            Component::Side(side, None) => (*side).into(),
            Component::Side(side, Some(lp)) => format!("{side} {lp}"),
        }
    }
}

/// A token: a keyword, a length-percentage, or neither.
#[derive(Clone)]
enum Part {
    Word(String),
    Length(String),
    Other,
}

struct Cursor {
    parts: Vec<Part>,
    at: usize,
}

const H: [&str; 2] = ["left", "right"];
const V: [&str; 2] = ["top", "bottom"];

impl Cursor {
    fn word(&mut self, options: &[&'static str]) -> Option<&'static str> {
        let Some(Part::Word(w)) = self.parts.get(self.at) else {
            return None;
        };
        let found = options.iter().find(|o| w == *o).copied()?;
        self.at += 1;
        Some(found)
    }
    fn length(&mut self) -> Option<String> {
        let Some(Part::Length(l)) = self.parts.get(self.at) else {
            return None;
        };
        let l = l.clone();
        self.at += 1;
        Some(l)
    }
    /// `PositionComponent::parse` with side keywords `sides`.
    fn component(&mut self, sides: [&'static str; 2]) -> Option<Component> {
        if self.word(&["center"]).is_some() {
            return Some(Component::Center);
        }
        if let Some(l) = self.length() {
            return Some(Component::Length(l));
        }
        let side = self.word(&sides)?;
        Some(Component::Side(side, self.length()))
    }
    fn attempt<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        let start = self.at;
        let result = f(self);
        if result.is_none() {
            self.at = start;
        }
        result
    }
}

fn parse(c: &mut Cursor) -> Option<(Component, Component)> {
    use Component::*;
    match c.attempt(|c| c.component(H)) {
        Some(Center) => {
            if let Some(y) = c.attempt(|c| c.component(V)) {
                return Some((Center, y));
            }
            let x = c.attempt(|c| c.component(H)).unwrap_or(Center);
            return Some((x, Center));
        }
        Some(Side(xk, lp)) => {
            if c.word(&["center"]).is_some() {
                return Some((Side(xk, lp), Center));
            }
            if let Some(yk) = c.word(&V) {
                let ylp = c.length();
                return Some((Side(xk, lp), Side(yk, ylp)));
            }
            return Some((Side(xk, None), lp.map_or(Center, Length)));
        }
        Some(Length(x)) => {
            if let Some(yk) = c.word(&V) {
                return Some((Length(x), Side(yk, None)));
            }
            if let Some(y) = c.length() {
                return Some((Length(x), Length(y)));
            }
            let _ = c.word(&["center"]);
            return Some((Length(x), Center));
        }
        None => {}
    }
    let yk = c.word(&V)?;
    let rest = c.attempt(|c| {
        let ylp = c.length();
        if let Some(xk) = c.word(&H) {
            let xlp = c.length();
            return Some((ylp, Side(xk, xlp)));
        }
        c.word(&["center"])?;
        Some((ylp, Center))
    });
    Some(match rest {
        Some((ylp, x)) => (x, Side(yk, ylp)),
        None => (Center, Side(yk, None)),
    })
}

/// A `<position>`, serialized as Stylo does.
pub fn position(value: &str) -> Specified {
    let Some(raw) = components(value) else {
        return Specified::Parsed(None);
    };
    let mut parts = Vec::new();
    for part in &raw {
        let lower = part.to_ascii_lowercase();
        parts.push(
            if ["center", "left", "right", "top", "bottom"].contains(&lower.as_str()) {
                Part::Word(lower)
            } else if matches!(super::single_token(part), Some(cssparser::Token::Ident(_))) {
                Part::Other
            } else {
                match length_percentage(part, &[], true, true) {
                    Specified::Parsed(Some(l)) => Part::Length(l),
                    Specified::Parsed(None) => Part::Other,
                    Specified::Unported => return Specified::Unported,
                }
            },
        );
    }
    let mut cursor = Cursor { parts, at: 0 };
    let Some((x, y)) = parse(&mut cursor) else {
        return Specified::Parsed(None);
    };
    if cursor.at != cursor.parts.len() || x.count() != y.count() {
        return Specified::Parsed(None);
    }
    Specified::Parsed(Some(match (&x, &y) {
        (Component::Side(_, Some(_)), Component::Length(ylp)) => format!("{} top {ylp}", x.css()),
        (Component::Length(xlp), Component::Side(_, Some(_))) => format!("left {xlp} {}", y.css()),
        _ => format!("{} {}", x.css(), y.css()),
    }))
}
