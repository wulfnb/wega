use super::Theme;
use egui::Color32;
#[derive(Clone, Copy)]
pub struct Palette {
    pub chrome: Color32,
    pub inset: Color32,
    pub surface: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub accent: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub amber: Color32,
    pub danger: Color32,
    pub border: Color32,
    pub warning_bg: Color32,
    /// Text and icons drawn on an `accent` fill (primary buttons).
    pub on_accent: Color32,
    /// Container behind error messages; `danger` and `text` stay readable on it.
    pub danger_bg: Color32,
    /// Confirmation colour for completed actions.
    pub success: Color32,
    pub dark: bool,
    pub glass: bool,
}
fn rgb(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}
impl Theme {
    pub fn palette(self, system_dark: bool, native: bool) -> Palette {
        let dark = self.dark(system_dark);
        let values = match self {
            Self::Emerald => [
                0x0c2925, 0x061b18, 0x153c34, 0x205247, 0x236553, 0x93f5d5, 0xedfff7, 0xaacabd,
                0xf0cf87, 0xffa6b5, 0x3c7868,
            ],
            Self::Midnight => [
                0x151d2b, 0x0d1420, 0x253144, 0x31425a, 0x304d70, 0x8bc7ff, 0xe7eef8, 0x9daec5,
                0xeac27b, 0xff9caa, 0x3b4d66,
            ],
            // Warm off-white with ink-brown accents, distinct from the cool Glass Light.
            Self::Paper => [
                0xf3efe6, 0xfbf9f4, 0xe8e2d5, 0xddd5c5, 0xe6d8bd, 0x7a4b1f, 0x2b2620, 0x6b6256,
                0x8a5a00, 0xa8323e, 0xd6cfc0,
            ],
            _ if !dark => [
                0xe9edf3, 0xf8fafd, 0xdce3ed, 0xcdd9e9, 0xc0d7f7, 0x1657a5, 0x1d293b, 0x536479,
                0x825008, 0xad2440, 0xc9d2de,
            ],
            _ => [
                0x222630, 0x171b24, 0x343b49, 0x414c60, 0x345a83, 0x9acaff, 0xf0f4fb, 0xaab7cc,
                0xeec584, 0xffa4b4, 0x53637b,
            ],
        };
        let [
            chrome,
            inset,
            surface,
            hover,
            selected,
            accent,
            text,
            muted,
            amber,
            danger,
            border,
        ] = values.map(rgb);
        let mut palette = Palette {
            chrome,
            inset,
            surface,
            hover,
            selected,
            accent,
            text,
            muted,
            amber,
            danger,
            border,
            warning_bg: rgb(if dark { 0x3b3124 } else { 0xffefcd }),
            // `inset` is near-black in dark themes (pale accents) and near-white
            // in light ones (dark accents), so it always contrasts the accent.
            on_accent: inset,
            danger_bg: mix(inset, danger, if dark { 0.16 } else { 0.10 }),
            success: rgb(if dark { 0x8fd99a } else { 0x1f6f34 }),
            dark,
            glass: self.glass() && native,
        };
        if palette.glass {
            // A thin tint lets a light desktop wash out dark chrome, so dark glass
            // stays mostly opaque and muted text moves toward the body colour.
            palette.chrome = Color32::from_rgba_unmultiplied(
                chrome.r(),
                chrome.g(),
                chrome.b(),
                if dark { 205 } else { 170 },
            );
            palette.inset = Color32::from_rgba_unmultiplied(inset.r(), inset.g(), inset.b(), 230);
            palette.muted = mix(muted, text, 0.4);
        }
        palette
    }
}
fn mix(from: Color32, to: Color32, t: f32) -> Color32 {
    let channel = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgb(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
    )
}
fn key() -> egui::Id {
    egui::Id::unique("browser-palette")
}
pub fn palette(ctx: &egui::Context) -> Palette {
    ctx.data(|data| data.get_temp::<Palette>(key()))
        .unwrap_or_else(|| Theme::Emerald.palette(true, false))
}
pub fn install(ctx: &egui::Context, palette: Palette) {
    ctx.data_mut(|data| data.insert_temp(key(), palette));
}
#[cfg(test)]
#[path = "theme_palette_tests.rs"]
mod tests;
