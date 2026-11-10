# Step 10: Render internal pages natively and preserve chrome/viewer UX

Branch: `feature/standalone-engine-steps`. Starts after Step 09 delivery.
This document records the contract, delivery, and verification evidence for Step 10.

## Scope & Capabilities

Step 10 routes internal browser URLs to native egui views, eliminates privileged web-document dependencies in the chrome, protects internal capabilities against untrusted content, and delivers a developer Wega-only execution mode:

1. **Native Views for Internal Pages (`src/internal_pages.rs`):**
   - URL routes:
     - `wega://newtab`: native search box with configured search engine (`DuckDuckGo`, `Google`, `Custom`, etc.), keyboard shortcut hint (`⌘L` / `Ctrl+L`), shortcut tiles ("Your pages") with favicons and click navigation, and recent history items with timestamps.
     - `wega://history`: search filter input, formatted list of visited entries with timestamps, click navigation, and clear history action with user confirmation.
     - `wega://bookmarks`: search filter input, list of bookmarks with click navigation, and individual bookmark removal.
     - `wega://settings`: overview cards for appearance, browsing, and engine configuration, with direct action to open the full settings panel (`self.features.settings = true`).
     - `wega://about`: version (`env!("CARGO_PKG_VERSION")`), platform info, UI toolkit, and engine mode overview.
     - Unknown `wega://` routes: native "Page not found" with action to return to New tab.
   - All internal pages render directly via egui inside the viewport with theme-matched colors, responsive layouts, and proper scroll areas.

2. **Zero Engine Overhead & Lazy Servo Initialization:**
   - Tabs on internal pages do not initialize or start Servo (`Host`), nor do they launch an external `wega-engine` child process.
   - Servo is built lazily (`Host::deref`) only when a Servo-backed web page is loaded.
   - On browser startup (where the initial tab is `wega://newtab`), Servo is not initialized.
   - Internal pages are recognized by `Tab::is_internal()` and are considered loaded immediately (`Tab::loaded()` returns true).

3. **Untrusted Page Privilege Separation (`src/navigation.rs`, `src/engine.rs`):**
   - Content navigation allowed rules (`wega::navigation::content_navigation_allowed`):
     - `matches!(target.scheme(), "wega")` requires `from == "wega"`.
     - Untrusted web content (`http:`, `https:`, `data:`, `file:`) cannot navigate top-level tabs or frames to `wega://` pages.
     - Script popups and window openers cannot navigate to internal pages.
     - Web-initiated requests in `src/engine.rs` targeting `"wega"` are blocked and denied before reaching the host.

4. **Session, History, and Navigation Semantics:**
   - Address bar (`self.chrome.address`) accurately reflects `wega://newtab`, `wega://history`, `wega://bookmarks`, `wega://settings`, `wega://about`.
   - History stack (`wega::navigation::History`):
     - Added `push(url)`, `traverse(forward)`, `can_back()`, `can_forward()`, and `clear()`.
     - Internal pages record transitions so Back and Forward work seamlessly between internal pages and web content.
     - When navigating away from an internal page to external content, the tab transitions to the chosen engine (`Wega` or `Servo`).
   - Session serialization and restore:
     - Tabs with `wega://` URLs persist to `browser.json` and restore cleanly on startup.

5. **Developer `--wega-only` Startup Path (`src/options.rs`, `src/model.rs`, `src/engine_choice.rs`):**
   - Added `--wega-only` CLI flag.
   - Sets `Options::wega_only = true` and `Policy::wega_only = true`.
   - All new tabs and navigations default to `EngineKind::Wega`.
   - Servo is completely disabled; attempts to switch to Servo produce explicit missing-feature notifications:
     `"Servo engine is disabled in developer --wega-only mode."`
   - Actions requiring Servo (such as viewport screenshots) provide explicit notifications.

6. **Container Windows & Deployment Workflows:**
   - Preserves `--container <id>` and `--profile-dir <path>` isolation.
   - Containers launch with their own isolated profiles and render native internal pages without conflict.

## Verification Evidence

1. **Navigation Security & History Unit Tests:**
   - `src/navigation_tests.rs`:
     - Verified `history_push_traverse_and_clear_semantics` (stack ordering, back/forward bounds, truncation).
     - Verified `web_content_cannot_open_local_files_or_top_level_data_documents`:
       - Untrusted web content cannot navigate to `wega://history`.
       - Script popups cannot navigate to `wega://` URLs.
       - Internal pages can link to each other.
   - All 5 tests passed: `cargo test --lib navigation::tests`.

2. **Engine Choice Unit Tests:**
   - `src/engine_choice.rs`:
     - Verified `wega://settings` chooses `EngineKind::Wega` without forcing Servo.
     - Verified `policy.wega_only` directs all navigations to `EngineKind::Wega`.
   - All 2 tests passed: `cargo test --lib engine_choice::tests`.

3. **CLI Options Unit Tests:**
   - `src/options_tests.rs`:
     - Added `wega_only_option_configures_engine_choice`.
     - Verified `--wega-only` sets `options.wega_only = true` and `options.engine = Some(EngineKind::Wega)`.
   - All 9 tests passed: `cargo test --lib options::tests`.

4. **Protocol & Engine Suite:**
   - `cargo test -p wega-protocol --manifest-path engines/Cargo.toml`: 31 tests passed.
   - `cargo check --lib`: passed.
   - `cargo check --bin wega`: passed.
   - `cargo check --bin wega-engine --manifest-path engines/Cargo.toml`: passed.
   - `python3 -m compileall -q scripts tests`: passed.
