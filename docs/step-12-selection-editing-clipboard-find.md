# Step 12 Contract: Implement Selection, Editing, Clipboard, Find and Drag/Drop

## 1. Overview & Objectives

Step 12 completes rich document interactions, selection geometry, content editing, browser-mediated clipboard transport, in-page find, and drag-and-drop operations per the standalone engine execution plan:
- Full W3C `Range` and `Selection` DOM APIs supporting multi-node selection, boundary comparison, content deletion/extraction/cloning, and bounding box computation.
- Editable document and element support via `contenteditable` and `isContentEditable` reflection, `document.designMode`, and standard editing command dispatch (`document.execCommand`, `document.queryCommandSupported`, etc.).
- Complete `TreeWalker`, `NodeIterator`, and `NodeFilter` traversal interfaces used by both scripts and the find subsystem.
- Browser-mediated clipboard commands and events (`Command::Copy`, `Command::Cut`, `Command::Paste`, `ContentEvent::ClipboardWrite`), plus `navigator.clipboard.writeText` and `readText`.
- Native and in-page find operations (`window.find`, `Command::FindInPage`, `ContentEvent::FindResult`) connected directly to the browser chrome find bar for Wega tabs.
- `DataTransfer`, `DragEvent`, and `ClipboardEvent` infrastructure for HTML5 drag-and-drop and clipboard events.

## 2. Architecture & Protocol Additions

### Protocol Layer (`engines/protocol/src/lib.rs`)
- **Commands:**
  - `Command::Copy`: triggers copy action on active selection, emitting `ContentEvent::ClipboardWrite`.
  - `Command::Cut`: copies active selection to clipboard and removes selected content from the document.
  - `Command::Paste(String)`: inserts text at active selection/caret.
  - `Command::SelectAll`: selects all contents of the document or active editable container.
  - `Command::FindInPage { query: String, backwards: bool, case_sensitive: bool }`: searches for query string, updates selection, and emits `ContentEvent::FindResult`.
- **Events:**
  - `ContentEvent::ClipboardWrite(String)`: requests host window to write text to system clipboard.
  - `ContentEvent::FindResult { active_match_ordinal: u32, number_of_matches: u32, query: String }`: reports search results and active match index to the host find bar.

### Engine View & DOM Integration (`engines/wega/src/view.rs`, `dom.rs`, `semantics.js`)
- `Range` extended with:
  - `deleteContents()`, `extractContents()`, `cloneContents()`, `surroundContents(newParent)`.
  - `getBoundingClientRect()` and `getClientRects()` computing layout bounding boxes.
- `Selection` class implemented:
  - `anchorNode`, `anchorOffset`, `focusNode`, `focusOffset`, `isCollapsed`, `rangeCount`, `type`.
  - `getRangeAt(index)`, `addRange(range)`, `removeRange(range)`, `removeAllRanges()`, `empty()`.
  - `collapse(node, offset)`, `setPosition(node, offset)`, `collapseToStart()`, `collapseToEnd()`.
  - `extend(node, offset)`, `setBaseAndExtent(anchorNode, anchorOffset, focusNode, focusOffset)`.
  - `selectAllChildren(node)`, `deleteFromDocument()`, `containsNode(node, allowPartial)`, `toString()`.
- `HTMLElement.prototype.contentEditable` and `isContentEditable` implemented.
- `document.execCommand`, `queryCommandSupported`, `queryCommandEnabled`, `queryCommandState`, `queryCommandValue` implemented with support for `selectAll`, `copy`, `cut`, `paste`, `insertText`, `delete`, `forwardDelete`, `bold`, `italic`, `underline`, `undo`, `redo`.
- `TreeWalker`, `NodeIterator`, `NodeFilter` implemented and exposed on `document.createTreeWalker` and `document.createNodeIterator`.
- `DataTransfer`, `DragEvent`, `ClipboardEvent` implemented and exposed on `globalThis`.
- `navigator.clipboard.writeText` and `readText` hooked to host clipboard calls.
- `window.find` implemented using `TreeWalker` across visible text nodes, highlighting matches via `window.getSelection()`, and centering matches via `scrollIntoView`.
- Keyboard shortcuts: Ctrl/Cmd+C (copy), Ctrl/Cmd+X (cut), Ctrl/Cmd+V (paste), Ctrl/Cmd+A (select all) handled in `View::key(...)`.

### Browser Host Integration (`src/wega_page.rs`, `src/wega_ui.rs`, `src/capabilities_ui.rs`)
- `Page` exposes `copy()`, `cut()`, `paste(text)`, `select_all()`, `find_in_page(query, backwards, case_sensitive)`, and `evaluate(source)`.
- `src/wega_ui.rs` receives `ContentEvent::ClipboardWrite` and copies text to system clipboard via `arboard::Clipboard`.
- `src/wega_ui.rs` receives `ContentEvent::FindResult` and updates `Snapshot.find_result` and `features.find_status`.
- `src/capabilities_ui.rs` enables the browser find bar (`Cmd+F` / `Ctrl+F`) on Wega tabs, dispatching find commands and clearing ranges when closed.

## 3. Verification & Evidence

- Unit tests in `engines/wega/src/view.rs`:
  - `range_and_selection_dom_apis` — PASSED
  - `content_editable_and_exec_command` — PASSED
  - `find_in_page_and_tree_walker` — PASSED
  - `clipboard_and_datatransfer_apis` — PASSED
  - Prior Step 11 tests (`accessibility_tree_generates_valid_json`, `file_dialog_canceling_leaks_no_grant`, `form_bodies_support_file_entries_multipart`, etc.) — ALL PASSED (24/24 engine tests passing)
- Protocol regression suite:
  - `tests::selection_editing_find_clipboard_transport` — PASSED (33/33 protocol tests passing)
- Browser Host Compilation:
  - `cargo check --bin wega` — PASSED (0 errors)
