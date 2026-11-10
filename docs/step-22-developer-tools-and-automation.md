# Step 22 Contract: Developer Tools and Automation Endpoints

## 1. Overview & Objectives

Step 22 ports every existing native tool and automation endpoint to the Wega standalone engine, ensuring complete developer tool parity across both Servo and Wega runtimes:
- **W3C WebDriver Bridge (`src/automation.rs`)**:
  - Full support for all baseline WebDriver commands on both Servo and Wega:
    - Session management: `Command::NewSession`, `Command::DeleteSession`, `Command::Status`.
    - Window management: `Command::GetWindowHandle`, `Command::GetWindowHandles`, `Command::SwitchToWindow`.
    - Navigation: `Command::Get`, `Command::GoBack`, `Command::GoForward`, `Command::Refresh`.
    - Document inspection: `Command::GetCurrentUrl`, `Command::GetTitle`, `Command::GetPageSource`.
    - JavaScript execution: `Command::ExecuteScript` with full JSON argument serialization and exception mapping.
    - Visual inspection: `Command::TakeScreenshot` generating valid base64-encoded PNGs from the engine frame buffer.
    - Engine resources: `Command::Extension` (`/session/{id}/wega/resources`) reporting live process metrics (`engine`, `pid`, `workerRssBytes`, `jsHeapBytes`, `frozen`, `loading`, `phase`, `compatibility`).
    - Unsupported command rejection: Returning standard W3C `unsupported operation` errors without crashing or disconnecting.
- **Real Load Completion & Cancellation**:
  - `PendingNavigation` updated to track both Servo webviews (`owner: Option<servo::WebViewId>`) and Wega tabs (`tab_key: u64`).
  - Correlates navigation completion via `wega::navigation::Completion`, completing when `!tab.loading()` becomes true.
  - Window closure or navigation cancellation during in-flight navigation invalidates handles and terminates pending commands with `no such window` / `unknown error`.
- **Synchronous Script Evaluation Bridge (`src/wega_page.rs`)**:
  - `evaluate_sync_parts`: Thread-safe evaluation helper that dispatches `Request::EvaluateDocument` to the worker process and awaits `Event::DocumentEvaluated` within a bounded timeout without holding snapshot locks.
  - `Page::eval_parts()`: Exposes `(mpsc::SyncSender<Request>, Arc<Mutex<Snapshot>>)` for background thread dispatch.
- **Native Developer Tools UI (`src/devtools_ui.rs`, `src/tool_window.rs`, `src/browser.rs`)**:
  - Native developer tools panel displays engine metrics (URL, Title, PID, Worker RSS, JS Heap, DOM Nodes, Loading status, Lifecycle phase, Compatibility) when active on a Wega tab.
  - Safe against internal and unloaded tabs (`self.tab().id()`, `self.tab().view()`, `self.tab().is_internal()`), eliminating panics when toggling or inspecting Wega documents.
  - F12 shortcut and developer tools menu toggle cleanly in docked and detached window modes.

---

## 2. Verification & Test Suite

1. **WebDriver Integration Suite (`tests/webdriver_smoke.py`)**:
   - Verified against live HTTP server on port 8767 (`capabilities.html`, `second.html`).
   - Verified against both Servo and Wega standalone engines (`--engine wega`):
     - W3C HTTP sessions & capability negotiation.
     - Extension resources check reporting Wega metrics.
     - Document navigation (`POST /url`), URL retrieval (`GET /url`), and title retrieval (`GET /title`).
     - Synchronous script execution (`POST /execute/sync`) with arguments and object returns.
     - Page source retrieval (`GET /source`).
     - Window handles (`GET /window`, `GET /window/handles`).
     - History traversal (`POST /back`, `POST /forward`, `POST /refresh`).
     - Base64-encoded PNG screenshot verification (`GET /screenshot`).
     - Unsupported command rejection (`POST /element` -> `unsupported operation`).
     - Session teardown (`DELETE /session/{id}`).
2. **Engines Workspace Tests**:
   - All tests in `engines/Cargo.toml` pass (39 in `wega-protocol`, 18 in `wega-network`, 7 in `wega-platform`, 29 in `wega-engine`).
3. **Compilation**:
   - `cargo check --bin wega` compiles cleanly with zero warnings or errors.
