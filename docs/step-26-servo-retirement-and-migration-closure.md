> Historical migration record. Earlier retirement and validation claims in this document do not establish the current implementation state. See [the current removal checkpoint](servo-removal-checkpoint.md) and the repository README.

# Step 26 Contract: Servo Retirement and Migration Closure

**Current status, 11 November 2026: browser feature completion remains open.**
The default-runtime switch described below does not establish browser parity.
The [current implementation and remaining stages](browser-completion.md) and
the blockers in `validation/standalone/progress.json` supersede the original
closure claims and historical test summary on this page.

## 1. Overview & Objectives

Step 26 represents the final milestone of the Wega standalone engine execution plan (`docs/standalone-engine-execution-plan.md`). It closes the migration by formally retiring the embedded Servo research backend, eliminating all runtime fallback paths and obsolete engine switching controls, preserving reusable native tooling and components (Stylo CSS, Blitz DOM, native media, containers, downloads, and developer tools), and documenting the complete multi-process architecture.

- **Standalone Wega Engine as Sole Runtime**:
  - The application operates exclusively on the independent multi-process Wega engine (`wega-engine`).
  - Architecture: Blitz DOM, Stylo CSS, V8 JavaScript runtime (via `deno_core`), Skia 2D rendering, and native OS sandboxing (Landlock/seccomp on Linux, Apple App Sandbox on macOS, AppContainer on Windows).
  - Zero runtime Servo initialization: During all baseline product flows, normal browsing, automated testing, and multi-tab sessions, Servo is never initialized (`servoInitialized` remains `false`).

- **Elimination of Runtime Fallbacks and Legacy UI Controls**:
  - Automatic fallback to Servo completely removed from tab creation, page loading, navigation dispatch, and compatibility reporting.
  - Error messages and status notices cleaned up across `src/wega_page.rs`, `src/wega_ui.rs`, `src/main.rs`, and `src/capabilities_ui.rs` (removing all legacy "switch to Servo" and "requires Servo" alerts).
  - UI engine indicator rendered as a clean static badge (`Wega`) rather than an interactive engine dropdown.
  - Stopped/failed tab views provide a clean restart/reload action without legacy engine fallback buttons.

- **Preservation of Reusable Native Media and Tooling**:
  - Native media playback and session controls preserved on the platform layer.
  - Container account isolation, native notifications, and downloads pipeline retained.
  - In-browser and detached developer tools (DOM inspector, console, network recorder, debugger) connected to Wega engine protocol streams.
  - Stylo (Servo's parallel CSS engine) and Blitz DOM retained with complete upstream license and notice attribution.

- **Progress Ledger & Architectural Closure**:
  - `docs/validation/standalone/progress.json` updated: `"servo_retirement_allowed": true` enabled and Step 26 registered as complete.
  - `docs/standalone-engine-execution-plan.md` updated with Step 26 delivered status and all execution plan checklist items verified.
  - Architectural documentation and `README.md` updated to reflect the standalone engine architecture.

---

## 2. Test Verification Matrix

All gates have been executed and verified against the clean working tree:

| Test Gate | Scope / Command | Result |
| :--- | :--- | :--- |
| **Compiler & Lints** | `cargo check --bin wega` | **PASS (0 errors, 0 warnings)** |
| **Application Tests** | `cargo test --bin wega` (50 unit & integration tests) | **PASS (50/50)** |
| **Core Library Tests** | `cargo test --lib` (111 tests) | **PASS (111/111)** |
| **Engine Workspace** | `cargo test --manifest-path engines/Cargo.toml` | **PASS (all crates)** |
| **Native Engine Suite** | `scripts/native-tests.py --suite engine` (Stages 0–17) | **PASS (100%)** |
| **Native WebDriver Suite**| `scripts/native-tests.py --suite webdriver` | **PASS (100%)** |
| **Benchmark Suite** | `python3 -m unittest tests/test_browser_bench.py` | **PASS (3/3 tests)** |

---

## 3. Migration Summary (Steps 01–26)

With Step 26 complete, the entire 26-step standalone engine execution plan is fulfilled:

1. **Steps 01–06**: Baseline inventory, protocol contract v11–v14, worker subprocess harness, authoritative network/storage brokering, OS-level sandbox containment, and document host lifecycle.
2. **Steps 07–15**: Deterministic WebIDL generation, full DOM/events/Web API families, navigation integration, native internal pages, input/controls/accessibility, selection/clipboard/find, CSS/layout rendering, iframes/site isolation, and custom elements/Shadow DOM.
3. **Steps 16–22**: Storage/cookies, workers/offline execution, Canvas 2D graphics acceleration, WebGL composition, media playback/audio, device permissions, and developer tools/WebDriver automation.
4. **Steps 23–26**: Standalone default runtime, process-tree resource tuning, cross-platform release packaging & Docker validation, and final Servo retirement and migration closure.

The standalone default runtime is delivered. The browser feature migration is
**not closed**; the current remaining stages must pass their own acceptance gates.
