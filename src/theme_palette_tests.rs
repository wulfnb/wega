use super::*;
#[test]
fn themes_resolve_system_appearance_and_native_fallback() {
    for theme in Theme::ALL {
        let p = theme.palette(false, false);
        assert_eq!(p.chrome.a(), 255);
        assert_ne!(p.text, p.chrome);
    }
    assert!(!Theme::Glass.palette(false, true).dark);
    assert!(Theme::Glass.palette(true, true).dark);
    assert!(Theme::GlassLight.palette(true, true).chrome.a() < 255);
    assert!(!Theme::GlassLight.palette(true, false).glass);
    assert_eq!(
        serde_json::from_str::<crate::profile::Profile>("{}")
            .unwrap()
            .theme,
        Theme::Emerald
    );
}

fn contrast(a: Color32, b: Color32) -> f32 {
    let luminance = |c: Color32| {
        let channel = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    };
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// WCAG AA (4.5:1) for every text role on the surfaces it is drawn on, in
/// every theme and both system appearances.
#[test]
fn text_roles_meet_wcag_aa() {
    for theme in Theme::ALL {
        for system_dark in [false, true] {
            let p = theme.palette(system_dark, false);
            let check = |fg: Color32, bg: Color32, what: &str| {
                let ratio = contrast(fg, bg);
                assert!(ratio >= 4.5, "{theme:?} {what}: {ratio:.2}:1");
            };
            for (bg, surface) in [
                (p.chrome, "chrome"),
                (p.inset, "inset"),
                (p.surface, "surface"),
            ] {
                for (fg, role) in [
                    (p.text, "text"),
                    (p.muted, "muted"),
                    (p.accent, "accent"),
                    (p.danger, "danger"),
                    (p.success, "success"),
                ] {
                    check(fg, bg, &format!("{role} on {surface}"));
                }
            }
            check(p.text, p.hover, "text on hover");
            check(p.text, p.selected, "text on selected");
            check(p.on_accent, p.accent, "on_accent on accent");
            check(p.danger, p.danger_bg, "danger on danger_bg");
            check(p.text, p.danger_bg, "text on danger_bg");
        }
    }
}
