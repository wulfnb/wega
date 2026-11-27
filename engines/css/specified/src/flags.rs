//! Keyword sets parsed as Stylo's `#[css(bitflags(...))]` types: one "single" keyword alone, or
//! one or more "mixed" keywords in any order, each at most once. Serialization uses the single
//! keyword whose bits equal the value, else the mixed keywords in declaration order.
//!
//! Ported from `stylo_derive` (`parse.rs`, `to_css.rs`) and the flag definitions in Stylo
//! `values/specified/{text,box}.rs` (MPL-2.0), Servo build.
use super::idents;

/// One bitflags type: keywords with their bits, as Stylo defines them.
pub struct Flags {
    single: &'static [(&'static str, u32)],
    mixed: &'static [(&'static str, u32)],
    /// Some keywords include others' bits (`contain: size` includes `inline-size`).
    overlapping: bool,
    /// Extra validity rule on the combined bits (`validate_mixed`).
    validate: fn(u32) -> bool,
}

const fn any(_: u32) -> bool {
    true
}

/// `text-decoration-line`.
pub const TEXT_DECORATION_LINE: Flags = Flags {
    single: &[("none", 0)],
    mixed: &[
        ("underline", 1),
        ("overline", 2),
        ("line-through", 4),
        ("blink", 8),
    ],
    overlapping: false,
    validate: any,
};

/// `touch-action`.
pub const TOUCH_ACTION: Flags = Flags {
    single: &[("none", 1), ("auto", 2), ("manipulation", 16)],
    mixed: &[("pan-x", 4), ("pan-y", 8), ("pinch-zoom", 32)],
    overlapping: false,
    validate: any,
};

/// `text-transform`: at most one case transform.
pub const TEXT_TRANSFORM: Flags = Flags {
    single: &[("none", 0), ("math-auto", 8)],
    mixed: &[
        ("uppercase", 1),
        ("lowercase", 2),
        ("capitalize", 4),
        ("full-width", 16),
        ("full-size-kana", 32),
    ],
    overlapping: false,
    validate: |bits| (bits & 0b1111).count_ones() <= 1,
};

/// `text-underline-position`: not `left` with `right`, nor `from-font` with `under`.
pub const TEXT_UNDERLINE_POSITION: Flags = Flags {
    single: &[("auto", 0)],
    mixed: &[("from-font", 1), ("under", 2), ("left", 4), ("right", 8)],
    overlapping: false,
    validate: |bits| bits & 0b1100 != 0b1100 && bits & 0b11 != 0b11,
};

const INLINE_SIZE: u32 = 1;
const BLOCK_SIZE: u32 = 2;
const LAYOUT: u32 = 4;
const STYLE: u32 = 8;
const PAINT: u32 = 16;
const SIZE: u32 = 32 | INLINE_SIZE | BLOCK_SIZE;

/// `contain`.
pub const CONTAIN: Flags = Flags {
    single: &[
        ("none", 0),
        ("strict", 128 | LAYOUT | STYLE | PAINT | SIZE),
        ("content", 64 | LAYOUT | STYLE | PAINT),
    ],
    mixed: &[
        ("size", SIZE),
        ("layout", LAYOUT),
        ("style", STYLE),
        ("paint", PAINT),
        ("inline-size", INLINE_SIZE),
    ],
    overlapping: true,
    validate: any,
};

/// `container-type`: not `size` with `inline-size`; `scroll-state` is behind a preference
/// that is off.
pub const CONTAINER_TYPE: Flags = Flags {
    single: &[("normal", 0)],
    mixed: &[("size", 2), ("inline-size", 1), ("scroll-state", 4)],
    overlapping: false,
    validate: |bits| bits & 0b11 != 0b11 && bits & 4 == 0,
};

/// Parse and serialize `value` as `flags`, or `None` if invalid.
pub fn flags(value: &str, flags: &Flags) -> Option<String> {
    let words = idents(value)?;
    let mut bits = 0;
    for (i, word) in words.iter().enumerate() {
        if bits == 0
            && let Some(single) = flags.single.iter().find(|(name, _)| name == word)
        {
            // A single keyword ends the value.
            return (i + 1 == words.len()).then(|| single.0.to_owned());
        }
        let (_, flag) = flags.mixed.iter().find(|(name, _)| name == word)?;
        if bits & flag != 0 {
            return None;
        }
        bits |= flag;
    }
    if bits == 0 || !(flags.validate)(bits) {
        return None;
    }
    if let Some((name, _)) = flags.single.iter().find(|(_, b)| *b == bits) {
        return Some((*name).to_owned());
    }
    let mut serialized = 0;
    let mut out = Vec::new();
    for (name, flag) in flags.mixed {
        let present = if flags.overlapping {
            bits & flag == *flag && serialized & flag == 0
        } else {
            bits & flag != 0
        };
        if present {
            out.push(*name);
            serialized |= flag;
        }
    }
    Some(out.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_follow_stylo() {
        assert_eq!(
            flags("Underline  overline", &TEXT_DECORATION_LINE).as_deref(),
            Some("underline overline")
        );
        assert_eq!(
            flags("blink underline", &TEXT_DECORATION_LINE).as_deref(),
            Some("underline blink")
        );
        assert_eq!(flags("underline underline", &TEXT_DECORATION_LINE), None);
        assert_eq!(flags("none underline", &TEXT_DECORATION_LINE), None);
        assert_eq!(flags("uppercase lowercase", &TEXT_TRANSFORM), None);
        assert_eq!(
            flags("full-width capitalize", &TEXT_TRANSFORM).as_deref(),
            Some("capitalize full-width")
        );
        assert_eq!(flags("paint size", &CONTAIN).as_deref(), Some("size paint"));
        assert_eq!(flags("size inline-size", &CONTAIN), None);
        assert_eq!(flags("left right", &TEXT_UNDERLINE_POSITION), None);
        assert_eq!(flags("scroll-state", &CONTAINER_TYPE), None);
        assert_eq!(flags("", &TOUCH_ACTION), None);
    }
}
