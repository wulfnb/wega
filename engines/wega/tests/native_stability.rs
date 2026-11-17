use std::{cell::RefCell, rc::Rc};
use wega_adapter_blitz::{BlitzDocument, DocumentSetup};
use wega_dom::{DocumentView, style::StyleControl};
use wega_engine::native_engine::*;
fn native(html: &str) -> NativeDocument {
    NativeDocument::new(Rc::new(RefCell::new(BlitzDocument::from_html(
        html,
        DocumentSetup::default(),
    ))))
}
#[test]
fn native_depth_probe() {
    let Ok(depth) = std::env::var("WEGA_NATIVE_DEPTH") else {
        return;
    };
    let depth = depth.parse::<usize>().unwrap();
    std::thread::Builder::new()
        .stack_size(wega_engine::ENGINE_THREAD_STACK)
        .spawn(move || {
            let html = format!(
                "<body>{}text{}",
                "<div>".repeat(depth),
                "</div>".repeat(depth)
            );
            let mut n = native(&html);
            n.flush_layout();
            if depth > wega_engine::native_engine::layout::MAX_NATIVE_LAYOUT_DEPTH {
                assert!(n.layout_refused);
                assert!(n.layout_tree.is_none());
            } else {
                assert!(!n.layout_refused);
                assert!(n.layout_tree.is_some());
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
#[test]
fn deep_dom_is_bounded_on_the_engine_stack() {
    for depth in [64, 124, 252, 300] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "native_depth_probe", "--nocapture"])
            .env("WEGA_NATIVE_DEPTH", depth.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "depth {depth}: {:?}",
                    child.wait_with_output().unwrap().stderr
                );
                break;
            }
            if std::time::Instant::now() > deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("depth {depth} exceeded bounded layout deadline");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
#[test]
fn malformed_css_and_nested_functions_are_bounded() {
    for value in ["#é0", "#💥", "#ffggff"] {
        assert!(Color::parse(value).is_none());
    }
    let nested = format!(
        "p{{--x:{}0{};color:red}}",
        "f(".repeat(1000),
        ")".repeat(1000)
    );
    let sheet = StyleSheet::parse(&nested);
    assert!(sheet.rules.len() <= 1);
    let css = (0..4000)
        .map(|i| format!(".c{i} {{ width:calc(50% - 2px);color:red }}"))
        .collect::<String>();
    let sheet = StyleSheet::parse(&css);
    assert_eq!(sheet.rules.len(), 4000);
    for css in [
        "p:hover{color:red}",
        "p:unknown(){width:2px}",
        "[broken { color:red }",
        "p {width:calc(2px / 0);height:20px}",
    ] {
        let _ = StyleSheet::parse(css);
    }
}
#[test]
fn large_unicode_text_and_repeated_invalidation_remain_renderable() {
    let html = format!(
        "<body><p id=p style='width:400px'>{}</p></body>",
        "Unicode café e\u{301} שלום العربية readable text. ".repeat(1000)
    );
    let mut n = native(&html);
    n.flush_layout();
    let key = n.doc.borrow().element_by_id("p").unwrap();
    assert!(n.find_box(key).unwrap().lines.len() > 100);
    for i in 0..40 {
        n.doc
            .borrow_mut()
            .set_inline_property(
                key,
                "width",
                if i % 2 == 0 { "300px" } else { "400px" },
                false,
            )
            .unwrap();
        n.ensure_layout();
        assert!(n.bounding_rect(key).unwrap().height.is_finite());
    }
    assert_eq!(n.layout_generation, 41);
    assert!(!n.is_dirty());
}
#[test]
fn custom_property_cycles_are_invalid_even_with_cycle_fallbacks() {
    let mut n = native(
        "<style>div{--a:var(--b,10px);--b:var(--a,20px);width:var(--a,30px);font-size:2em;line-height:2em}body{font-size:10px}#x{font-size:3em}</style><div id=x></div>",
    );
    n.flush_layout();
    let key = n.doc.borrow().element_by_id("x").unwrap();
    let b = n.find_box(key).unwrap();
    assert_eq!(b.border_box.width, 30.0);
    assert_eq!(b.style.font_size, 30.0);
    assert_eq!(b.style.line_height, 60.0);
}
