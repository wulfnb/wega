# Step 23 Contract: Standalone Default and Unified Workspace

## 1. Overview & Objectives

Step 23 establishes the Wega standalone engine as the default runtime across the application, migrates stored engine selections safely without data loss, removes automatic runtime fallback to Servo, and provides navigation history traversal parity for developer tooling and automation:

- **Wega Standalone as Default Engine (`src/engine_choice.rs`)**:
  - `EngineKind::default()` defaults to `EngineKind::Wega`.
  - `wega::engine_choice::choose()` chooses `EngineKind::Wega` when the standalone content helper executable is present.
  - UI engine picker labels update `Wega` to the primary engine and marks `Servo` as `Servo (legacy)`.
  - Unit tests updated and verified for default standalone engine resolution.

- **Safe Profile Normalization (`src/profile.rs`)**:
  - `Profile::normalize()` migrates saved tab engine preferences to `EngineKind::Wega` automatically.
  - Zero data loss: Preserves all tab URLs, tab titles, active selection index, history items, and bookmark entries during migration.
  - Tested and verified in `src/profile_tests.rs`.

- **UI Streamlining (`src/wega_ui.rs`)**:
  - `engine_badge` renders a static chip `Wega` instead of an interactive dropdown menu selector.
  - Engine selection dropdown removed from standard chrome toolbar.

- **Elimination of Automatic Runtime Fallback (`src/browser.rs`)**:
  - Removed automatic fallback to Servo in `push_tab`, `load_tab`, and `dispatch_navigation_confirmed`.
  - Errors during Wega worker startup or navigation are surfaced directly via `self.status` rather than silently switching engine backends.

- **Navigation History Traversal & Automation Parity (`src/wega_page.rs`, `src/browser.rs`, `src/automation.rs`)**:
  - `Page::history_target(&self, forward: bool)` and `Browser::history_target(&self, forward: bool)` compute expected navigation destinations for backward and forward traversals.
  - `src/automation.rs` uses `self.history_target(...)` to set expected targets for `Command::GoBack` and `Command::GoForward`.
  - `TakeScreenshot` polls the engine frame buffer with bounded timeout, eliminating race conditions between frame rendering intervals and screenshot capture.

---

## 2. Verification & Test Suite

1. **WebDriver Full Test Suite (`tests/webdriver_smoke.py`)**:
   - `WEGA_UNSAFE_DEVELOPMENT_WORKER=1 WEGA_TEST_ALLOW_LOOPBACK=1 python3 scripts/native-tests.py --binary target/debug/wega --suite webdriver`
   - PASSED 100%:
     - W3C HTTP sessions & capability rejection
     - URL navigation (`POST /url`), URL retrieval (`GET /url`), title retrieval (`GET /title`)
     - Synchronous script execution with complex arguments and return structures (`POST /execute/sync`)
     - Window handles inspection and switching (`GET /window`, `GET /window/handles`, `POST /window`)
     - History traversal: `POST /back`, `POST /forward`, `POST /refresh`, and end-of-history no-op forward
     - Base64 PNG viewport screenshot capture (`GET /screenshot`)
     - Unsupported command error reporting (`POST /element` -> 500 unsupported operation)
     - Clean session deletion (`DELETE /session/{id}`)

2. **Wega Binary Unit & Integration Tests**:
   - `cargo test --bin wega`: 50 passed, 0 failed.

3. **Core Library Tests**:
   - `cargo test --lib`: 111 passed, 0 failed.

4. **Engines Workspace Tests**:
   - `cargo test --manifest-path engines/Cargo.toml`:
     - `wega-protocol`: 39 passed, 0 failed.
     - `wega-platform`: 18 passed, 0 failed.
     - `browser-resources`: 7 passed, 0 failed.
     - `wega-engine`: all tests passed.

5. **Compilation Cleanliness**:
   - `cargo check --bin wega`: 0 errors, 0 warnings.
