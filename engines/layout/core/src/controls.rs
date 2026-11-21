//! Shared native control presentation. The document retains the sole editor state.
use crate::LayoutBox;
use wega_dom::{NodeKey, layout::Rect};

/// A borrowed view of a live control; no retained editor or DOM copy.
#[derive(Clone)]
pub struct ControlView<'a> {
    pub value: Option<std::borrow::Cow<'a, str>>,
    pub selection: (usize, usize),
    pub checked: Option<bool>,
    pub focused: bool,
    pub placeholder: Option<&'a str>,
}
pub fn sync<'a>(b: &mut LayoutBox, control: &impl Fn(NodeKey) -> Option<ControlView<'a>>) {
    use crate::FormControlKind;
    if let Some(node_key) = b.node
        && let Some(input) = control(node_key)
    {
        let is_focused = input.focused;
        if let Some(ref mut form) = b.form_state {
            form.is_focused = is_focused;
            if let Some(checked) = input.checked {
                form.checked = checked;
            }
            if let Some(editor_text) = input.value {
                let is_pwd = matches!(
                    form.kind,
                    Some(FormControlKind::TextInput {
                        is_password: true,
                        ..
                    })
                );
                let ph = input.placeholder;
                let (disp, is_ph) = if !editor_text.is_empty() {
                    if is_pwd {
                        ("•".repeat(editor_text.chars().count()), false)
                    } else {
                        (editor_text.to_string(), false)
                    }
                } else if let Some(placeholder) = ph {
                    (placeholder.to_string(), true)
                } else {
                    (String::new(), false)
                };
                b.children.clear();
                if !disp.is_empty() {
                    let mut child_style = b.style.clone();
                    if is_ph {
                        child_style.color = crate::Color {
                            r: 117,
                            g: 117,
                            b: 117,
                            a: 255,
                        };
                    }
                    let mut text_child =
                        LayoutBox::new(Some(node_key), crate::BoxType::Text, child_style);
                    text_child.text_content = Some(disp);
                    b.children.push(text_child);
                }
            }
        }
    }
    for child in &mut b.children {
        sync(child, control);
    }
}

pub fn decorate<'a>(b: &mut LayoutBox, control: &impl Fn(NodeKey) -> Option<ControlView<'a>>) {
    use crate::FormControlKind;
    if let Some(input) = b.node.and_then(control)
        && let Some(full_text) = input.value
    {
        let (is_pwd, is_focused) = if let Some(ref form) = b.form_state {
            (
                matches!(
                    form.kind,
                    Some(FormControlKind::TextInput {
                        is_password: true,
                        ..
                    })
                ),
                form.is_focused,
            )
        } else {
            (false, false)
        };
        if b.form_state.is_some() {
            let (a, f) = input.selection;
            let (sel_min, sel_max) = (a.min(f), a.max(f));
            let cb = b.content_box();
            let mut caret_rect = None;
            let mut selection_rects = Vec::new();

            if b.lines.is_empty() {
                if is_focused {
                    let h = (16.0_f64).min(cb.height);
                    caret_rect = Some(Rect {
                        x: cb.x + 2.0,
                        y: cb.y + ((cb.height - h) / 2.0).max(0.0),
                        width: 1.5,
                        height: h,
                    });
                }
            } else {
                let line = &b.lines[0];
                let get_x = |idx: usize| -> f64 {
                    if line.text_runs.is_empty() {
                        return cb.x;
                    }
                    let run = &line.text_runs[0];
                    if idx >= run.glyphs.len() {
                        cb.x + run.rect.x + run.rect.width
                    } else {
                        cb.x + run.rect.x + run.glyphs[idx].x as f64
                    }
                };
                let sel_min_char = if is_pwd {
                    full_text
                        .get(..sel_min)
                        .map(|s| s.chars().count())
                        .unwrap_or(0)
                } else {
                    sel_min
                };
                let sel_max_char = if is_pwd {
                    full_text
                        .get(..sel_max)
                        .map(|s| s.chars().count())
                        .unwrap_or(0)
                } else {
                    sel_max
                };
                let focus_char = if is_pwd {
                    full_text.get(..f).map(|s| s.chars().count()).unwrap_or(0)
                } else {
                    f
                };
                if is_focused {
                    let cx = get_x(focus_char);
                    caret_rect = Some(Rect {
                        x: cx,
                        y: cb.y + line.rect.y,
                        width: 1.5,
                        height: line.rect.height,
                    });
                }
                if sel_min_char < sel_max_char {
                    let x0 = get_x(sel_min_char);
                    let x1 = get_x(sel_max_char);
                    selection_rects.push(Rect {
                        x: x0,
                        y: cb.y + line.rect.y,
                        width: (x1 - x0).max(1.0),
                        height: line.rect.height,
                    });
                }
            }

            if let Some(ref mut form) = b.form_state {
                form.caret_rect = caret_rect;
                form.selection_rects = selection_rects;
            }
        }
    }
    for child in &mut b.children {
        decorate(child, control);
    }
}
