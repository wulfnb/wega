use super::*;
use egui::{Context, Event, PointerButton, Pos2, RawInput};

type Nodes = Vec<(egui::accesskit::NodeId, egui::accesskit::Node)>;

fn frame(ctx: &Context, events: Vec<Event>, mut draw: impl FnMut(&mut Ui)) -> Nodes {
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| draw(ui),
    );
    output.textures_delta.clear();
    output.platform_output.accesskit_update.unwrap().nodes
}

fn node<'a>(nodes: &'a Nodes, label: &str) -> &'a egui::accesskit::Node {
    &nodes
        .iter()
        // egui exposes a text label's string as its value.
        .find(|(_, n)| n.label() == Some(label) || n.value() == Some(label))
        .unwrap_or_else(|| panic!("missing {label}"))
        .1
}

fn center(node: &egui::accesskit::Node) -> Pos2 {
    let b = node.bounds().unwrap();
    egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

fn click(ctx: &Context, at: Pos2, draw: &mut impl FnMut(&mut Ui)) {
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(ctx, vec![Event::PointerMoved(at), button(true)], &mut *draw);
    frame(ctx, vec![button(false)], &mut *draw);
}

fn context() -> Context {
    let ctx = Context::default();
    ctx.enable_accesskit();
    ctx
}

#[test]
fn tabs_expose_selection_and_switch_on_click() {
    let ctx = context();
    let mut current = 1;
    let mut changes = 0;
    let mut draw = |ui: &mut Ui| {
        if tabs(ui, &mut current, &[(1, "Console"), (2, "Network")]) {
            changes += 1;
        }
    };
    let nodes = frame(&ctx, vec![], &mut draw);
    assert_eq!(node(&nodes, "Console").role(), egui::Role::Tab);
    assert_eq!(node(&nodes, "Console").is_selected(), Some(true));
    assert_eq!(node(&nodes, "Network").is_selected(), Some(false));
    // Clicking the selected tab is not a change.
    click(&ctx, center(node(&nodes, "Console")), &mut draw);
    click(&ctx, center(node(&nodes, "Network")), &mut draw);
    let nodes = frame(&ctx, vec![Event::PointerMoved(Pos2::ZERO)], &mut draw);
    assert_eq!(node(&nodes, "Network").is_selected(), Some(true));
    assert_eq!((current, changes), (2, 1));
}

#[test]
fn dialog_buttons_put_the_primary_action_last_and_report_the_choice() {
    for (destructive, target, expected) in [(false, "Save", true), (true, "Cancel", false)] {
        let ctx = context();
        let mut answer = None;
        let mut draw = |ui: &mut Ui| {
            if let Some(choice) = dialog_buttons(ui, Some("Cancel"), "Save", destructive) {
                answer = Some(choice);
            }
        };
        let nodes = frame(&ctx, vec![], &mut draw);
        assert!(center(node(&nodes, "Cancel")).x < center(node(&nodes, "Save")).x);
        click(&ctx, center(node(&nodes, target)), &mut draw);
        assert_eq!(answer, Some(expected));
    }
    let ctx = context();
    let nodes = frame(&ctx, vec![], |ui| {
        dialog_buttons(ui, None, "OK", false);
    });
    assert!(nodes.iter().all(|(_, n)| n.label() != Some("Cancel")));
}

#[test]
fn messages_chips_and_empty_states_render_with_actions() {
    let ctx = context();
    let mut clicked = false;
    let mut toggled = false;
    let mut draw = |ui: &mut Ui| {
        for severity in [
            Severity::Error,
            Severity::Warning,
            Severity::Success,
            Severity::Info,
        ] {
            inline_message(ui, severity, &format!("{severity:?} message"));
        }
        toggled |= chip(ui, true, "Errors").clicked();
        chip(ui, false, "Warnings");
        ui.add(primary_button(ui, "Primary"));
        clicked |= empty_state(
            ui,
            "No requests yet",
            "Reload the page to record traffic.",
            Some("Reload"),
        );
        empty_state(ui, "Nothing here", "", None);
    };
    let nodes = frame(&ctx, vec![], &mut draw);
    for label in [
        "Error message",
        "Info message",
        "No requests yet",
        "Nothing here",
        "Primary",
    ] {
        node(&nodes, label);
    }
    assert_eq!(
        node(&nodes, "Errors").toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    click(&ctx, center(node(&nodes, "Reload")), &mut draw);
    click(&ctx, center(node(&nodes, "Errors")), &mut draw);
    assert!(clicked && toggled);
}
