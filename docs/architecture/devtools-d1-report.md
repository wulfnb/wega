# DevTools D1 report: inspector foundation, IPC and lifecycle

Branch `feature/dom-rounds-9-12`, local only, not pushed. Design: [`devtools-d1-design.md`](./devtools-d1-design.md). All results below were **run on macOS arm64 (Apple M4, 10 cores, 16 GiB) only**.

## 1. Result

**D1 is complete for the scope in the brief**: a trusted browser UI attaches to a selected Wega target over a typed, versioned inspector protocol; a real engine status snapshot travels through it; the status panel renders that snapshot; sessions cannot reach another tab; navigation, detach and process replacement reject stale replies; every queue and pending-request count is bounded; a SIGSTOP-stalled engine does not freeze the UI. Nothing from D2 onward was started (no console, evaluation, DOM/CSS, network or debugger code).

Commits (oldest first):

| Commit | Stage |
|---|---|
| `47dc779` | Design |
| `584843e` | Protocol, session authority, engine backend, real-process tests |
| `8bcfe48` | Browser UI integration (`Page`, `DevtoolsView`, panel) and tests, native end-to-end harness |
| `b0770d5` | Shared subprocess harness, inspector benchmarks, D1 engine-bench baseline |
| this commit | Report and roadmap update |

## 2. What changed

| Area | Module | Notes |
|---|---|---|
| Protocol | `engines/protocol/src/inspector.rs`, `inspector/session.rs`, `inspector_tests.rs` | `VERSION` 21 → 22, `Capability::Inspector`, `Command::{Inspector, InspectorDetach}`, `Event::{InspectorResponse, InspectorEvent}`, `RequestKind::Inspector`, `DocumentRequests::{begin_inspector, cancel_inspector, inspector_pending}` |
| Engine backend | `engines/wega/src/inspector/{mod,tests}.rs`, `main.rs` dispatch and per-loop `observe` | names no layout/script engine type (a test enforces it) |
| Browser transport | `src/wega_page.rs` | `HostSessions` inside `Snapshot`; `Page::inspector_{open,request,close,poll,target}` |
| Browser client | `src/devtools_session.rs` | `DevtoolsView` state machine over an `InspectorPort` trait |
| UI | `src/browser.rs`, `devtools_ui.rs`, `tool_window.rs`, `ui.rs`, `capabilities_ui.rs` | panel renders from the snapshot; session exists exactly while a surface showing it is open |
| Native harness | `src/harness/inspector.rs`, `--inspector-test`, `scripts/inspector-native-test.sh` | real window, real `Page`, real engine, real SIGSTOP |
| Tests/bench | `engines/wega/tests/{inspector,inspector_bench}.rs`, `tests/support/inspector_worker.rs` | |

### Protocol (as built)

Closed, parameterless command set: `Attach, Detach, GetTargetInfo, SubscribeStatus, UnsubscribeStatus, GetStatusSnapshot` (an unknown or parameterised command cannot be represented). Ids: `TargetId{tab,frame,process}`, `InspectorSessionId(u64)` (process-wide monotonic issuer, checked add, never reused, never 0), the existing `RequestContext` for correlation and `DocumentContext` for the document/execution-context generation. Errors: `Unauthorized, UnknownTarget, StaleSession, StaleDocument, Unavailable, UnsupportedVersion, Timeout, Cancelled, InvalidPayload, Busy, SessionLimit`. The request payload crosses the pipe as an opaque value decoded by the receiver, so a malformed request is answered (`InvalidPayload` / `RequestFailed`) instead of failing the packet and tearing the worker down. `Detach` for revocation is a separate unscoped `InspectorDetach` (reason in design §3).

Status snapshot (385 bytes on the wire for a blank page; hard cap far below 16 KiB): target, document stamp, url, title, loading, viewport (CSS px + DPR), DOM node count, JS heap, back/forward, protocol/inspector versions, engine name.

### UI behaviour (as built)

* F12 and Cmd/Ctrl+Shift+I/E/K unchanged; the "Engine diagnostics" window shares the surface and the same session rules.
* Open attaches to the selected tab; close detaches (no suspended state); the page is untouched.
* Docked panel follows the selection by detach-then-attach, clearing displayed data in the same tick. Detached window (`Dock::Window`) is pinned to its target tab; a tab with an open inspector is exempt from background discard.
* Process replacement for the same tab re-attaches with a new session id; crash shows "Disconnected"; closed tab shows "The inspected tab was closed"; no-page tabs show the prior message.
* The panel displays a snapshot only if it passes **render-time** checks against the live page (same target, same document stamp). PID, RSS and lifecycle phase are browser-side supervisor facts, labelled as such, not engine data.

## 3. Deviations from D0 and the brief

See design §2 for the full table. Summary: typed enums instead of `Domain.method` strings; flat `src/devtools_session.rs` naming; registry lives in each page's `Snapshot` (not a separate per-tab authority object); no `Target.list`; no credit-window protocol (events are change-driven, bounded browser-side ring with drop-oldest and an in-band `EventsDropped` marker); no fuzz target (table-driven malformed-payload tests instead). The roadmap's "cross-cutting prerequisite" about `set-cookie` response headers was **not** done (it belongs to D4). `viewport` is the viewport the browser last requested (the document applies it at the next paint; the document's own config lags a `Resize`, which a test caught). `nodes`/`js_heap_bytes` are the engine's existing ≤ 2 s samples (a tree walk per request was rejected).

## 4. Tests

| Suite | Result |
|---|---|
| `wega-protocol` | 61 passed (39 before; +22 incl. malformed-payload table, authority rules, ring overflow, issuer exhaustion/threads, boundary text) |
| `wega-engine` inspector backend (unit) | 7 passed |
| `wega-engine --test inspector` (**real engine process over the real pipe**) | 15 passed: attach/snapshot/detach correctness (viewport, target, document, bounds), capability, forged/unknown/cross-tab/cross-process, supersede and idempotency, stale document and revision, replayed id, unsupported versions and malformed payloads (worker survives), events across navigation and after unsubscribe/detach, none without subscription, 150 rapid attach/detach cycles, page-JS inaccessibility, queued request expiry and cancellation behind a busy DOM thread, v21 sessions never offered the inspector, process replacement for the same tab |
| Root `DevtoolsView` (scripted port enforcing registry liveness) | 18 passed: open/close/reopen, closed panel makes zero calls, tab switch clears and detaches, foreign snapshot rejected, pinned window, unpin, closed target, unloaded then new process, restart, crash with rate-limited retry, navigation and stale-stamp snapshot, stalled page (no blocking, ≤ 3 requests), refresh once per second, bounded attach retries, refused open, 200 rapid open/close (≤ 1 live session, no id reuse), dropped events |
| Root `Snapshot` glue (`wega_page`) | 9 new (20 in module): reply correlation, mismatched kind/session, delayed reply after detach, navigation → `StaleDocument`, timeout without a later send, late reply as timeout, pending cap, event gating/replay/flood, dead process |
| Native end-to-end (`scripts/inspector-native-test.sh`, 1 run mode) | passes (see below); run 4 times |
| Whole `engines` workspace, run one target at a time because of disk | `wega-engine` integration targets: 41 targets, **334 passed, 0 failed, 10 ignored**; workspace lib/bin tests: 146 passed, 0 failed; other crates' integration tests: 181 passed, 0 failed, 3 ignored. The last two lines overlap for lib test binaries, so do not add them to compare with Round 12's 509 (a different aggregation) |
| Broker mode (`WEGA_TEST_BROKER=1`) security, forms, scripts, documents, history | 5 targets with 5, 4, 3, 8 and 9 tests, all passed, 0 failed (per-target names not recorded in the output I kept) |
| Required-worker test with the macOS content bundle helper (v22 worker) | passes |
| Root: lib 113, bin 54 (`--features test-harness`); `--no-default-features --lib` 109 | all pass |
| Tools workspace tests (architecture checker 8, tools 6) | pass |

**Not covered by tests:** the inspector running inside the *sandboxed* (required) worker (the subprocess tests and the native test use the development worker, because release workers refuse it and a debug sandbox run was not set up); tab switching, tab closure and process restart through a **real** `Page` (unit-tested through the port only; only crash/close glue is covered at `Snapshot` level); the detached OS window rendering path (shared code, not exercised); visual output of the panel (no screenshot taken).

### Native end-to-end (real window, `Page`, engine process)
Stages: no session while DevTools is closed → open → real snapshot (URL, title, nodes, viewport) → navigate: any displayed snapshot always equals the live document and tab → close revokes the session in the page's authority and the page keeps its PID → reopen with a **different** session id → `SIGSTOP` the worker → view reports not responding, no data shown, UI loop alive → `SIGCONT` → data returns. Measured: **NotResponding 2.47 s after SIGSTOP with ~290 UI polls (1.5 s stall threshold + one refresh slot); recovery 62 ms after SIGCONT.**

## 5. Performance (Phase 8)

Conditions: `scripts/engine-bench.py`, release, 3 repeats, commit `b0770d5`'s engine. **Load average was 1.7–2.4 before and 4.8–6.1 after; the Round 12 baseline was recorded at ≈ 9–11.** The comparison is therefore indicative, not a clean A/B. Saved as `baseline/engine-bench-d1.json`.

| Item | Result |
|---|---|
| Engine benchmark with DevTools closed vs Round 12 | 64 metrics compared. Beyond run-to-run spread: `8 parallel workers create + reply` −4.7 % (faster), `create_view+set_html (document lifecycle)` **+7.5 % (slower)**. Others within ±4 % except `createElementNS` +10.7 % (inside its own spread) and `paint 200x150` −6.7 %. The D1 engine change on this path is one `version >= 22 && inspector.is_some()` branch per loop iteration; I did **not** establish whether the +7.5 % is noise or load: it should be re-measured on a quiet machine before claiming "no regression". `create_view+set_html` does not execute inspector code. |
| Attach / detach round trip (debug engine, 300 each) | 0.091 ms median, p95 0.111 / 0.112 ms |
| Status snapshot round trip (500) | 0.126 ms median, p95 0.136 ms, max 0.19 ms; 385 B |
| Navigation settle, 40 each | closed 87.9 ms median; attached+subscribed 90.0 ms (+2.1 ms, p95 91.1 vs 91.6 ms); 40 `DocumentChanged` events delivered |
| Idle engine CPU, 10 s windows (`ps`, 10 ms resolution) | closed 0.00 %; attached+subscribed 0.00 %; with 1 Hz snapshots 0.20 % (earlier run 0.10 %); differences are at the instrument's resolution |
| Event queue memory | ring capacity 64 events = 1,536 B static bound; peak length after 5,000,000 events = 64; resident growth 0–64 KiB; ingest 6–42 ns/event (release/debug) |
| Browser-side idle cost | one snapshot request per second while a surface is visible and **none when closed** (test: 0 port calls with the panel closed over 100 ticks; 8–11 requests in 10 s of 60 fps ticks); UI CPU was not measured |
| Stalled engine | see native test; snapshot requests against a stalled page ≤ 3 over 4 s of ticks (unit test) |

Debug-engine numbers are not comparable with release numbers (release workers refuse the unsandboxed development mode, so subprocess benchmarks can only run in debug).

## 6. Security findings

1. Page JavaScript has no path to the inspector: the page has no pipe, the engine's JS surface names nothing inspector-related (test scans globals and prototype chain), and forged routes produced no inspector traffic. **Limit:** the test asserts absence of names and traffic, not that every probed route throws.
2. Authority is browser-issued and process-bound; every id the engine sees is also checked engine-side (never attached → `Unauthorized`, revoked/older → `StaleSession`, other tab/process/frame → `UnknownTarget`; foreign identity envelopes still kill the worker).
3. **Found by the native run (fixed):** snapshot validation happened only at tick time, so a navigation between tick and paint could display one frame of a stale snapshot. Display now re-validates against the live page (`Browser::devtools_snapshot`).
4. **Found by the native run (fixed):** `devtools_snapshot` initially re-locked the page mutex it already held (self-deadlock of the UI thread). Fixed by reading the target from the held guard; the native test now covers it, unit tests did not (mock ports cannot deadlock a real mutex).
5. **Found by design review (fixed):** a scoped `Detach` could be dropped by stale-document admission, leaving a revoked session live engine-side; revocation uses the unscoped `InspectorDetach`, and a newer attach supersedes anyway.
6. **Found by the engine run (fixed):** viewport reported the document's lagging config after `Resize`.
7. No new `unsafe` (diff grep on the branch: only a comment containing the word). The native harness signals the worker with the system `kill`.
8. No new listener, port or remote transport; WebDriver unchanged; `engines/Cargo.lock`, root and tools lockfiles unchanged.

## 7. Platform and gate coverage

| Gate | Result |
|---|---|
| `cargo fmt --check` root, engines, tools | pass |
| Clippy `-D warnings`: root (default, `test-harness`, `--no-default-features`), engines workspace `--all-targets`, tools | pass |
| `cargo check --locked` root; `cargo metadata --locked` root, engines, tools | pass |
| Architecture checker | 13 members, **24 edges, 0 violations**, 11 informational (unchanged) |
| `scripts/check-engine-boundary.py`; protocol and engine inspector boundary text tests | pass |
| Dependency graph validation | the architecture checker's graph (24 edges) is unchanged; `dependency-graph` was not regenerated |
| Unsafe inventory | 0 new |
| **Platforms** | macOS only. **Linux, Windows, MSVC Skia, native sandbox probes, packaged builds were not run**; nothing here claims them. A disk-full during a first all-targets run (1.4 GiB free) was resolved by deleting `engines/target/release` (this repository's artifacts) and running targets one at a time. |

## 8. Known limitations

* One shared FIFO pipe: a stalled DOM thread also delays other commands; D1 adds at most 4 pending inspector requests to it and no priority lane. When the pipe to a stopped process fills, the browser's writer thread (never the UI) blocks as it already did for any command.
* Snapshot `nodes`/`js_heap_bytes` can be up to 2 s old; `viewport` is the requested, not yet painted, viewport.
* Refresh is polling at 1 Hz plus change events; there is no credit window.
* A pinned detached window re-attaches automatically when the same tab gets a new process (documented policy).
* One session per target; a second UI client would supersede the first.
* No fuzzing; no schema golden file (round-trip and table tests only).

## 9. What D2 can rely on, and what is unverified

**Can rely on:** the closed-enum extension path (add a variant + capability gate, bump `INSPECTOR_VERSION`); `HostSessions`/`ContentSessions` authority and bounded rings; `RequestKind::Inspector` correlation with navigation cancellation, deadlines and the sub-cap; `Page::inspector_*` as the only UI entry; `DevtoolsView` + `InspectorPort` for UI policy tests; `devtools_snapshot`-style render-time validation as the pattern for any displayed engine data; the subprocess harness (`tests/support/inspector_worker.rs`) and the native `--inspector-test` for real-process checks including stalls; the engine backend dispatch in `main.rs` and `inspector/`.

**Unverified / D2 must check first:** the inspector under the sandboxed required worker (D2's evaluation gate raises the stakes); real-`Page` tab switch/close/restart and the detached OS window; whether the `create_view+set_html` +7.5 % is noise (re-run `engine-bench.py` on a quiet machine); Linux and Windows behaviour; `evaluate` still has no capability gate and D1 adds none (D2 must add `NotAuthorised` before exposing evaluation).
