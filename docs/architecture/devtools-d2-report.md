# DevTools D2 report: console, JavaScript runtime and secure evaluation

Branch `feature/dom-rounds-9-12`, local only, not pushed. Design: [`devtools-d2-design.md`](./devtools-d2-design.md); builds on [`devtools-d1-report.md`](./devtools-d1-report.md). All results below were **run on macOS arm64 (Apple M4, 10 cores, 16 GiB) only**.

## 1. Result

**D2 meets its definition of done for the scope below**, with the limitations in §9 stated rather than hidden. Real page console output and uncaught errors reach the Wega DevTools Console (including output from before the panel opened); histories and memory are bounded at every hop; evaluation needs a browser-minted, engine-verified grant that navigation, detach, supersede and process death revoke; cross-tab and replacement-process evaluation is refused; a stalled engine does not freeze the UI. Object handles, `awaitPromise` and `throwOnSideEffect` are **deliberately not implemented** (§8). Nothing from D3 was started.

Commits (oldest first):

| Commit | Stage |
|---|---|
| `bdb9519` | Design |
| `05f0d88` | Protocol (version 23, inspector 2, closed commands/events, grant, bounded queues) |
| `183aae6` | Console capture, preview generator, per-document ring, engine backend, V8 `evaluate_inspect` |
| `6c7ab86` | Authorisation, browser plumbing, view state machine, 19 real-process tests |
| `4371f47` | Console panel, native end-to-end test, sandboxed-worker test |
| `6eebdd6`, `a6880a3` | Benchmarks |
| this commit | Report and roadmap |

Preflight: branch clean at `3e8964d`, `VERSION` 22 / `INSPECTOR_VERSION` 1 confirmed; the roadmap file exists (D1 had updated it).

## 2. The four invariants, with evidence

| # | Invariant | Evidence |
|---|---|---|
| 1 | Grants: CSPRNG, bound to session/process/document, revoked on navigation and detach, revalidated before execution, token never exposed | Token = 128 bits from `getrandom` (`Page::inspector_grant`), stored only in the page's `HostSessions` and sent on the identity-wrapped pipe. `GrantToken` and `InspectorPayload` have redacting `Debug` (tests print a whole `Command`). Browser refuses to send without a grant for *this session and the current document stamp*; the engine re-checks session, token (branch-free compare) and the **current** document stamp immediately before running, independently of whether `observe` has already cleared state (unit test: stamp moved, grant not yet cleared → `StaleDocument`, page never touched). Cleared on navigation commit, detach, supersede, revoke, refused grant and process death. Real-process tests: forged/zero/guessed tokens, navigation, detach, supersede, revoke, another tab's session and token on both pipes, replacement process of the same tab. Native test: grant disappears at navigation and the next command is refused. |
| 2 | Previews never run page code | `__wega_preview` uses only intrinsics captured at bootstrap, own data descriptors, `core.isProxy` first, hand-built JSON (never `JSON.stringify` on objects). Tests against real V8: objects with getters, `toJSON`, `toString`, `valueOf`, `Symbol.toPrimitive`, proxies (including a proxy of a proxy handler), accessor array elements, an error with an accessor `message`: **0 hits on every counter** after previewing and after `console.log`; the page replaced `JSON.stringify`, `Object.getOwnPropertyNames/Descriptor`, `getPrototypeOf`, `Object.hasOwn`, `Reflect.apply`, `Array.isArray`, `Array.prototype.push/[Symbol.iterator]/toJSON`, `String.prototype.slice/concat`, `Object.prototype.{toJSON,value,get,kind}` and none was called; the generator is deleted from the global after capture (a forged `__wega_preview` has no effect). Exotic values are type-only. |
| 3 | Pre-DevTools output and uncaught errors retained | Per-document ring always collects. Real-process tests: a data page that logs and throws during load, logs after load, then the console is enabled → all backfilled in order with the exception (name, message, stack, first frame). Native test: same through the real panel. Timer errors and unhandled rejections too. |
| 4 | Floods bounded and non-blocking | Producer ring **1000 entries and 1 MiB** per document (tests: 10,000 and 5,000 messages; byte budget trips before the entry budget for large messages; one message ≤ 8 KiB). Delivery ≤ 32 messages / 64 KiB per engine loop iteration, with a 2 ms pacing wake while a backlog exists (found necessary: without it the backlog drained only on unrelated events). Browser queue 256 entries / 256 KiB and UI history 1000 / 1 MiB, each with drop-oldest and counters surfaced in the panel. Engine reports `ConsoleMessagesDropped`; a 20,000-message flood arrives as 1,000 messages + 19,000 reported dropped. A snapshot request during a draining backlog takes 2.9 ms (debug); navigation after a flood completes. The page's flood task is not slowed by delivery (1788 ms disabled vs 1773 ms enabled, 20,000 calls, debug). |

## 3. Architecture (as built)

* **Protocol** (`wega_protocol::inspector`): `InspectorCommand` stays a parameterless Copy discriminant (+ `EnableConsole, DisableConsole, ClearConsole, GetConsoleHistory, GrantEvaluation, RevokeEvaluation, Evaluate`); parameters travel in a closed `InspectorArgs` enum validated against the command; `Preview`, `ConsoleMessage` (with optional `ExceptionDetails`), `EvalOutcome`; events `ConsoleMessageAdded`, `ExceptionThrown`, `ExecutionContextCreated/Destroyed`, `ConsoleMessagesDropped`. `VERSION` 23 / `Capability::InspectorConsole`; inspector version 2 with v1 requests still valid; a protocol-22 worker answers `Unsupported`. Per-command deadline: 2 s, but 6 s for `Evaluate` (script deadline 5 s + margin).
* **Engine**: `console.rs` (ring, validation of page-supplied entries, exception-text parser), `inspector/mod.rs` (`PagePort` trait implemented by `View`; grant check; budgeted delivery; backlog pacing), `ScriptHost::evaluate_inspect` in the V8 adapter (same `execute_script` under the same `bounded()` watchdog and heap budget as page script; exceptions come from the structured `JsError`; preview produced by the captured generator in a second bounded section). Page-side: wrappers for `log/info/warn/error/debug` (exact method and `currentUserCallSite()` location), formatter-derived output (`table`, `time*`, `count*`, `trace`, `assert`, `group*`, `dir`) captured as text from the existing callback; uncaught errors and unhandled rejections (new `rejection` host op) parsed Rust-side from text the runtime already reported, so no new property reads on thrown values.
* **Browser**: `Page::inspector_{grant,revoke_evaluation,evaluate}`; `HostSessions` holds the grant, the console queue and clears both at navigation commit; `DevtoolsView` enables the console after attach, owns `ConsoleView` (history, filters, command history 100, pending evaluation), and exposes the user actions. **Authorisation flow**: user clicks *Enable evaluation* (panel button; "Enable anyway (opaque origin)…" for documents whose scheme is not http/https/file) → browser mints token and records `{session, process target, document stamp, token, override}` → `GrantEvaluation` → engine stores it after checking session and origin policy → each `Evaluate` carries the token, which the browser only attaches for the live session and current document.
* **UI**: Status | Console switch in the retained panel; message list with severity glyph/colour, local timestamps, `file:line:col`, collapsible exception stacks; text filter, severity toggles, Clear, dropped-message banner, persistent "Evaluation enabled" banner with Disable, command input (Enter to run, Up/Down history), disconnected / not-responding states. Docking and the pinned detached window are unchanged.

## 4. Object lifetime rules

No JavaScript object is retained anywhere by the inspector: previews are self-contained values, so there is nothing to release, expire or reuse across navigation or process death. Expandable objects (`GetObjectProperties`/`ReleaseObject`) are **not offered**; a test asserts those commands (and `AwaitPromise`, `CallFunctionOn`) are rejected as invalid.

## 5. Budgets

| Resource | Bound | Overflow |
|---|---|---|
| Engine ring | 1000 entries, 1 MiB per document | drop-oldest, counted, reported in-band |
| One message | ≤ 8 KiB; preview depth 3, 8 props/level, 64 nodes, 256 chars; ≤ 16 args | shrunk (props dropped, args truncated) |
| Delivery | 32 messages / 64 KiB per loop iteration | cursor lag → `ConsoleMessagesDropped` |
| Browser queue | 256 / 256 KiB per session | drop-oldest + marker |
| UI history | 1000 / 1 MiB | drop-oldest + visible counter |
| Command history | 100 | oldest dropped |
| Pending inspector requests | 4 | `Busy` |
| Expression | 16 KiB | `InvalidPayload` |

## 6. Tests

| Suite | Result |
|---|---|
| `wega-protocol` | 76 passed (61 at D1; +15: args validation, v1 compatibility, deadlines, token redaction, grant registry negatives, console queue bounds, bounded types, wire round trips) |
| Engine unit (`wega-engine --lib`) | 62 passed (console ring 12, backend 19 incl. grant matrix and budgeted delivery) |
| **Real V8, in-process** (`tests/console.rs`) | 13 passed: every method, hostile objects (0 hits), tamper resistance, every value kind within budgets, forged entries, load-time exceptions/rejections, 10,000-message flood, oversize values, navigation, evaluation results/exceptions, watchdog, origin policy |
| **Real engine process over the pipe** (`tests/inspector_console.rs`) | 19 passed, covering the brief's items 1–20 (item 15, stale object handles: handles are not offered, which a test asserts; item 17, promise rejections: delivered as `UnhandledRejection` exceptions; items 18–19: SIGSTOP/SIGCONT) |
| D1 real-process inspector suite | 15 passed unchanged |
| **Sandboxed required worker** (`tests/inspector_sandbox.rs`, ignored, run with the bundled helper) | passes: `is_development()` asserted false; attach, console backfill of the page-load log, refusal before grant, grant (http origin, no override), `6*7`, a `TypeError` outcome, `typeof Deno` is absent, output from an evaluation, detach revokes. Closes D1's gap for the engine side. |
| Root `DevtoolsView` with a scripted port (incl. console/evaluation) | 32 view tests; root bin total 82 (54 at D1), lib 113, `--no-default-features --lib` 109 |
| **Native end-to-end** (`scripts/inspector-native-test.sh`): real window, `Page`, engine | passes (4 runs): backfill, live capture, refused-before-grant, explicit grant, `6*7 = 42`, a thrown `TypeError` with stack, navigation clears grant and history, detach/reattach with a new session, SIGSTOP → not responding after 2.45 s with ~290 UI polls, recovery 78 ms after SIGCONT |
| Whole engines workspace, one target at a time (disk) | wega-engine integration: 45 targets, **366 passed, 0 failed, 13 ignored** (D1: 334 in 41); workspace lib/bin 185 passed; other crates 196 passed, 3 ignored. WPT (`wpt`, `css_wpt`) and all earlier targets pass. |
| Broker mode (`WEGA_TEST_BROKER=1`) | 5 targets (5, 4, 3, 8, 9 tests) passed |
| Required-worker JIT/storage/cookies/frames test | passes with the v23 helper |
| Existing `--smoke-test` | passes (engine, containers UI, chrome incl. developer-panel close) |

## 7. Performance (Phase J)

Conditions: release builds for A/B (`hot_path_bench`, `console_bench`), alternating runs on the same machine, **load average 1.8–7.7 while building, ≈ 2–5 while measuring**; 6 repeats (3 for `console_bench`); debug for subprocess IPC benchmarks (release workers refuse the unsandboxed development mode, so those are debug numbers).

**The unexplained `create_view+set_html` result from D1 is resolved: it was measurement noise.** Round 12 source (`e2a9cc9`), D1 and D2 release binaries run interleaved (6 repeats): 80.86 ms [77.7–83.4] / 78.94 [77.1–83.2] / 80.70 [77.6–81.8]. All 14 hot-path metrics agree within spread (largest D2-vs-R12 difference +0.9 %); none flagged. (The D1 comparison against the stored Round 12 JSON was taken at a different load.)

| Item | Result |
|---|---|
| **Console call cost, DevTools closed, release (D1 → D2, median of 3 runs × 7 samples)** | `log(string, number)` 6.08 → 11.39 µs/call (+5.3); nested object 21.8 → 32.5 (+10.7); `warn` 10.8 → 16.4 (+5.6); 8 numbers 8.7 → 16.8 (+8.2); 100-property object 106 → 120 (+14). **This is a real cost of always collecting** (so output before DevTools opens is retained): about +5 to +14 µs per call; a page that logs 10,000 messages pays ~50–140 ms. Debug build: +35 µs on the simple call. Optimisation candidates (not done): avoid the call-site op for non-error levels, build the entry in one host call. |
| Memory under 100,000 calls with object arguments (release) | resident growth D1 2.5 MiB → D2 10.7 MiB (peak 66 → 84 MiB). Includes the 1 MiB ring plus allocator/GC retention; flat after the first rounds (4 rounds × 20,000: 97 → 100 MiB in an earlier probe). |
| Evaluation round trip, `1+1` (debug, 300) | 0.134 ms median, p95 0.165 ms |
| Console history page, 100 entries from a full ring (debug, 200) | 2.75 ms median, p95 2.79 ms |
| 20,000-message flood | task 1788 ms disabled / 1773 ms enabled; delivered 1,000, reported dropped 19,000; quiesces 243 ms after the task; engine resident +9 MiB over two floods; browser queue peak ≤ 256 entries / 256 KiB (real flood test) |
| Navigation (debug, 30) | 88.4 ms median with the console enabled (D1: 87.9 closed / 90.0 attached) |
| Idle engine CPU, console enabled | 0.00 % (10 ms `ps` resolution) |
| Browser-side | with the panel closed no inspector call is made (view tests); no new periodic work beyond D1's 1 Hz snapshot while visible. UI CPU was not measured. |

## 8. Deferred, and why

* **Object handles / expansion**: needs a handle table with session+process+document lifetimes; previews first, as the brief allowed.
* **`awaitPromise`**: evaluation is synchronous under the watchdog; settling promises needs event-loop pumping outside the evaluation budget. A returned promise reports its state only.
* **`throwOnSideEffect`**: V8's side-effect-free mode is not wired in the vendored runtime; not claimed.
* **`console.table/time/count/group/trace`** keep their existing behaviour and are captured as formatter text; structured table/group rendering is not done.
* Console history is per document (cleared at navigation); no "preserve log".

## 9. Security findings and limitations

1. **Page code can still run in three places that pre-date D2 and that D2 does not change:** Deno's console formatter (a page-defined custom-inspect symbol and `%s` formatting run when the page calls `console.log`), the text of uncaught errors (`String(error)`/`.stack`, as before), and V8's formatting of an *evaluation* exception's stack (honours a page `Error.prepareStackTrace`). Previews, history and the panel add none.
2. A nested `console.*` call made from inside the formatter is not recorded (suppression flag).
3. A page can forge **its own** console entries through the host op; they are validated and bounded, and they carry no authority.
4. The grant stops a mistargeted, stale or replayed command; it does **not** defend against a compromised browser process, which owns the pipe. WebDriver `ExecuteScript` remains an unauthenticated evaluation path (D0 §6.2, out of scope; unchanged).
5. The opaque-origin policy is a **URL-scheme** test (http/https/file allowed, everything else needs the user's override), not the browsing-context origin; sandboxed frames and CSP sandbox are not considered (frames do not render).
6. Evaluation runs a classic script: top-level `let`/`const` cannot be redeclared by later commands, and it is refused while the document is loading.
7. A fatal evaluation (deadline, heap, stack overflow) ends the worker exactly as for page script. The browser-side timeout text says the evaluation "may still be running: not confirmed stopped"; the termination message appears only when the engine itself reported it.
8. Found and fixed during D2: (a) the console backlog drained only on unrelated wakeups (added pacing); (b) uncaught errors were recorded twice (via `script_error` and `__wega_report`); (c) exception names were lost behind the script-host prefix; (d) a decode failure for an unsupported version surfaced as `InvalidRequest` instead of `Unsupported`; (e) a scoped test of mine blamed the formatter's cost on capture (`console.log` of a 5,000-property object costs ~50 ms in the *pre-existing* formatter and 0.9 ms in the preview).
9. No new `unsafe` (diff grep: only a comment from D1). Lockfiles unchanged. No new listener or remote transport.

## 10. Platform and gate coverage

| Gate | Result |
|---|---|
| `cargo fmt --check` root, engines, tools | pass |
| Clippy `-D warnings`: root (default, `test-harness`, `--no-default-features`), engines workspace `--all-targets`, tools | pass |
| `cargo check --locked` root; `cargo metadata --locked` root, engines, tools | pass |
| Architecture checker | 13 members, **24 edges, 0 violations**, 11 informational (unchanged); dependency graph JSON: 24 edges, no cycles |
| `check-engine-boundary.py`; protocol, backend boundary text tests | pass |
| Unsafe inventory | 0 new |
| **Platforms** | macOS only. **Linux, Windows, MSVC Skia, native sandbox probes and packaged builds were not run.** The native end-to-end test uses the development worker; the sandboxed run is the engine-level test above, not the whole UI. The Console panel was not visually inspected (no screenshot); its rendering is covered by headless egui draw tests. |
| Disk | kept ≥ 4.7 GiB free; release artifacts deleted after the A/B; test binaries deleted per target |

## 11. Implementation sequence and remaining dependencies (D3–D7)

D0 design → D1 inspector foundation → **D2 console/evaluation (this)**. Remaining, per the roadmap: **D3** DOM/CSS (needs `wega_dom::inspect`; can reuse `PagePort`, the grant for any mutation, `devtools_snapshot`-style render-time validation, handles design), **D4** network (browser-side observer in `wega-network`; the `set-cookie` header prerequisite still open), **D5** debugger (V8 inspector prototype; depends on D2's evaluation gate and watchdog interaction), **D6** profiling/memory, **D7** UI consolidation. D3 and D4 can start in parallel.

## 12. What D3 can rely on, and what is unverified

**Can rely on:** `PagePort` as the engine-side seam (add methods, implement on `View`); `InspectorArgs` for parameterised commands and `Preview` for bounded values; the grant (add a capability or reuse it for mutation); per-command deadlines; the budgeted, paced delivery pattern for any event stream; `ConsoleView`-style bounded history in the UI; the real-V8 in-process test pattern, the subprocess harness (`inspector_worker.rs`), the sandbox test and the native `--inspector-test`.

**Unverified / D3 must check:** Linux and Windows; the full UI under the sandboxed worker; tab switch/close/restart through a real `Page` with the console (view-level only); the pinned detached OS window with the console; visual quality of the panel; behaviour with iframes (not rendered); release-profile IPC numbers (debug only); whether the always-on capture cost (+5–14 µs per console call) should be reduced before D3 adds more always-on hooks.
