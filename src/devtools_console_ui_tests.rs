use super::*;
use wega::engine_protocol::inspector::{PreviewProp, SourceLocation};

fn prop(key: &str, value: Preview) -> PreviewProp {
    PreviewProp {
        key: key.into(),
        value,
    }
}

#[test]
fn previews_render_compactly_and_never_expand_beyond_their_budget() {
    let number = Preview::simple(PreviewKind::Number, "1");
    assert_eq!(preview_line(&number, false), "1");
    assert_eq!(
        preview_line(&Preview::simple(PreviewKind::String, "hi"), false),
        "hi"
    );
    assert_eq!(
        preview_line(&Preview::simple(PreviewKind::String, "hi"), true),
        "\"hi\""
    );
    let object = Preview {
        props: vec![
            prop("a", number.clone()),
            prop("b", Preview::simple(PreviewKind::String, "x")),
        ],
        more: 3,
        ..Preview::simple(PreviewKind::Object, "Point")
    };
    assert_eq!(
        preview_line(&object, false),
        "Point {a: 1, b: \"x\", … 3 more}"
    );
    let array = Preview {
        props: vec![prop("0", number.clone()), prop("1", number.clone())],
        more: 0,
        ..Preview::simple(PreviewKind::Array, "Array(2)")
    };
    assert_eq!(preview_line(&array, false), "(2) [1, 1]");
    let cut = Preview {
        truncated: true,
        ..Preview::simple(PreviewKind::String, "long text")
    };
    assert_eq!(preview_line(&cut, false), "long text…");
    assert_eq!(
        preview_line(&Preview::simple(PreviewKind::Circular, "[Circular]"), true),
        "[Circular]"
    );
}

#[test]
fn source_labels_use_the_file_name_and_position() {
    let message = ConsoleMessage {
        seq: 1,
        timestamp_ms: 0,
        level: ConsoleLevel::Log,
        method: "log".into(),
        args: vec![],
        text: String::new(),
        source: Some(SourceLocation {
            url: "https://e.test/js/app.js".into(),
            line: 4,
            column: 9,
        }),
        exception: None,
    };
    assert_eq!(source_label(&message).as_deref(), Some("app.js:4:9"));
    assert_eq!(clock(1_700_000_000_123).len(), 12);
}

#[test]
fn the_console_tab_draws_every_kind_of_line_without_panicking() {
    let ctx = egui::Context::default();
    crate::ui::style(&ctx);
    let mut console = ConsoleView::default();
    console.push(ConsoleLine::Notice("note".into()));
    console.push(ConsoleLine::Input("1+1".into()));
    console.push(ConsoleLine::Result(Preview::simple(
        PreviewKind::Number,
        "2",
    )));
    for level in [
        ConsoleLevel::Debug,
        ConsoleLevel::Log,
        ConsoleLevel::Info,
        ConsoleLevel::Warn,
        ConsoleLevel::Error,
    ] {
        console.push(ConsoleLine::Message(Box::new(ConsoleMessage {
            seq: 1,
            timestamp_ms: 1,
            level,
            method: "log".into(),
            args: vec![Preview::simple(PreviewKind::String, "text")],
            text: "text".into(),
            source: None,
            exception: None,
        })));
    }
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        for line in console.lines() {
            draw_line(ui, line);
        }
        level_toggles(ui, &mut console);
    });
    assert!(!output.shapes.is_empty());
    output.textures_delta.clear();
}
