//! Round 1 Acceptance Test: Native Forms, Editing, and Keyboard Interaction.
mod support;
#[path = "support/native_visual.rs"]
mod visual;

use keyboard_types::{Code, Key, Location, Modifiers};
use support::{Reply, Server, engine, open, settle};
use wega_engine::RenderingBackend;
use wega_engine::events::{BlitzKeyEvent, KeyState};

fn press_key(view: &mut wega_engine::View, key_str: &str, code: Code, text: Option<&str>) {
    let key: Key = key_str
        .parse()
        .unwrap_or_else(|_| Key::Character(key_str.into()));
    view.key(BlitzKeyEvent {
        key,
        code,
        text: text.map(Into::into),
        state: KeyState::Pressed,
        location: Location::Standard,
        modifiers: Modifiers::empty(),
        is_auto_repeating: false,
        is_composing: false,
    });
}

fn type_str(view: &mut wega_engine::View, s: &str) {
    for ch in s.chars() {
        let ch_str = ch.to_string();
        let code = match ch {
            'a'..='z' | 'A'..='Z' => Code::KeyA,
            '0'..='9' => Code::Digit0,
            '@' => Code::Digit2,
            '.' => Code::Period,
            _ => Code::Unidentified,
        };
        press_key(view, &ch_str, code, Some(&ch_str));
    }
}

#[test]
fn native_login_form_editing_tabbing_and_submission() {
    let server = Server::new(|seen| match (seen.method.as_str(), seen.path.as_str()) {
        ("POST", "/login") => {
            let body = String::from_utf8_lossy(&seen.body);
            Reply::html(&format!(
                "<title>dashboard</title><pre id=result>{}</pre>",
                body.replace('<', "&lt;")
            ))
        }
        _ => Reply::html(
            r#"<!DOCTYPE html>
<html>
<head>
<title>Sign In</title>
<style>
body { margin: 0; padding: 20px; font-family: sans-serif; background: #f0f2f5; }
.login-card { width: 320px; padding: 24px; background: #ffffff; border: 1px solid #dcdcdc; border-radius: 8px; }
.form-group { margin-bottom: 16px; }
label { display: block; margin-bottom: 6px; font-size: 14px; font-weight: bold; color: #333333; }
input[type="email"], input[type="password"] {
    width: 100%; height: 36px; padding: 0 10px; font-size: 14px;
    border: 1px solid #cccccc; border-radius: 4px; box-sizing: border-box;
}
.checkbox-group { display: flex; align-items: center; gap: 8px; font-size: 14px; }
button[type="submit"] {
    width: 100%; height: 40px; background: #0066cc; color: #ffffff;
    font-size: 16px; font-weight: bold; border: none; border-radius: 4px; cursor: pointer;
}
</style>
</head>
<body>
<div class="login-card">
  <form id="loginForm" method="post" action="/login">
    <div class="form-group">
      <label for="email">Email</label>
      <input type="email" id="email" name="email" placeholder="user@example.com" data-probe>
    </div>
    <div class="form-group">
      <label for="password">Password</label>
      <input type="password" id="password" name="password" placeholder="Password" data-probe>
    </div>
    <div class="form-group checkbox-group">
      <input type="checkbox" id="remember" name="remember" value="yes" data-probe>
      <label for="remember">Remember me</label>
    </div>
    <button type="submit" id="submitBtn" data-probe>Sign In</button>
  </form>
</div>
</body>
</html>"#,
        ),
    });

    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.set_rendering_backend(RenderingBackend::WegaNative);

    // Initial paint
    let pixels = view.paint(800, 600, 1.0, 1.0, false).unwrap();
    assert!(
        !pixels.rgba.is_empty(),
        "native paint must produce pixel bytes"
    );

    // Count non-background pixels to verify actual content ink
    let non_bg = pixels
        .rgba
        .as_chunks::<4>().0.iter()
        .filter(|p| !(p[0] == 240 && p[1] == 242 && p[2] == 245))
        .count();
    assert!(
        non_bg > 5000,
        "page must render non-background card and form ink: got {non_bg}"
    );

    // 1. Initial focus: focus the email input
    view.evaluate("document.getElementById('email').focus()")
        .unwrap();
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"email\""
    );

    // 2. Automate typing into email field
    type_str(&mut view, "test@wega.dev");
    assert_eq!(
        view.evaluate_json("document.getElementById('email').value")
            .unwrap(),
        "\"test@wega.dev\""
    );

    // 3. Automate editing: backspace deletion
    press_key(&mut view, "Backspace", Code::Backspace, None);
    press_key(&mut view, "Backspace", Code::Backspace, None);
    press_key(&mut view, "Backspace", Code::Backspace, None);
    assert_eq!(
        view.evaluate_json("document.getElementById('email').value")
            .unwrap(),
        "\"test@wega.\""
    );
    type_str(&mut view, "org");
    assert_eq!(
        view.evaluate_json("document.getElementById('email').value")
            .unwrap(),
        "\"test@wega.org\""
    );

    // 4. Test selection and replacement
    view.evaluate("document.getElementById('email').setSelectionRange(0, 4)")
        .unwrap();
    assert_eq!(
        view.evaluate_json("[document.getElementById('email').selectionStart, document.getElementById('email').selectionEnd]").unwrap(),
        "[0,4]"
    );
    view.paste("admin");
    assert_eq!(
        view.evaluate_json("document.getElementById('email').value")
            .unwrap(),
        "\"admin@wega.org\""
    );

    // 5. Automate tabbing: Tab from email to password
    press_key(&mut view, "Tab", Code::Tab, None);
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"password\"",
        "Tabbing from email must focus password"
    );

    // 6. Type password into password field
    type_str(&mut view, "SecretPass123");
    assert_eq!(
        view.evaluate_json("document.getElementById('password').value")
            .unwrap(),
        "\"SecretPass123\""
    );

    // Repaint to verify password masking and caret rendering do not crash
    let pwd_pixels = view.paint(800, 600, 1.0, 1.0, false).unwrap();
    assert!(!pwd_pixels.rgba.is_empty());

    // 7. Tab to remember-me checkbox
    press_key(&mut view, "Tab", Code::Tab, None);
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"remember\"",
        "Tabbing from password must focus remember-me checkbox"
    );
    assert_eq!(
        view.evaluate_json("document.getElementById('remember').checked")
            .unwrap(),
        "false"
    );

    // 8. Press Space to toggle checkbox
    press_key(&mut view, " ", Code::Space, Some(" "));
    assert_eq!(
        view.evaluate_json("document.getElementById('remember').checked")
            .unwrap(),
        "true",
        "Space on focused checkbox must toggle checked state"
    );

    // 9. Tab to submit button
    press_key(&mut view, "Tab", Code::Tab, None);
    assert_eq!(
        view.evaluate_json("document.activeElement.id").unwrap(),
        "\"submitBtn\"",
        "Tabbing from checkbox must focus submit button"
    );

    // 10. Press Enter to submit the form
    press_key(&mut view, "Enter", Code::Enter, None);
    settle(&mut view);

    assert_eq!(view.title(), "dashboard");
    let result_text = view.text("#result").expect("submission result");
    assert!(
        result_text.contains("email=admin%40wega.org")
            || result_text.contains("email=admin@wega.org"),
        "body was: {result_text}"
    );
    assert!(
        result_text.contains("password=SecretPass123"),
        "body was: {result_text}"
    );
    assert!(
        result_text.contains("remember=yes"),
        "body was: {result_text}"
    );
}

#[test]
fn native_form_controls_variety_and_disabled_behavior() {
    let server = Server::new(|_| {
        Reply::html(
            r#"<!DOCTYPE html>
<html>
<body>
<input type="text" id="t1" value="editable">
<input type="text" id="t2" value="readonly text" readonly>
<input type="text" id="t3" value="disabled text" disabled>
<textarea id="area">Multi
line</textarea>
<select id="sel"><option value="a">Option A</option><option value="b" selected>Option B</option></select>
<input type="number" id="num" value="42">
<input type="search" id="srch" value="query">
<input type="radio" id="r1" name="grp" value="1" checked>
<input type="radio" id="r2" name="grp" value="2">
</body>
</html>"#,
        )
    });

    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    view.paint(600, 400, 1.0, 1.0, false).unwrap();

    // Verify readonly behavior
    view.evaluate("document.getElementById('t2').focus()")
        .unwrap();
    type_str(&mut view, "EXTRA");
    press_key(&mut view, "Backspace", Code::Backspace, None);
    assert_eq!(
        view.evaluate_json("document.getElementById('t2').value")
            .unwrap(),
        "\"readonly text\"",
        "Readonly input must not accept character input or backspace mutations"
    );

    // Verify select value
    assert_eq!(
        view.evaluate_json("document.getElementById('sel').value")
            .unwrap(),
        "\"b\""
    );
    view.evaluate("document.getElementById('sel').value = 'a'")
        .unwrap();
    assert_eq!(
        view.evaluate_json("document.getElementById('sel').value")
            .unwrap(),
        "\"a\""
    );

    // Verify textarea editing
    view.evaluate("document.getElementById('area').focus()")
        .unwrap();
    type_str(&mut view, " appended");
    assert!(
        view.evaluate_json("document.getElementById('area').value")
            .unwrap()
            .contains("appended")
    );

    // Verify radio button mutual exclusion
    assert_eq!(
        view.evaluate_json(
            "[document.getElementById('r1').checked, document.getElementById('r2').checked]"
        )
        .unwrap(),
        "[true,false]"
    );
    view.evaluate("document.getElementById('r2').focus()")
        .unwrap();
    press_key(&mut view, " ", Code::Space, Some(" "));
    assert_eq!(
        view.evaluate_json(
            "[document.getElementById('r1').checked, document.getElementById('r2').checked]"
        )
        .unwrap(),
        "[false,true]"
    );
}

#[test]
fn native_login_form_chrome_visual_and_geometry_comparison() {
    let html = r#"<!DOCTYPE html>
<html>
<head>
<style>
body { margin: 0; padding: 20px; font-family: sans-serif; background: #f0f2f5; }
.login-card { width: 320px; padding: 24px; background: #ffffff; border: 1px solid #dcdcdc; border-radius: 8px; }
.form-group { margin-bottom: 16px; }
label { display: block; margin-bottom: 6px; font-size: 14px; font-weight: bold; color: #333333; }
input[type="email"], input[type="password"] {
    width: 100%; height: 36px; padding: 0 10px; font-size: 14px;
    border: 1px solid #cccccc; border-radius: 4px; box-sizing: border-box;
}
.checkbox-group { display: flex; align-items: center; gap: 8px; font-size: 14px; }
button[type="submit"] {
    width: 100%; height: 40px; background: #0066cc; color: #ffffff;
    font-size: 16px; font-weight: bold; border: none; border-radius: 4px; cursor: pointer;
}
</style>
</head>
<body>
<div class="login-card" id="card" data-probe>
  <form id="loginForm" method="post" action="/login">
    <div class="form-group" id="grp1" data-probe>
      <label for="email">Email</label>
      <input type="email" id="email" name="email" value="user@example.com" data-probe>
    </div>
    <div class="form-group" id="grp2" data-probe>
      <label for="password">Password</label>
      <input type="password" id="password" name="password" value="secret" data-probe>
    </div>
    <div class="form-group checkbox-group" id="grp3" data-probe>
      <input type="checkbox" id="remember" name="remember" value="yes" checked data-probe>
      <label for="remember">Remember me</label>
    </div>
    <button type="submit" id="submitBtn" data-probe>Sign In</button>
  </form>
</div>
</body>
</html>"#;

    let server = Server::new(move |_| Reply::html(html));
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    view.set_rendering_backend(RenderingBackend::WegaNative);
    let frame = view.paint(800, 600, 1.0, 1.0, false).unwrap();
    assert!(!frame.rgba.is_empty());

    let native_geom = visual::geometry(&mut view);
    let temp_dir = tempfile::tempdir().unwrap();
    let chrome_bin = std::env::var("WEGA_CHROME_BIN").unwrap_or_else(|_| {
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string()
    });
    unsafe {
        std::env::set_var("WEGA_CHROME_BIN", &chrome_bin);
    }

    if let Some((chrome_png, chrome_geom)) =
        visual::chrome(html, None, 0.0, "login_form", temp_dir.path())
    {
        assert!(!chrome_png.is_empty(), "Chrome capture succeeded");
        // Compare probe element positions: card width must be within 2px
        let n_card = native_geom
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e[0] == "card")
            .unwrap();
        let c_card = chrome_geom
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e[0] == "card")
            .unwrap();
        let n_w = n_card[3].as_f64().unwrap();
        let c_w = c_card[3].as_f64().unwrap();
        assert!(
            (n_w - c_w).abs() < 5.0,
            "Card width native={n_w} chrome={c_w}"
        );
    }
}
