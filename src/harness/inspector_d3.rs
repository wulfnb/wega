//! Native acceptance of the final D3 Elements flow, run inside `--inspector-test` with a real
//! window, a real `Page`, a real `wega-engine` process and the real UI loop: pointer events are
//! injected into egui's input (so the real interception code in `wega_ui.rs` consumes them),
//! edits go through the real browser-to-engine authority, and the rendered result is checked on
//! the engine's own page pixels (which never contain the overlay).
use crate::devtools_overlay::Plan;
use std::time::{Duration, Instant};
use wega::engine_protocol::inspector::{
    GrantToken, InspectorArgs, InspectorCommand, InspectorErrorCode, edit::MutationOp,
};
use winit::{
    event::ElementState,
    keyboard::{Key, NamedKey},
};

/// Write the engine's page frame with the overlay plan composited on it, when
/// `WEGA_D3_EVIDENCE_DIR` is set. This is the page pixels plus the exact rectangles the egui
/// painter draws (no label text), not an OS screenshot, so nothing outside Wega is captured.
fn evidence(
    name: &str,
    frame: &(u32, u32, std::sync::Arc<[u8]>),
    viewport_css: (f64, f64),
    plans: &[(Plan, bool)],
) {
    let Ok(dir) = std::env::var("WEGA_D3_EVIDENCE_DIR") else {
        return;
    };
    let (w, h) = (frame.0 as usize, frame.1 as usize);
    let mut rgba = frame.2.to_vec();
    let (sx, sy) = (
        w as f64 / viewport_css.0.max(1.0),
        h as f64 / viewport_css.1.max(1.0),
    );
    let blend = |rgba: &mut Vec<u8>,
                 r: &wega::engine_protocol::inspector::geometry::GeomRect,
                 c: [u8; 3],
                 a: u32| {
        let (x0, y0) = ((r.x * sx).max(0.0) as usize, (r.y * sy).max(0.0) as usize);
        let (x1, y1) = (
            ((r.x + r.width) * sx).min(w as f64) as usize,
            ((r.y + r.height) * sy).min(h as f64) as usize,
        );
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * w + x) * 4;
                for k in 0..3 {
                    rgba[i + k] = ((rgba[i + k] as u32 * (255 - a) + c[k] as u32 * a) / 255) as u8;
                }
            }
        }
    };
    for (plan, strong) in plans {
        let Plan::Draw { bands, outline, .. } = plan else {
            continue;
        };
        let a = if *strong { 110 } else { 70 };
        for (layer, rect) in bands {
            let c = match layer {
                crate::devtools_overlay::Layer::Margin => [246, 178, 107],
                crate::devtools_overlay::Layer::Border => [255, 229, 153],
                crate::devtools_overlay::Layer::Padding => [147, 196, 125],
                crate::devtools_overlay::Layer::Content => [111, 168, 220],
            };
            blend(&mut rgba, rect, c, a);
        }
        for edge in [
            (outline.x, outline.y, outline.width, 1.0),
            (
                outline.x,
                outline.y + outline.height - 1.0,
                outline.width,
                1.0,
            ),
            (outline.x, outline.y, 1.0, outline.height),
            (
                outline.x + outline.width - 1.0,
                outline.y,
                1.0,
                outline.height,
            ),
        ] {
            let r = wega::engine_protocol::inspector::geometry::GeomRect {
                x: edge.0,
                y: edge.1,
                width: edge.2,
                height: edge.3,
            };
            blend(&mut rgba, &r, [60, 120, 216], 255);
        }
    }
    let path = std::path::Path::new(&dir).join(name);
    if let Err(error) =
        image::save_buffer(&path, &rgba, w as u32, h as u32, image::ColorType::Rgba8)
    {
        println!("inspector-test: evidence not written: {error}");
    } else {
        println!("inspector-test: D3 evidence written: {}", path.display());
    }
}

/// A point inside the fixture's `<h1>` (body margin 8 + padding 48 + the heading's margin).
const POINT: (f32, f32) = (100.0, 95.0);

#[derive(Default)]
pub struct D3 {
    /// Stage to continue with when this pass ends.
    pub resume: u8,
    pub pass: u8,
    waited_since: Option<Instant>,
    h1: Option<u64>,
    red_before: usize,
    queued: bool,
    /// Evaluation was live when the code-running edit was submitted.
    eval_live: bool,
}

fn red_pixels(frame: &(u32, u32, std::sync::Arc<[u8]>)) -> usize {
    frame
        .2
        .chunks_exact(4)
        .filter(|p| p[0] == 255 && p[1] == 0 && p[2] == 0)
        .count()
}
/// Pixels that could only come from the overlay palette (never page content in the fixture).
fn overlay_pixels(frame: &(u32, u32, std::sync::Arc<[u8]>)) -> usize {
    frame
        .2
        .chunks_exact(4)
        .filter(|p| {
            matches!(
                (p[0], p[1], p[2]),
                (246, 178, 107)
                    | (255, 229, 153)
                    | (147, 196, 125)
                    | (111, 168, 220)
                    | (60, 120, 216)
            )
        })
        .count()
}

impl crate::browser::Browser {
    fn d3_set_stage(&mut self, stage: u8) {
        println!("inspector-test: D3 stage {stage}");
        self.harness.inspector_test.as_mut().unwrap().stage = stage;
    }
    fn d3(&mut self) -> &mut D3 {
        &mut self.harness.inspector_test.as_mut().unwrap().d3
    }
    fn d3_timeout(&mut self, what: &str) {
        let since = *self.d3().waited_since.get_or_insert_with(Instant::now);
        assert!(
            since.elapsed() < Duration::from_secs(20),
            "D3 acceptance stalled waiting for: {what}"
        );
    }
    fn d3_advance(&mut self, stage: u8) {
        self.d3().waited_since = None;
        self.d3_set_stage(stage);
    }
    fn d3_screen(&self, css: (f32, f32)) -> egui::Pos2 {
        let zoom = self.tab().wega.as_ref().map_or(1.0, |p| p.zoom);
        self.chrome.page_rect.min + egui::vec2(css.0 * zoom, css.1 * zoom)
    }
    fn d3_inject(&mut self, events: Vec<egui::Event>) {
        let gui = self.gui.as_mut().expect("native window");
        gui.egui_winit.egui_input_mut().events.extend(events);
        self.window.request_redraw();
    }
    fn d3_console_has(&self, prefix: &str) -> Option<String> {
        use crate::devtools_console::ConsoleLine;
        self.devtools.console.lines().find_map(|line| match line {
            ConsoleLine::Message(m) if m.text.starts_with(prefix) => Some(m.text.clone()),
            _ => None,
        })
    }

    pub(crate) fn d3_stage(&mut self, stage: u8) {
        self.window.request_redraw();
        let Some(document) = self.devtools_document() else {
            return;
        };
        match stage {
            // Open the Elements panel and wait for a loaded tree.
            30 => {
                self.tools.elements_tab = true;
                self.tools.console_tab = false;
                if self.devtools_snapshot().is_none()
                    || self.devtools.elements.root.is_none()
                    || self.devtools.elements.epoch.is_none()
                    || self.chrome.page_rect.width() < 10.0
                {
                    return self.d3_timeout("the Elements tree and page rectangle");
                }
                // Page-side probes: any input event the page sees is counted.
                self.tab().wega.as_ref().unwrap().evaluate(
                    "window.__in={};for(const t of ['mousemove','mousedown','mouseup','click','pointermove','pointerdown','keydown','keyup'])document.addEventListener(t,()=>{__in[t]=(__in[t]||0)+1},true);console.log('PROBE ready')",
                );
                // Editing starts disabled: an edit is refused in the browser.
                assert!(
                    !self.devtools.edit.is_enabled(),
                    "editing must start disabled"
                );
                self.devtools.geometry.set_picker(true);
                assert!(self.devtools_picker_active());
                self.d3_advance(31);
            }
            // Hover over the heading with a real (injected) pointer event.
            31 => {
                if self.d3_console_has("PROBE ready").is_none() {
                    return self.d3_timeout("the page probe");
                }
                let pos = self.d3_screen(POINT);
                self.d3_inject(vec![egui::Event::PointerMoved(pos)]);
                self.d3_advance(32);
            }
            32 => {
                let Some(hover) = self.devtools.geometry.hover.clone() else {
                    let pos = self.d3_screen(POINT);
                    self.d3_inject(vec![egui::Event::PointerMoved(pos)]);
                    return self.d3_timeout("a hover pick");
                };
                let boxes = hover.boxes.expect("hover geometry has boxes");
                let b = boxes.border_box;
                assert!(
                    (POINT.0 as f64) >= b.x
                        && (POINT.0 as f64) <= b.x + b.width
                        && (POINT.1 as f64) >= b.y
                        && (POINT.1 as f64) <= b.y + b.height,
                    "the picked element's box {b:?} does not contain the pointer {POINT:?}"
                );
                // The overlay would draw the hover highlight over exactly that box.
                let plans = self.devtools_overlay_plans();
                assert!(
                    plans.iter().any(|(plan, strong)| !strong
                        && matches!(plan, Plan::Draw { outline, .. }
                            if (POINT.0 as f64) >= outline.x && (POINT.0 as f64) <= outline.x + outline.width
                            && (POINT.1 as f64) >= outline.y && (POINT.1 as f64) <= outline.y + outline.height)),
                    "no hover highlight at the pointer: {plans:?}"
                );
                let pass = self.d3().pass;
                if let Some(frame) = self
                    .tab()
                    .wega
                    .as_ref()
                    .and_then(|p| p.state.lock().ok()?.last_frame.clone())
                    && let Some(snapshot) = self.devtools_snapshot()
                    && let Some(v) = snapshot.viewport
                {
                    evidence(
                        &format!("d3-pass{pass}-hover-highlight.png"),
                        &frame,
                        (v.width, v.height),
                        &plans,
                    );
                }
                // A real click selects it.
                let pos = self.d3_screen(POINT);
                self.d3_inject(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
                self.d3_advance(33);
            }
            // The click ended picking and selected the same element in the tree.
            33 => {
                let selected = self.devtools.elements.selected;
                let ready = !self.devtools.geometry.picker
                    && selected.is_some_and(|s| {
                        self.devtools.elements.rows.contains_key(&s.key)
                            && self
                                .devtools
                                .geometry
                                .selected
                                .as_ref()
                                .is_some_and(|g| g.node == s)
                            && self.devtools.styles.page.is_some()
                    });
                if !ready {
                    return self
                        .d3_timeout("selection, geometry and styles of the clicked element");
                }
                let id = selected.unwrap();
                let row = &self.devtools.elements.rows[&id.key];
                assert_eq!(row.node.name, "h1", "the click selected the wrong element");
                assert_eq!(id.document, document);
                // Box Model and Styles/Computed are synchronised with the selection.
                let geometry = self.devtools.geometry.selected.clone().unwrap();
                assert!(geometry.boxes.is_some(), "{:?}", geometry.state);
                self.d3().h1 = Some(id.key);
                // The page saw none of it, and no key press.
                self.tab()
                    .wega
                    .as_ref()
                    .unwrap()
                    .evaluate("console.log('IN:'+JSON.stringify(__in))");
                self.d3_advance(34);
            }
            34 => {
                let Some(text) = self.d3_console_has("IN:") else {
                    return self.d3_timeout("the input probe");
                };
                assert_eq!(text, "IN:{}", "the picker leaked input to the page: {text}");
                // Escape cancels a fresh picker session before any other handler sees it.
                self.devtools.geometry.set_picker(true);
                assert!(self.shortcut_key(&Key::Named(NamedKey::Escape), ElementState::Pressed));
                assert!(!self.devtools.geometry.picker);
                assert!(self.devtools.geometry.swallow_escape_release);
                self.devtools.geometry.swallow_escape_release = false;
                // The button toggles it off again.
                self.devtools.geometry.set_picker(true);
                self.devtools.geometry.set_picker(false);
                assert!(!self.devtools_picker_active());
                self.d3_advance(35);
            }
            // Editing: refused until the user enables it, then applied through the engine.
            35 => {
                let id = wega::engine_protocol::inspector::dom::InspectorNodeId {
                    document,
                    key: self.d3().h1.unwrap(),
                };
                let attr = || MutationOp::SetAttribute {
                    name: "data-d3".into(),
                    value: "1".into(),
                };
                assert!(
                    !self.devtools.edit.submit(id, attr()),
                    "edit accepted while disabled"
                );
                assert!(self.devtools_picker_active() == false);
                self.devtools_enable_editing(false);
                self.d3_advance(36);
            }
            36 => {
                if !self.devtools.edit.is_enabled() {
                    return self.d3_timeout("the editing grant");
                }
                let key = self.d3().h1.unwrap();
                let id = wega::engine_protocol::inspector::dom::InspectorNodeId { document, key };
                // Expand the heading to reach its text node.
                if let Some(row) = self.devtools.elements.rows.get_mut(&key) {
                    row.expanded = true;
                }
                let text = self
                    .devtools
                    .elements
                    .rows
                    .get(&key)
                    .map(|row| row.children.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .find_map(|child| {
                        let row = self.devtools.elements.rows.get(&child)?;
                        (row.node.node_type == 3).then_some(row.node.id)
                    });
                let Some(text) = text else {
                    return self.d3_timeout("the heading's text node");
                };
                if !self.d3().queued {
                    self.d3().queued = true;
                    let eval = self.devtools.console.evaluation_enabled(document).is_some();
                    self.d3().eval_live = eval;
                    self.d3().red_before = self
                        .tab()
                        .wega
                        .as_ref()
                        .and_then(|p| p.state.lock().ok()?.last_frame.as_ref().map(red_pixels))
                        .unwrap_or(0);
                    let edits = &mut self.devtools.edit;
                    assert!(edits.submit(
                        id,
                        MutationOp::SetAttribute {
                            name: "data-d3".into(),
                            value: "1".into()
                        }
                    ));
                    assert!(edits.submit(
                        text,
                        MutationOp::SetText {
                            text: "Edited by DevTools".into()
                        }
                    ));
                    assert!(edits.submit(
                        id,
                        MutationOp::SetInlineProperty {
                            property: "background-color".into(),
                            value: "#ff0000".into(),
                            important: true,
                        }
                    ));
                    // An edit the engine must refuse (an event handler needs the evaluation grant).
                    assert!(edits.submit(
                        id,
                        MutationOp::SetAttribute {
                            name: "onclick".into(),
                            value: "1".into()
                        }
                    ));
                }
                if self.devtools.edit.busy() {
                    return self.d3_timeout("the edits to be applied");
                }
                // The code-running edit (onclick) needs the evaluation grant too: applied while
                // evaluation is live (first pass), refused with a visible reason otherwise.
                let eval_live = self.d3().eval_live;
                if eval_live {
                    assert_eq!(
                        self.devtools.edit.last.as_deref(),
                        Some("Set attribute onclick")
                    );
                } else {
                    assert!(
                        self.devtools.edit.notice.contains("evaluation"),
                        "notice {:?}, last {:?}",
                        self.devtools.edit.notice,
                        self.devtools.edit.last
                    );
                }
                self.tab().wega.as_ref().unwrap().evaluate(
                    "{const h=document.querySelector('h1');console.log('V:'+h.getAttribute('data-d3')+'|'+h.textContent+'|'+h.style.backgroundColor+'|'+h.hasAttribute('onclick'))}",
                );
                self.d3_advance(37);
            }
            37 => {
                let Some(text) = self.d3_console_has("V:") else {
                    return self.d3_timeout("the verification message");
                };
                let parts: Vec<_> = text.trim_start_matches("V:").split('|').collect();
                assert_eq!(parts[0], "1", "attribute edit: {text}");
                assert_eq!(parts[1], "Edited by DevTools", "text edit: {text}");
                assert!(
                    parts[2].contains("255, 0, 0") || parts[2].contains("#ff0000"),
                    "style edit: {text}"
                );
                let eval_live = self.d3().eval_live;
                assert_eq!(
                    parts[3],
                    eval_live.to_string(),
                    "the code-running edit must apply exactly when evaluation is live: {text}"
                );
                // The rendered page changed, measured on the engine's own pixels.
                let frame = self
                    .tab()
                    .wega
                    .as_ref()
                    .and_then(|p| p.state.lock().ok()?.last_frame.clone());
                let Some(frame) = frame else {
                    return self.d3_timeout("a frame");
                };
                let before = self.d3().red_before;
                let red = red_pixels(&frame);
                if red < before + 1000 {
                    return self.d3_timeout("the edited heading to be painted red");
                }
                // The page's pixels never contain the DevTools overlay.
                assert_eq!(
                    overlay_pixels(&frame),
                    0,
                    "overlay leaked into the page frame"
                );
                let pass = self.d3().pass;
                let plans = self.devtools_overlay_plans();
                if let Some(v) = self.devtools_snapshot().and_then(|s| s.viewport) {
                    evidence(
                        &format!("d3-pass{pass}-after-edits.png"),
                        &frame,
                        (v.width, v.height),
                        &plans,
                    );
                }
                println!(
                    "inspector-test: D3 rendered page changed: {before} -> {red} red pixels; overlay pixels in page frame: 0"
                );
                self.d3_advance(38);
            }
            // Revocation is immediate, in the browser and for any later attempt.
            38 => {
                let key = self.d3().h1.unwrap();
                let id = wega::engine_protocol::inspector::dom::InspectorNodeId { document, key };
                self.devtools_revoke_editing();
                assert!(!self.devtools.edit.is_enabled());
                assert!(!self.devtools.edit.submit(id, MutationOp::RemoveNode));
                let session = self.devtools.session().expect("session");
                let page = self.tab().wega.as_ref().unwrap();
                let epoch = self.devtools.elements.epoch.unwrap_or(0);
                // No token exists any more: nothing is sent.
                let refused = page.inspector_mutate(session, id, epoch, MutationOp::RemoveNode);
                assert_eq!(refused.unwrap_err().code, InspectorErrorCode::Unauthorized);
                // The generic entry point refuses to carry a mutation at all, forged token or not.
                let forged = page.inspector_request_args(
                    session,
                    InspectorCommand::Mutate,
                    InspectorArgs::Mutate {
                        grant: GrantToken([7; 16]),
                        node: id,
                        epoch,
                        op: MutationOp::RemoveNode,
                    },
                );
                assert_eq!(forged.unwrap_err().code, InspectorErrorCode::Unauthorized);
                let forged = page.inspector_request_args(
                    session,
                    InspectorCommand::GrantMutation,
                    InspectorArgs::Grant {
                        token: GrantToken([7; 16]),
                        opaque_override: true,
                        origin: None,
                    },
                );
                assert_eq!(forged.unwrap_err().code, InspectorErrorCode::Unauthorized);
                self.d3().queued = false;
                println!(
                    "inspector-test: D3 pass {} passed: pick, hover/selection highlight, tree sync, box model, styles, authorized attribute/text/inline-CSS edits, rendered change, revocation, forged-grant refusal",
                    self.d3().pass
                );
                let resume = self.d3().resume;
                self.d3().pass += 1;
                self.d3_advance(resume);
            }
            _ => {}
        }
    }
}
