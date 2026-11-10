# Step 11 Contract: Complete Input, Native Controls and Accessibility Transport

## 1. Overview & Objectives

Step 11 fulfills the input, form control lifecycle, file selection capability grant, and native accessibility requirements defined in the Wega standalone engine execution plan:
- Full pointer, keyboard, and space-to-toggle interactions on form controls (checkbox, radio, text, button).
- Form validation and form reset semantics (`HTMLFormElement.prototype.reset()` and `resetForm` native command restoring `defaultValue` and `defaultChecked`).
- Disabled attribute enforcement across mouse, keyboard, and focus events.
- Browser-mediated file selection dialogs (`ContentEvent::FileDialog`, `PageEvent::FileDialog`, `Command::FileSelected`) with path capability confinement — canceling dialog leaks zero grant, and selected files feed real multipart/form-data payloads.
- Native AccessKit accessibility tree building and action dispatch (`ContentEvent::Accessibility`, `Command::EnableAccessibility`, `Command::AccessibilityAction`).

## 2. Architecture & Protocol Additions

### Protocol Layer (`engines/protocol/src/lib.rs`)
- **Commands:**
  - `Command::EnableAccessibility(bool)`: Activates accessibility tree updates in the engine worker.
  - `Command::AccessibilityAction { action: String, target: u64, data: Option<String> }`: Forwards native AccessKit actions (focus, click/default, setValue) to Blitz DOM nodes.
  - `Command::FileSelected { id: u64, paths: Vec<PathBuf> }`: Grants access to user-selected file paths for a specific pending file dialog.
- **Events:**
  - `ContentEvent::Accessibility(String)`: Serialized AccessKit `TreeUpdate` JSON payload sent from worker to browser host.
  - `ContentEvent::FileDialog { id: u64, multiple: bool, accept: Vec<String> }`: Dialog request dispatched when `<input type="file">` is clicked.

### Engine View & DOM Integration (`engines/wega/src/view.rs`, `dom.rs`)
- `blitz-dom` integrated with `accessibility` and `file-input` features and AccessKit `0.25.2`.
- `View::accessibility_tree()` resolves document styles before generating `accesskit::TreeUpdate` to ensure layout styles are valid.
- `Dom::reset_form_controls(doc, form)` traverses form descendants, resetting `input` and `textarea` controls to their initial DOM attributes.
- `HTMLInputElement.prototype.files` returns a structured `FileList` proxy with native `File` objects constructed upon host response via `globalThis.__wega_native_file_input(id, fileNames)`.
- Multipart form encoding (`encode_body`) supports `EntryValue::File(path)` and reads file contents directly with MIME multipart boundaries.

### Browser Host Integration (`src/wega_page.rs`, `src/accessibility.rs`, `src/wega_ui.rs`)
- `Snapshot` tracks accessibility enabled state and pending file dialog request state.
- `src/accessibility.rs` connects `WindowEvent::InitialTreeRequested` and `WindowEvent::ActionRequested` directly to the active Wega page.
- `src/wega_ui.rs` receives `ContentEvent::Accessibility` and applies updates to `gui.egui_winit.accesskit`.
- `src/wega_ui.rs` receives `ContentEvent::FileDialog`, invokes native file picker (`rfd::FileDialog`), and replies with `Page::select_files`.

## 3. Verification & Evidence

- Unit tests in `engines/wega/src/view.rs`:
  - `script_types_follow_the_html_mime_list` — PASSED
  - `form_bodies_are_encoded_for_their_enctype` — PASSED
  - `form_bodies_support_file_entries_multipart` — PASSED
  - `file_dialog_canceling_leaks_no_grant` — PASSED
  - `accessibility_tree_generates_valid_json` — PASSED
- Protocol regression suite:
  - `tests::input_controls_and_accessibility_transport` — PASSED
- Browser Host Compilation:
  - `cargo check --bin wega` — PASSED (0 errors)
