# DevTools D2: Console, JavaScript runtime and secure evaluation (design)

Written and committed before any D2 code. Builds on [`devtools-d1-design.md`](./devtools-d1-design.md), [`devtools-d1-report.md`](./devtools-d1-report.md), [`devtools-design.md`](./devtools-design.md) and the roadmap's D2 section. Preflight facts verified in source: branch `feature/dom-rounds-9-12` at `3e8964d`, clean worktree, `VERSION = 22`, `INSPECTOR_VERSION = 1`, roadmap present, 8.2 GiB free (D1 exhausted the disk once; no release artifacts are kept).

## 1. Audit of the JavaScript path (Phase A)

| Facility | What exists (evidence) | Consequence for D2 |
|---|---|---|
| Runtime | One `V8Host` per document (`engines/wega/src/view.rs:652-669` creates a `Dom`, then `script::create`). Torn down and rebuilt on every document replacement. | A per-document console store created beside the `Dom` is destroyed with it. No second runtime. |
| Console | `web.js:759` builds `new console.Console((message, level) => op_dom_read("log", message, level))` using Deno's formatter. The callback receives an **already formatted string**; levels are 0 debug, 1 log/info, 2 warn, 3 error. Rust (`dom.rs:1604`) prints it to stderr and emits a `Diagnostic` for warn/error. No structured args, no stack, no timestamps, no method name (log vs info). | Capture needs a wrapper around the methods (exact method + argument values) plus the existing callback for formatter-derived text (`table`, `time*`, `count*`, `trace`, `assert`, `group*`, `dir`). Existing behaviour is preserved: the original method is still called. |
| Uncaught errors | `__wega_report(error)` (`semantics.js:88`) → host `"error"` with `message + "\n" + stack` → stderr diagnostic + 16-entry list → notices. Called for timer/event/microtask callback errors and by `core.setReportExceptionCallback`. | Add a structured host op beside it (name, message, stack, parsed first frame). |
| Promise rejections | `setUnhandledPromiseRejectionHandler` (`web.js:40`) → host `"error"` with `String(reason)+stack`. | Same structured op, tagged `unhandledrejection`. |
| Evaluation | `ScriptHost::evaluate_json` runs the source as a **classic script** (no `eval`, so page CSP is irrelevant) via `execute_script` under `bounded()` (watchdog thread terminates at `script_time`, default 5 s; a fatal result tears the realm down and the worker exits, proven by the D1-era `a_fatal_script_...` test), then `v8::json::stringify`s the result (which **runs `toJSON` and getters**, also under the watchdog). Errors collapse into `ScriptError{fatal, message}` text: no stack, line or column. | JSON serialisation is unacceptable for DevTools previews. D2 adds a second `ScriptHost` method that runs the same `execute_script` under the same `bounded()` watchdog, converts exceptions to structured data from `deno_core`'s `JsError`, and builds the result preview with a side-effect-free generator. |
| Host ops | Page script can reach `Deno.core.ops.op_dom_read(...)` only through the bootstrapped surface; every op is producer-side (read DOM, log, error). None accepts a command. | New console ops are producers only. A page can forge *its own* console entries (harmless); it cannot create a session or a grant (those exist only in the browser/engine inspector state set by pipe commands). |
| Execution context | One main world per document; `DocumentContext{document, revision}` is the existing context identity. | `ExecutionContextCreated/Destroyed` carry that stamp; no second world. |
| Call sites | `Deno.core.currentUserCallSite()` returns `{fileName, lineNumber, columnNumber}` natively without formatting `Error.stack`. | Used for console source locations: avoids `Error.prepareStackTrace` (a page hook) on the hot path. |
| Proxy/promise introspection | `Deno.core.isProxy`, `getPromiseDetails` exist. | Proxies are never inspected (type only). Promises report state only. |

## 2. Protocol (Phase B)

* `VERSION` 22 → **23**, `Capability::InspectorConsole` (>= 23), `INSPECTOR_VERSION` 1 → **2** (`supports_inspector_version` accepts 1..=2; v1 requests behave exactly as in D1; v2-only commands from a v1 request are `UnsupportedVersion`). The browser already requires `Ready{VERSION}` equality, so a v22 worker is rejected at the handshake as before; mixed pairs cannot exist at runtime. Protocol-level decoding of v22 sessions is unchanged (`supports_version` stays 4..=VERSION).
* `InspectorCommand` stays a Copy, parameterless discriminant (it is the correlation key in `RequestKind::Inspector`). New variants: `EnableConsole, DisableConsole, ClearConsole, GetConsoleHistory, GrantEvaluation, RevokeEvaluation, Evaluate`. Parameters travel in a new `InspectorRequest.args: InspectorArgs` (`#[serde(default)]`, closed enum: `None`, `History{after, limit}`, `Grant{token, opaque_override}`, `Evaluate{expression, grant}`); the receiver rejects an `args` variant that does not match `command` with `InvalidPayload`.
* Responses: `ConsoleEnabled{retained, first_seq}`, `ConsoleDisabled`, `ConsoleCleared`, `ConsoleHistory{messages, next_seq, dropped}`, `EvaluationGranted`, `EvaluationRevoked`, `Evaluated(EvalOutcome)`.
* Events (`InspectorEvent` loses `Copy`): `ConsoleMessageAdded(ConsoleMessage)`, `ExceptionThrown(ExceptionDetails)`, `ExecutionContextCreated{document}`, `ExecutionContextDestroyed{document}`, `ConsoleMessagesDropped{count}`.
* **Deferred, with reason:** `GetObjectProperties` and `ReleaseObject` (no live handles exist in D2: previews are values, not references, so there is nothing to leak or to go stale; expandable objects need a handle table with per-realm lifetimes and are an explicit acceptance limitation); `awaitPromise` (evaluation is synchronous under the watchdog; settling a promise needs event-loop pumping that would run page tasks outside the evaluation budget; a returned promise is reported with its state only); `throwOnSideEffect` (V8's side-effect-free mode is not wired into the vendored runtime and I will not claim it).
* Types: `Preview{kind, text, truncated, props: Vec<PreviewProp>, more}` (depth ≤ 3, ≤ 8 props/level, ≤ 64 nodes, text ≤ 256 chars; validated by `Preview::is_bounded` at the consumer), `ConsoleMessage{seq, timestamp_ms, level, method, args, text, source, stack}` (≤ 8 KiB serialised; producer shrinks previews to fit), `ExceptionDetails{kind (uncaught / unhandled rejection / evaluation), name, message, stack, url, line, column}`, `EvalOutcome::{Value{preview}, Exception{details}}`, `ConsoleLevel{Debug,Log,Info,Warn,Error}`.

## 3. Console capture (Phase C)

**Engine, per document:** a `ConsoleLog` (`Rc<RefCell<…>>` in `HostContext`, created with the `Dom`, therefore dropped at document replacement) holds a ring bounded by **1000 entries and 1 MiB** of accounted bytes (drop-oldest; `dropped_total` counts evictions) plus a monotonically increasing `seq`. It collects **always**, so messages logged during load or before DevTools opens are retained; with DevTools closed the only cost is the preview build and one ring push.

**JS side** (`web.js`): `__wega_preview(value)` (a non-enumerable global holding no authority) builds the `Preview` using only intrinsics captured at bootstrap: it checks `core.isProxy` first, reads data descriptors only (never getters, never `toJSON`, never `toString`, never `Symbol.toPrimitive`/custom inspect), detects cycles (`[Circular]`), applies the depth/width/node/text budgets, and returns a type-only preview for exotic objects it will not enumerate (typed arrays report length). Console methods `log/info/warn/error/debug` are wrapped: record `{method, previews, callsite}` then call the original exactly as before. Formatter-derived methods (`table, time*, count*, trace, assert, dir, group*`) keep Deno behaviour and are captured from the existing callback as text at the callback's level. Error arguments preview as name + message from own data properties only (no `stack` read, so no `Error.prepareStackTrace` hook); the call site comes from `currentUserCallSite()`. Uncaught errors and unhandled rejections use `__wega_report`/the rejection handler to emit a structured op carrying name/message/stack (the same reads those paths already did) plus the first frame parsed Rust-side.

Known limitation: a nested `console.*` call made from inside Deno's formatting (a page-defined custom inspect) is not recorded while the outer call is formatting (suppression flag to avoid double capture). Documented in the report.

**Delivery:** a session that sent `EnableConsole` has a cursor into the ring. Once per engine loop iteration (`observe`) the backend sends at most **32 messages / 64 KiB** past the cursor (fair: navigation, commands and frames run in the same iteration); a cursor that fell behind the ring's first sequence yields one `ConsoleMessagesDropped{count}`. Enable backfills from the ring start (messages before the panel opened). `GetConsoleHistory{after,limit≤100}` pages the ring (bounded to 64 KiB) for reconnect/backfill. `ClearConsole` empties the ring and advances the cursor. With no enabled session `observe` does not touch the ring.

**Browser side:** `HostSessions` gets a byte-bounded console queue (256 messages / 256 KiB, drop-oldest, counted into `ConsoleMessagesDropped` markers) fed by the reader thread; `DevtoolsView` drains it every tick into its own history (**1000 entries / 1 MiB**, drop-oldest, counted) and surfaces the drop count. Closing the panel detaches (queue destroyed with the session); nothing accumulates while the UI is closed. The reader thread never blocks on the UI.

## 4. Evaluation authority (Phase D, security-critical)

Opening DevTools or the Console grants nothing. Evaluation needs a **grant**, a stateful object that exists in both processes:

1. The user clicks **Enable evaluation** in the trusted console panel (native egui; page script cannot click it). For an opaque/internal document (scheme not `http`, `https`, `file`) the button reads "Enable anyway" and the grant carries `opaque_override = true`; without it the engine refuses.
2. The browser mints `GrantToken` = 128 random bits (`getrandom`), stores `Grant{session, process target, document stamp, token, opaque_override}` in that page's `HostSessions` and sends `GrantEvaluation`.
3. The engine's `Backend` stores `Grant{session, document stamp, token}` only after verifying: live session, matching target, version ≥ 2, **document origin policy**. It never accepts "authorized" as data.
4. `Evaluate{expression, grant}` is executed only if (engine side, independently) the stored grant exists **and** the session is the live one **and** the token equals the stored token **and** the stored document stamp equals the *current* document stamp. The browser makes the same check before sending, so a stale UI cannot even emit it.
5. Revocation: engine clears the grant on detach, supersede, `RevokeEvaluation`, session change and **any document change** (also verified at use, not only at clear time); browser clears on revoke, supersede, `close`, process death (`failed()`), tab drop and on every navigation (`bind_principal` path). `DevtoolsView` also clears its "enabled" indicator on any document or session change. The panel shows a persistent "Evaluation enabled for this document" banner while a grant is live.

Why a token if only the browser can write the pipe? It makes authority a verifiable *state* rather than a flag in a message: a replayed or mis-targeted evaluate from a browser-side bug (old session, other tab's pipe, pre-navigation request) fails the engine's own check; a guessed session id is useless without the token; the token never leaves the two processes (not displayed, not logged, not in WebDriver or any URL). It is not a defence against a compromised browser process (out of scope; the browser owns the pipe).

Denials are `Unauthorized` (no/foreign/bad grant), `StaleSession`, `StaleDocument`, or the new `PolicyDenied`-class message under `Unauthorized` for opaque origins (the code set from D1 is reused, with distinct messages).

## 5. Evaluation (Phase E)

`ScriptHost::evaluate_inspect(source) -> Result<InspectResult, ScriptError>` in `wega-adapter-v8`: `execute_script` under `bounded()` (same watchdog and heap budget as `evaluate_json`), exception → `JsError` structured (name, message, stack, source URL, line, column), success → `__wega_preview` invoked under a second `bounded()` section. A fatal result (deadline, OOM, stack overflow) is returned as `ScriptError{fatal:true}`; the existing path tears the realm down and ends the worker, and the browser reports the *confirmed* outcome ("the engine terminated the page's script runtime (ScriptStopped)"). The browser-side request timeout for `Evaluate` is 6 s (script deadline 5 s + margin) versus 2 s for other inspector commands; a browser-side timeout is reported as "no answer; the evaluation may still be running" and never as termination. Expression limit 16 KiB; results are previews (≤ 16 KiB serialised). The evaluation runs in the main world; no second world. Console output produced by an evaluation is captured like any other.

## 6. Object handles (Phase F)

Not implemented (see §2). Previews are self-contained values; no JavaScript object is retained by the inspector, so there is nothing to release, expire or reuse. The UI shows expandable-looking structure only as far as the bounded preview goes and says so. D3/D5 will introduce handles with session+process+document scoping.

## 7. Budgets (Phase G)

| Resource | Bound | Overflow behaviour |
|---|---|---|
| Engine ring | 1000 entries and 1 MiB per document | drop-oldest, `dropped_total` |
| One message | ≤ 8 KiB serialised, preview ≤ 64 nodes/depth 3/8 props/256-char text | truncated flags |
| Engine delivery | ≤ 32 messages and 64 KiB per loop iteration | cursor lag → `ConsoleMessagesDropped` |
| Pipe message | existing 1 MiB frame cap, far above 8 KiB | n/a |
| Browser queue | 256 messages / 256 KiB per session | drop-oldest + counter |
| UI history | 1000 entries / 1 MiB | drop-oldest + visible counter |
| Pending inspector requests | 4 (unchanged) | `Busy` |
| Evaluate expression | 16 KiB | `InvalidPayload` |

A hostile page can make the engine spend CPU on previews (bounded per call); it cannot make any queue exceed the above, and it cannot make the DOM thread block on the UI: writes are bounded per iteration and the browser reader drains unconditionally.

## 8. UI (Phase H)

The retained panel gains a **Status | Console** switch. Console: message list (severity colour + glyph, `HH:MM:SS.mmm`, source `url:line:col`, text/preview, exception stack collapsible), text filter, severity toggles, Clear, dropped-message banner, disconnected / not-responding states (reuse `ViewState`), the **Enable evaluation** control with the persistent enabled banner, a single-line input with Enter-to-run and Up/Down command history (100 entries, in memory only, cleared with the session), results/errors appended to the same list. All engine data passes the same render-time target/document validation as the status snapshot. Docking and the pinned detached window behave as in D1.

## 9. Tests (Phase I) and performance (Phase J) — plan

Protocol unit tests (round trip, malformed args, bounds, grant registry negatives); engine unit tests (ring bounds, preview rules, grant checks); **real-process** tests for all 20 items in the brief, extending `tests/support/inspector_worker.rs`; real `Page` end-to-end via `--inspector-test` stages; an attempt at the required (sandboxed) worker through the `inherited_broker` harness; benchmark additions to `inspector_bench` (capture overhead closed/open, flood memory, evaluate and history latency, throughput/drops) and a **controlled re-run of `create_view+set_html`** (before/after comparison against a build of the D1 commit in a separate target directory only if disk allows; otherwise repeated runs with recorded load). No regression is claimed without matched measurements.

## 10. Risks

* The preview generator is security-relevant JavaScript running in the page realm; it is built only from intrinsics captured at bootstrap and unit-tested against hostile objects (accessors that throw/count calls, proxies with traps, `toJSON`, `toString`, cyclic and huge structures, `Symbol.toPrimitive`).
* `console` wrapping must not change observable behaviour (return value, `this`, arity); tested.
* `Error.prepareStackTrace` is a page-controlled hook reachable only for *uncaught-exception stacks* (as today) and evaluation exceptions (V8 builds `JsError` stack); previews never read `stack`.
* Disk: engine test targets are run one at a time, test executables deleted after each, no release builds kept.
