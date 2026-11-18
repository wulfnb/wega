//! The existing Parley editing algorithm over the sole Wega control state.
use super::*;
use crate::events::{ImeEvent, KeyEvent};
use keyboard_types::{Key, Modifiers, NamedKey};
use parley::{FontContext, LayoutContext};
#[cfg(target_os = "macos")]
const ACTION_MOD: Modifiers = Modifiers::META;
#[cfg(not(target_os = "macos"))]
const ACTION_MOD: Modifiers = Modifiers::CONTROL;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditEvent {
    Input,
    Select,
    PreEditChange,
    Submit,
}
impl TextInput {
    pub(super) fn apply_keypress_event(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<()>,
        event: KeyEvent,
    ) -> Option<EditEvent> {
        // Do nothing if it is a keyup event
        if !event.state.is_pressed() {
            return None;
        }

        let mods = event.modifiers;
        let shift = mods.contains(Modifiers::SHIFT);
        let action_mod = mods.contains(ACTION_MOD);

        let is_multiline = self.multiline;
        let editor = &mut self.editor;
        let mut driver = editor.driver(font_ctx, layout_ctx);
        match event.key {
            Key::Character(c) if action_mod && matches!(c.to_lowercase().as_str(), "a") => {
                if shift {
                    driver.collapse_selection()
                } else {
                    driver.select_all()
                }
                return Some(EditEvent::Select);
            }
            Key::Named(NamedKey::ArrowLeft) => {
                if action_mod {
                    if shift {
                        driver.select_word_left()
                    } else {
                        driver.move_word_left()
                    }
                } else if shift {
                    driver.select_left()
                } else {
                    driver.move_left()
                }
                return Some(EditEvent::Select);
            }
            Key::Named(NamedKey::ArrowRight) => {
                if action_mod {
                    if shift {
                        driver.select_word_right()
                    } else {
                        driver.move_word_right()
                    }
                } else if shift {
                    driver.select_right()
                } else {
                    driver.move_right()
                }
                return Some(EditEvent::Select);
            }
            Key::Named(NamedKey::ArrowUp) => {
                if shift {
                    driver.select_up()
                } else {
                    driver.move_up()
                }
                return Some(EditEvent::Select);
            }
            Key::Named(NamedKey::ArrowDown) => {
                if shift {
                    driver.select_down()
                } else {
                    driver.move_down()
                }
                return Some(EditEvent::Select);
            }
            Key::Named(NamedKey::Home) => {
                if action_mod {
                    if shift {
                        driver.select_to_text_start()
                    } else {
                        driver.move_to_text_start()
                    }
                } else if shift {
                    driver.select_to_line_start()
                } else {
                    driver.move_to_line_start()
                }
                return Some(EditEvent::Select);
            }
            Key::Named(NamedKey::End) => {
                if action_mod {
                    if shift {
                        driver.select_to_text_end()
                    } else {
                        driver.move_to_text_end()
                    }
                } else if shift {
                    driver.select_to_line_end()
                } else {
                    driver.move_to_line_end()
                }
                return Some(EditEvent::Select);
            }
            Key::Named(NamedKey::Delete) => {
                if !driver.editor.raw_selection().is_collapsed() {
                    driver.delete_selection();
                } else if action_mod {
                    driver.delete_word();
                } else {
                    driver.delete();
                }
                return Some(EditEvent::Input);
            }

            Key::Named(NamedKey::Backspace) => {
                if !driver.editor.raw_selection().is_collapsed() {
                    driver.delete_selection();
                } else if action_mod {
                    driver.backdelete_word();
                } else {
                    driver.backdelete();
                }
                return Some(EditEvent::Input);
            }

            Key::Character(c) if c == "\n" => {
                if is_multiline {
                    driver.insert_or_replace_selection("\n");
                    return Some(EditEvent::Input);
                } else {
                    return Some(EditEvent::Submit);
                }
            }
            Key::Named(NamedKey::Enter) => {
                if is_multiline {
                    driver.insert_or_replace_selection("\n");
                    return Some(EditEvent::Input);
                } else {
                    return Some(EditEvent::Submit);
                }
            }
            Key::Character(s)
                if !mods.contains(Modifiers::CONTROL) && !mods.contains(Modifiers::META) =>
            {
                driver.insert_or_replace_selection(&s);
                return Some(EditEvent::Input);
            }
            _ => {}
        };

        None
    }

    pub(super) fn apply_ime_event(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<()>,
        event: ImeEvent,
    ) -> Option<EditEvent> {
        let editor = &mut self.editor;
        let mut driver = editor.driver(font_ctx, layout_ctx);

        match event {
            ImeEvent::Enabled => {
                // Do nothing
                None
            }
            ImeEvent::Disabled => {
                driver.clear_compose();
                Some(EditEvent::PreEditChange)
            }
            ImeEvent::Commit(text) => {
                driver.insert_or_replace_selection(&text);
                Some(EditEvent::Input)
            }
            ImeEvent::Preedit(text, cursor) => {
                if text.is_empty() {
                    driver.clear_compose();
                } else {
                    driver.set_compose(&text, cursor);
                }
                Some(EditEvent::PreEditChange)
            }
            ImeEvent::DeleteSurrounding {
                before_bytes,
                after_bytes,
            } => {
                let _ = before_bytes;
                let _ = after_bytes;
                // TODO
                None
            }
        }
    }
}

impl WegaDocument {
    pub(crate) fn with_editor<R>(
        &mut self,
        key: NodeKey,
        edit: impl FnOnce(&mut parley::PlainEditor<()>, &mut FontContext) -> R,
    ) -> Option<R> {
        self.ensure_control(key);
        let fonts = self.fonts.clone();
        let input = self.text_input_mut(key)?;
        let result = edit(
            &mut input.editor,
            &mut fonts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        self.record_external_mutation();
        Some(result)
    }
    pub fn selected_text(&self, key: NodeKey) -> Option<String> {
        self.text_input(key)?
            .editor
            .selected_text()
            .map(str::to_owned)
    }
    pub fn set_input_value(&mut self, key: NodeKey, value: &str) -> Result<(), String> {
        if self.node(key).is_none() {
            return Err("Node no longer exists".into());
        }
        self.ensure_control(key);
        if let Some(input) = self.text_input_mut(key) {
            input.editor.set_text(value);
        }
        self.mutate()
            .set_attribute(
                key,
                QualName::new(None, html5ever::ns!(), "value".into()),
                value,
            )
            .map_err(|e| e.to_string())?;
        self.record_external_mutation();
        Ok(())
    }
    pub fn selection_range(&self, key: NodeKey) -> (usize, usize) {
        let Some(input) = self.text_input(key) else {
            return (0, 0);
        };
        let selection = input.editor.raw_selection();
        let (a, b) = (selection.anchor().index(), selection.focus().index());
        let text = input.editor.text().to_string();
        (
            text.get(..a.min(b)).map_or(0, |s| s.encode_utf16().count()),
            text.get(..a.max(b)).map_or(0, |s| s.encode_utf16().count()),
        )
    }
    pub fn set_selection_range(&mut self, key: NodeKey, start: usize, end: usize) {
        let fonts = self.fonts.clone();
        let Some(input) = self.text_input_mut(key) else {
            return;
        };
        let text = input.editor.text().to_string();
        let to_byte = |requested| {
            let mut units = 0;
            for (index, character) in text.char_indices() {
                if units >= requested {
                    return index;
                }
                units += character.len_utf16();
            }
            text.len()
        };
        let mut fonts = fonts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        input
            .editor
            .driver(&mut fonts, &mut LayoutContext::new())
            .select_byte_range(to_byte(start), to_byte(end));
        self.record_external_mutation();
    }
    pub(crate) fn edit_key(&mut self, key: NodeKey, event: KeyEvent) -> Option<EditEvent> {
        if self.focused() != Some(key) {
            return None;
        }
        let node = self.node(key)?;
        if node.attr("disabled").is_some() {
            return None;
        }
        if node.attr("readonly").is_some() {
            let mutating = match &event.key {
                Key::Character(c) => {
                    !event.modifiers.contains(ACTION_MOD)
                        || matches!(c.to_ascii_lowercase().as_str(), "x" | "v")
                }
                Key::Named(NamedKey::Backspace | NamedKey::Delete | NamedKey::Enter) => true,
                _ => false,
            };
            if mutating {
                return None;
            }
        }
        self.ensure_control(key);
        let fonts = self.fonts.clone();
        let input = self.text_input_mut(key)?;
        let result = input.apply_keypress_event(
            &mut fonts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            &mut LayoutContext::new(),
            event,
        );
        if result.is_some() {
            self.record_external_mutation();
        }
        result
    }
    pub(crate) fn edit_ime(&mut self, key: NodeKey, event: ImeEvent) -> Option<EditEvent> {
        let node = self.node(key)?;
        if self.focused() != Some(key)
            || node.attr("disabled").is_some()
            || node.attr("readonly").is_some()
        {
            return None;
        }
        self.ensure_control(key);
        let fonts = self.fonts.clone();
        let input = self.text_input_mut(key)?;
        let result = input.apply_ime_event(
            &mut fonts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            &mut LayoutContext::new(),
            event,
        );
        if result.is_some() {
            self.record_external_mutation();
        }
        result
    }
    pub fn set_selected_files(&mut self, key: NodeKey, paths: Vec<std::path::PathBuf>) {
        if self.node(key).is_some() {
            self.files.insert(key, paths);
            self.record_external_mutation();
        }
    }
    pub fn toggle_radio(&mut self, key: NodeKey) {
        let Some(node) = self.node(key) else { return };
        let name = node.attr("name").unwrap_or_default().to_owned();
        if name.is_empty() {
            self.set_checked(key, true);
            return;
        }
        let form = wega_dom::content::form_owner(self, key);
        let root = wega_dom::tree_root(self, key);
        let peers: Vec<_> = wega_dom::preorder(self, root)
            .filter(|peer| {
                *peer != key
                    && self.node(*peer).is_some_and(|n| {
                        n.element_name().is_some_and(|n| n.local == "input")
                            && n.attr("type")
                                .is_some_and(|v| v.eq_ignore_ascii_case("radio"))
                            && n.attr("name").unwrap_or_default() == name
                            && wega_dom::content::form_owner(self, *peer) == form
                    })
            })
            .collect();
        for peer in peers {
            self.set_checked(peer, false);
        }
        self.set_checked(key, true);
    }
    pub fn reset_form(&mut self, form: NodeKey) {
        let controls: Vec<_> = wega_dom::preorder(self, self.root())
            .filter(|key| wega_dom::content::form_owner(self, *key) == Some(form))
            .collect();
        for key in controls {
            let Some(node) = self.node(key) else { continue };
            let default_checked = node.attr("checked").is_some();
            let value = node.attr("value").map(str::to_owned).unwrap_or_else(|| {
                if node.element_name().is_some_and(|n| n.local == "textarea") {
                    wega_dom::content::text_content(self, key).unwrap_or_default()
                } else {
                    String::new()
                }
            });
            if let Some(input) = self.controls.get_mut(&key) {
                input.editor.set_text(&value);
                input.dirty_value = false;
            }
            if let Some(checked) = self.checked.get_mut(&key) {
                *checked = default_checked;
            }
            self.files.remove(&key);
        }
        self.record_external_mutation();
    }
    pub fn focus_order(&self) -> Vec<NodeKey> {
        let mut keys: Vec<_> = wega_dom::preorder(self, self.root())
            .filter_map(|key| {
                let node = self.node(key)?;
                let name = node.element_name()?;
                if node.attr("disabled").is_some() || node.attr("hidden").is_some() {
                    return None;
                }
                let index = node.attr("tabindex").and_then(|v| v.parse::<i32>().ok());
                if index.is_some_and(|n| n < 0) {
                    return None;
                }
                let default = matches!(name.local, "button" | "select" | "textarea" | "iframe")
                    || name.local == "input"
                        && !node
                            .attr("type")
                            .is_some_and(|v| v.eq_ignore_ascii_case("hidden"))
                    || matches!(name.local, "a" | "area") && node.attr("href").is_some();
                (default || index.is_some())
                    .then_some((index.filter(|n| *n > 0).unwrap_or(i32::MAX), key))
            })
            .collect();
        keys.sort_by_key(|(index, _)| *index);
        keys.into_iter().map(|(_, key)| key).collect()
    }
}
