# PR #58 deletion audit: what was removed, and what of it was DevTools

Audit round D0. Nothing was restored, implemented or changed at runtime; this document records evidence only.

Evidence kinds: **git** (read from local history with `git show`, `git log`, `git diff`, `git grep`; no checkout, no build), **read** (source read at the historical revision), **inferred** (reasoned from the evidence, not shown by it), **unverified** (could not be checked). GitHub metadata was not reachable (`git fetch` is denied in this environment: *Permission denied (publickey)*), so everything below comes from the local clone.

## 1. The PR in git

| Item | Value (git) |
|---|---|
| Merge commit | `0efe45b` "Merge pull request #58 from wulfnb/fix/wega-engine-rendering-and-svg-colors", 2026-11-11 |
| Parents | `e58c5a8` (main side), `c7d4320` (branch head) |
| Merge base | `0316dd2` (referred to below as **OLD**) |
| Branch commits (8) | `ee3977f` refactor: checkpoint standalone Wega without Servo backend · `70ccd88` refactor: finish standalone runtime integration and validate Servo removal · `4a7b92f` fix(engine): oklch SVG stroke colors, bg-clip-text fallback, module status notice · `a811372` fix(ci): retry DMG creation · `f1a8727` style · `85b8723` fix(ci): surfman sm-x11, dead code on evaluate · `1f2b962` fix(harness): smoke-page.png · `c7d4320` fix(scripts): desktop flag in package-macos.sh |
| Branch-side diff (`0316dd2..c7d4320`) | **1,104 files, +3,631 / −198,516**: exactly the figures quoted. A = 6, D = 1,002, M = 89, R = 7 |
| Main-line diff (merge vs first parent) | 1,104 files, +3,618 / −198,515 |
| Where the lines went, by commit | `ee3977f`: 1,079 files, +2,100 / **−198,000**; `70ccd88`: 42 files, +1,430 / −811; `4a7b92f`: +387 / −21; the remaining five commits change ≤ 13 lines each |

So 99.7 % of the deletion is one commit, `ee3977f`, and the remainder of the PR is the rendering/SVG fixes its title names.

## 2. Breakdown of the 198,516 deleted lines

| Category | Deleted lines | Files | Nature |
|---|---|---|---|
| `vendor/servo-script-bindings` | 86,085 | 789 | vendored Servo DOM-bindings generator: `third_party` Python deps 35,803; 571 WebIDL files 18,534; `codegen` 12,367; its `Cargo.lock` 6,456; Rust glue |
| `vendor/servo-net` | 30,432 | 51 | vendored Servo network stack: `tests` 9,480; `Cargo.lock` 6,217; `http_loader.rs` 2,998; `fetch`, `image_cache.rs`, `http_cache.rs`, `devtools.rs` (195) |
| `vendor/servo` | 23,841 | 35 | the Servo embedder crate: `Cargo.lock` 11,688; `tests` 5,378; `servo.rs`, `webview.rs`, `webview_delegate.rs`, `servo_delegate.rs` |
| `vendor/epaint` | 20,963 | 60 | vendored egui renderer fork (`vello_cpu` feature-unification fix) |
| `vendor/servo-storage` | 13,111 | 24 | vendored Servo storage: `Cargo.lock` 5,394; IndexedDB 4,638 |
| `vendor/sea-query-rusqlite` | 991 | 3 | SQLite glue |
| other `vendor/` (README, `sources.json`, patches) | 158 | 5 | bookkeeping |
| **Vendored Servo-side total** | **175,581** (88.4 %) | 967 | not Wega-authored; recorded in `vendor/README.md` |
| root `Cargo.lock` | 7,479 (+1,207) | 1 | 1,012 → 522 locked packages (the `servo*`, `stylo*`, `mozjs*`, `webrender*`, `servo-devtools`, `servo-devtools-traits` families left) |
| `src/` (Wega-authored browser UI) | 14,288 (+1,095) | 73 | see §3 |
| `README.md`, docs, tools, tests, scripts, CI | ≈ 1,170 | ~50 | `tools/standalone_inventory.rs` 431; README 485; test and script adjustments |
| Added by the PR | +3,631 | — | `docs/history/embedded-servo-readme.md` (514), `docs/servo-removal-checkpoint.md` (47), `src/window_context.rs` (144), Blitz/SVG fixes (`gradient.rs`, `text.rs`, `background.rs`), WebIDL upstream copies, media plugin lists |

By file type: `.rs` 88,044; `.py` 43,802 (the bindings generator's third-party Python); `.lock` 39,068; `.webidl` 19,192 (580 files).

**Servo versus Wega-authored.** About 88 % of the deleted lines (175,581) are vendored Servo-side trees plus lockfiles for them. The Wega-authored code the PR deleted is concentrated in `src/` (≈ 14,300 net lines) and was the browser's Servo integration: delegates, adapters, harnesses and the developer tools below. The Wega *engine* workspace (`engines/`) was not touched except small additions (`+606`, `−42`).

## 3. What was DevTools

Not every deleted file was DevTools. Deleted or gutted files that were (OLD line counts, all by `ee3977f` unless noted; `src/` paths):

| File | OLD → HEAD lines | Role | Backend (read) |
|---|---|---|---|
| `devtools_ui.rs` | 1,083 → 104 (`ee3977f` → 100, `70ccd88` → 104) | dock layout, ten-tab host, Network/Storage/Accessibility/Style panels | Servo `WebView` (`header: "every panel reads the active Servo webview"`) |
| `toolbox.rs` | 989 → 13 | `Tool` enum (Inspector, Console, Debugger, Network, StyleEditor, Performance, Memory, Storage, Accessibility, Application), snapshot jobs, native log pump, memory report | `devtools_traits::DevtoolScriptControlMsg`, `servo::WebView::evaluate_javascript`, `servo.create_memory_report` |
| `console_ui.rs`, `console_model.rs` (+2 test files) | 518 + 263 (+55 + 142) → 0 | structured console | `devtools_traits` console and page-error messages |
| `debug_driver.rs`, `debugger_ui.rs`, `debug_model.rs` (+tests) | 437 + 444 + 99 (+38) → 0 | sources, breakpoints, pause/step, frames, scopes, watches | `DevtoolScriptControlMsg::{Interrupt, ListFrames, GetEnvironment, Eval, Blackbox, …}` over `servo_base::generic_channel` to Servo script threads |
| `inspector_ui.rs`, `inspector_probes.rs`, `probes.rs` | 387 + 43 + 58 → 0 | DOM tree, box model, rules, element picker, highlight, CSS/HTML editing | JavaScript injected into the Servo page (`evaluate_javascript`); `inspector.js` (44 lines, unchanged but now unreferenced from `src/`) |
| `network.rs` (+tests), `network_controls.rs` (+tests), `network_har.rs` (+tests) | 857 + 155 + 204 (+273 + 81 + 172) → 0 | request recorder (500 rows, 64 KiB bodies), HAR 1.2 import/export, cURL/replay, options menu | passive Servo `DevtoolsControlMsg::NetworkEvent` stream through a `native-devtools` bridge thread |
| `performance_ui.rs` | 241 → 0 | Performance API + rAF timeline | JS injected into the page (`PerformanceObserver`, `requestAnimationFrame`); no CPU profile |
| `cookie_inspector.rs` (+tests) | 157 (+67) → 0 | cookie table including HttpOnly | `servo.site_data_manager().cookies_for_url_async` |
| `harness/toolbox.rs` | 1,284 → 0 | `--toolbox-test` end-to-end check on a real page and HTTP fixture | Servo |
| `tool_window.rs` | 248 → 198 | detached tool window (rendering context rewritten, picker/debugger shortcuts dropped) | Servo `RenderingContext` → `window_context` |
| `engine.rs` (455 → 17), `browser.rs` (−1,410), `options.rs` | n/a | `notify_devtools_server_started`, `request_devtools_connection`, `devtools_server_enabled`, `--remote-devtools PORT` | Servo delegate and builder prefs |

**Quantified.** Wega-authored DevTools code deleted (net lines of the files above, git numstat): **UI and models 4,937** + **network panel 1,742** + **test harness 1,284** = **7,963 lines (4.0 % of the PR's deletions)**. Vendored Servo files whose path names DevTools, debugger or console: 14 files, 484 lines (`servo-net/devtools.rs` 195; `Debugger*Event.webidl`, `Console.webidl`). The Servo DevTools server and actors themselves (`servo-devtools`, `servo-devtools-traits`, and the script-thread side in `servo-script`) were **never vendored**: they were crates.io dependencies (`servo-devtools`, `servo-devtools-traits` are in the OLD `Cargo.lock`; the OLD `Cargo.toml` `desktop` feature names `dep:servo-devtools-traits` directly), so their size does not appear in the diff beyond the lockfile.

## 4. What the OLD DevTools actually were

- They were a **native egui front-end written by Wega**, fed by **Servo's in-process devtools event stream and script-thread control messages**. They were not Servo's own (Firefox-style) front-end: no embedded Firefox DevTools UI exists in OLD (`devtools_ui.rs`, `toolbox.rs`, `tool_window.rs`). The only route to Servo's own protocol server was the opt-in `--remote-devtools PORT` (loopback, default 7000, attach Firefox via `about:debugging`; in that mode native Network and Debugger recording is disabled per `browser.rs` comments).
- **They worked only for tabs on the legacy Servo engine.** At OLD, `Browser::devtools_contents` begins `if self.is_wega() { …minimal panel…; return; }` (`devtools_ui.rs:85-117`, verified): for a Wega-engine tab it drew the same small status panel that HEAD draws (URL, title, PID, worker RSS, JS heap, DOM nodes, loading, phase, compatibility). Everything below line 118 ran only for Servo tabs (`self.tab().view()` is a `servo::WebView`).
- **Servo was not the default at OLD but was reachable.** `EngineKind` defaults to `Wega` (`engine_choice.rs:3-8`); `choose()` returns Wega unless a saved choice, an explicit choice, a per-site preference, or "Wega unavailable" selects Servo; `ui.rs:581` called `engine_picker` (a combo box offering "Wega" and "Servo (legacy)"); and `options.rs` forced Servo for every native test mode except the engine test, so `--toolbox-test` ran on Servo. `docs/standalone-engine-execution-plan.md:721-725` says Step 23 made Wega the default and replaced the selector with a badge; the picker code was still present and called at OLD (a stale comment at `ui.rs:693` still says "Servo is the default"). Whether most users ever switched a tab to Servo is **unverified**.
- **Real or simulated?** The data was real engine data (Servo's own network events, console messages, cookie jar, memory report, debugger state). The checked-in end-to-end test `src/harness/toolbox.rs` (run by `scripts/docker-toolbox-test.sh` via `.github/workflows/ci.yml:395`) asserted real results on a real page: POST status 201 and bodies/headers, a non-empty memory report, an HttpOnly cookie row not visible to page JS, `paused_line == Some(5)` for the debugger, `1+2` evaluated in the console, computed style values, element picker cleanup, dock and detached-window frames. **Whether those assertions passed at OLD was not checked** (no CI logs; nothing was run). The `console_*`, `debug_model`, `network_*` unit tests only test Wega's own models with fabricated events.
- Documented limits at OLD (in the code): no network throttling or WebSocket frame inspection ("not exposed by this Servo build"); HAR `dns`/`wait`/`receive`/`ssl` timings are −1; Performance is a timeline of Performance-API entries, "CPU stacks, flame graphs and engine-thread sampling are unavailable"; the debugger lacks pause-on-exception, conditional/log/XHR/event/DOM breakpoints, source maps and worker targets; source capture capped at 250 files × 1 MiB.

## 5. Did PR #58 remove working DevTools?

**Yes for the legacy Servo engine; no for the default Wega engine.**

- The PR removed a substantial, apparently working Servo-backed DevTools suite (≈ 8,000 Wega-authored lines plus its test harness) because its only data source, Servo, was removed in the same commit. The PR's own documents say so: `docs/servo-removal-checkpoint.md` ("The removed Servo-specific inspector, debugger, network bridge, and toolbox are not ported. Wega's native developer panel currently exposes worker and document state") and README line 38.
- For a Wega-engine tab (the default) nothing was lost at the UI level: the OLD panel was already the minimal status panel, identical in kind to HEAD's. Wega-engine tabs never had an inspector, console, network, debugger or storage panel.
- What survives at HEAD: the docking shell (`Dock::{Left, Right, Bottom, Window}`, narrow windows forced to Bottom, persisted in the profile), the detached tool window (`tool_window.rs`), the F12 toggle and the toolbar/menu entries, the minimal status panel, the native logger, and WebDriver (`--webdriver-port`, `automation.rs`: navigation, JavaScript evaluation, page source, screenshots, resource reports). Removed: Cmd/Ctrl+Shift+I/E/K now only open the empty panel (the per-tool tab selection is gone), the `--remote-devtools` option (now rejected as an unknown option; `options_tests.rs` asserts the error), the element picker, the split-console behaviour and the `--toolbox-test` mode.
- `inspector.js` (DOM/CSS snapshot script) and the data models' logic are engine-neutral in design but nothing in `src/` loads `inspector.js` any more (referenced only by `tests/probe_scripts_tests.mjs`).

## 6. Uncertainties

- No code was run: "working" means the code paths exist and a harness asserted them, not that the harness passed.
- GitHub PR metadata (review comments, CI results, description) was unavailable; the PR number and branch name come from the merge commit message.
- `devtools_ui_tests.rs` and `debug_driver_tests.rs` assertions were not read.
- Whether the "block request" action in the Network panel blocked live traffic was not checked.
- Whether the engine selector was rendered in the shipped UI at OLD (the `engine_picker` call site exists; the plan document claims it was replaced by a badge) was not resolved.
- The Wega engine's WebDriver coverage at HEAD (what network, console or storage data it can return) was not audited in this document; see `devtools-current-capabilities.md`.
