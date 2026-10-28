//! Readability fallbacks for CSS features not yet implemented by the pinned Servo release.
use std::rc::Rc;

// Servo 0.7 does not implement background-clip:text. Tailwind's transparent text
// then disappears while its gradient is painted as a rectangle. Keep the document's
// gradient end color (or inherited text color) and remove only that unsupported background. Other
// gradients (buttons, cards, artwork) are unaffected. This rule retires itself when
// the engine reports support for either spelling of text clipping.
pub const READABLE_GRADIENT_TEXT: &str = r#"
@supports not ((background-clip: text) or (-webkit-background-clip: text)) {
    .bg-clip-text,
    [style*="background-clip:text" i],
    [style*="background-clip: text" i] {
        background-image: none !important;
        background-color: transparent !important;
        color: var(--tw-gradient-to, var(--tw-gradient-from, inherit)) !important;
        -webkit-text-fill-color: currentColor !important;
    }
}
"#;

pub fn content_manager(servo: &servo::Servo) -> Rc<servo::UserContentManager> {
    let manager = Rc::new(servo::UserContentManager::new(servo));
    manager.add_script(Rc::new(servo::UserScript::new(
        include_str!("page_state.js").into(),
        None,
    )));
    manager.add_stylesheet(Rc::new(servo::user_contents::UserStyleSheet::new(
        READABLE_GRADIENT_TEXT.into(),
        url::Url::parse("about:wega-readability").unwrap(),
    )));
    manager.add_script(Rc::new(servo::UserScript::new(
        "document.addEventListener('click',e=>{const a=e.target.closest?.('a[download]');if(!a||e.defaultPrevented||e.button!==0)return;const u=new URL(a.href,location.href);if(!['http:','https:'].includes(u.protocol))return;e.preventDefault();location.href='wega://download?url='+encodeURIComponent(u.href)+'&name='+encodeURIComponent(a.getAttribute('download')||'')},false);".into(),None,
    )));
    manager.add_script(Rc::new(servo::UserScript::new(
        include_str!("media_fallback.js").into(),
        None,
    )));
    manager.add_script(Rc::new(servo::UserScript::new(
        include_str!("mask_icons.js").into(),
        None,
    )));
    manager
}
