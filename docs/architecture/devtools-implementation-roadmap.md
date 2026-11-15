# Wega DevTools implementation roadmap (D1–D7)

Plan; **D1, D2, read-only D3-A and the supported read-only D3-B scope are implemented locally** (see [`devtools-d1-report.md`](./devtools-d1-report.md), [`devtools-d2-report.md`](./devtools-d2-report.md), [`devtools-d3-a-report.md`](./devtools-d3-a-report.md), [`devtools-d3-b-report.md`](./devtools-d3-b-report.md)). D3-B matched rules remain explicitly unsupported after the runtime feasibility stop. D3-C and D4–D7 are not started. Design: [`devtools-design.md`](./devtools-design.md). Evidence: [`devtools-pr58-deletion-audit.md`](./devtools-pr58-deletion-audit.md), [`devtools-current-capabilities.md`](./devtools-current-capabilities.md).

**Complexity scale** (one engineer, rough, includes tests and docs): S ≤ 1 week, M 1–3 weeks, L 3–6 weeks, XL > 6 weeks. These are estimates, not measurements.

**Rules for every round** (acceptance gates, not repeated below):
1. No Blitz, V8, Deno or Servo type in `wega_protocol::inspector`; a boundary test enforces it (D1) and a second test forbids `blitz_` references under `engines/wega/src/inspector/` (D2).
2. **Zero cost when closed:** no hook runs unless a session has enabled the domain. Verified by `scripts/engine-bench.py` against `docs/architecture/baseline/engine-bench-round12.json`: no metric changes beyond its spread with DevTools closed.
3. Every event stream: bounded ring, drop-oldest with a counted marker for log-like events, coalescing for state events, credit window (design §6.4); a flood test per stream.
4. Every id carries its document stamp; every domain has a navigation test (id from before a navigation fails with the right stale error, never resolves to something else).
5. No new `unsafe`; no dependency cycle (`architecture-check` 0 violations; `cargo metadata --locked` at root, engines, tools); full engine suite and broker-mode suites pass; formatting and clippy `-D warnings`.
6. Remote debugging stays off and unimplemented; WebDriver behaviour unchanged.

## Cross-cutting prerequisites (before or inside D1)

| Item | Why | Size |
|---|---|---|
| A trust-boundary decision record: where `DevtoolsAuthority` lives (root crate, per tab), the grant names, and what a grant is bound to (`DocumentContext`) | the audit found `evaluate` has no capability gate | S |
| Verify what response headers (notably `set-cookie`) the broker returns to the engine | design §9; decides whether redaction must also be applied on the engine-facing path | S |
| Decide the inspector message schema format and add a schema/golden-file test | stable protocol and fuzzable parser | S |

## D1: Inspector protocol, sessions, targets, lifecycle, IPC

**Scope.** `wega_protocol::inspector` module: envelope, error codes, id types (`TargetId`, `InspectorNodeId`, `RemoteObjectId`, `RequestId`), limits, `INSPECTOR_VERSION`; `Command::Inspector`/`Event::Inspector` variants, `Capability::Inspector`, `VERSION` bump with the `content_event()` downgrade arm; content-side dispatcher in `wega-engine` (`inspector/mod.rs`) that routes by domain and returns `Unsupported`; browser-side `InspectorClient` and `DevtoolsAuthority` in the root crate (`devtools/`), session attach/detach, credit-window flow control, per-session bounded queues, lifecycle on navigation/crash. One real domain end to end: `Target` (list/attach/detach, `documentChanged`) plus a `Runtime.getPageInfo`-style status call replacing the current status-panel reads, so the pipeline is proven before D2 depends on it. The status panel is re-fed through the client (no behaviour change for users).

**Reuse.** `RequestContext`/`DocumentRequests` correlation, deadlines, cancellation and the 32-pending cap; `SessionIdentity`; 1 MiB framing; `Capability`/version gating; `State` event; the dock shell, profile persistence and F12/shortcut plumbing.

**New interfaces.** `InspectorRequest/Response/Event` types; `InspectorClient` (async-free, channel-based, used by egui on the UI thread); engine `InspectorBackend::handle(request, &mut View)`; `DevtoolsAuthority::{grant, revoke, check}`; `inspector::Limits`.

**Depends on.** Cross-cutting prerequisites only.

**Security risks.** A new command family on the pipe (malformed JSON, oversize, replay); grant confusion across tabs/containers; an inspector request that outlives its document; protocol-version downgrade mishandling.

**Tests.** Unit: encode/decode round trips and golden files, oversize/truncated/unknown-method rejection, version negotiation and downgrade, credit-window accounting, drop-oldest/coalescing behaviour, authority grant/revoke/expiry on navigation. Integration (engines): attach, request, stale-document rejection after `set_html`/navigation, cancellation, process-death cleanup, broker-mode run of all of it, a flood of events with a slow consumer (memory stays bounded). Root crate: panel shows data through the client; session closes on tab close. Fuzz target for the inspector message parser (new infrastructure, matches the roadmap item in the Round 12 report).

**Acceptance.** The status panel's values arrive via the inspector protocol; closing the tab or navigating invalidates sessions as specified; all rules above hold; a protocol schema document is checked in; the suite counts rise only by the new tests.

**Complexity.** M–L.

**Non-goals.** Any inspection domain beyond `Target` and page info; UI redesign; network, DOM, console; remote transport; WebDriver changes.

### D1 actual progress and deviations (implemented)

Done: protocol module, session authority, engine status backend, UI re-fed through the client, reliability/security tests, native end-to-end check, performance measurements. Commits `47dc779`, `584843e`, `8bcfe48`, `b0770d5` (+ the report commit). Deviations: typed enums instead of `Domain.method`; no `Target.list`; no credit-window flow control (bounded ring + change-driven events instead); no fuzz target or schema golden file; `RemoteObjectId`/`InspectorNodeId` not introduced (no consumer yet); the `set-cookie` header prerequisite moved to D4; `VERSION` is now 22 with no downgrade arm (inspector events exist only for v22 sessions). Open items carried forward: inspector under the sandboxed worker, Linux/Windows runs, re-measure `create_view+set_html` on a quiet machine, evaluation capability gate (D2).

## D2: Console events, runtime evaluation, exception reporting

**Scope.** `Console` and `Runtime` domains: intercept `console.*` at `web.js` for **all levels** (structured arguments with bounded depth/length previews, accessors not invoked, timestamp, call-site/stack from V8 message data, document stamp); upgrade `ScriptError`/uncaught-error reporting to carry exception details (message, name, stack, source URL, line, column) and promise rejections; per-document console history ring (1,000 entries) and `Console.getHistory` with a cursor; `Runtime.evaluate` (value/preview, awaitPromise, `throwOnSideEffect` for previews) and `getProperties`/`releaseObject` over bounded object groups; browser-side Console panel with the evaluation grant UX (explicit user action, logged, cleared on navigation); an in-memory per-document diagnostics store for parser/CSS warnings (`WEGA_DIAGNOSTIC`) that the Console panel merges. Reuse `console_eval.js` (await support, 4-level previews) as the specification for result rendering; its logic is re-implemented inside the evaluate path rather than injected.

**Reuse.** `execute_script` under the existing watchdog and 128 KiB result cap; `Diagnostic` type and its bounds; the removed `console_model` design (filtering, 100-entry command history, repeat grouping, 1,000-entry cap) and `console_ui_tests` assertions as acceptance criteria; the `ScriptHost` trait.

**New interfaces.** `ConsoleMessage`, `RemoteObject`/`ObjectPreview`, `ExceptionDetails`; `ScriptHost::evaluate_inspector(expr, options) -> Result<RemoteObject, ExceptionDetails>` in the V8 adapter; `console` capture hook in `web.js` guarded by a "console enabled" flag; host op for structured log events.

**Depends on.** D1.

**Security risks.** Evaluation runs in the page's main world with full page authority and bypasses page CSP `unsafe-eval` (existing property of `execute_script`): the grant gate is the only control; preview evaluation must not trigger side effects or getters; large/cyclic object graphs and hostile `toString`/`Proxy` objects; console flooding; leaking cross-origin data through previews; stack traces revealing cross-origin script URLs (same data the page can already see; cross-origin scripts without CORS must be redacted like `window.onerror`).

**Tests.** Unit: preview depth/length/cycle limits; accessor not invoked; special numbers (NaN, ±Infinity, -0); repeat grouping and ring eviction; error-shape serialisation. Integration: every console level reaches the client with the right stamp; uncaught error and unhandled rejection with line/column; `evaluate` without a grant is refused; with a grant runs under the watchdog (an infinite loop returns the fatal path and the session survives the realm teardown); evaluate after navigation fails with `StaleDocument`; flood test (1e5 `console.log` in one task) keeps the ring and event queue bounded with a `dropped` count; the old assertions (log/warn/error/uncaught entries with source and line, `console.trace` stack, object previews, CSS parse messages) as new tests.

**Acceptance.** All console levels visible with structured arguments, source and stack; evaluate works for expressions and `await`; errors show stack/line/column; no measurable overhead with the console closed; no event or memory growth beyond the caps under the flood tests.

**Complexity.** M–L.

**Non-goals.** The debugger (D5); Runtime on workers; autocompletion; source maps; evaluation in isolated worlds (the engine has one world).

### D2 actual progress and deviations (implemented)

Done: console capture (all five structured methods plus formatter output), bounded per-document history that also retains pre-DevTools output and uncaught errors, side-effect-free previews, exceptions with stack and location, explicit user-granted evaluation with an engine-verified grant, Console panel, real-V8, real-process, sandboxed-worker and native end-to-end tests, benchmarks. Commits `bdb9519`, `05f0d88`, `183aae6`, `6c7ab86`, `4371f47` (+ benchmarks and report). Deviations from the plan above: **no `Runtime.getProperties/releaseObject`, no `awaitPromise`, no `throwOnSideEffect`** (no handles in D2; see report §8); evaluation uses a `GrantEvaluation` token rather than the planned `DevtoolsAuthority` grant names; no credit window (budgeted engine delivery, paced wakeups, drop-oldest queues with in-band markers); `ExceptionThrown` reuses the console message type; `console.*` capture is always on (measured +5 to +14 us per call in release); the V8 inspector is not involved. Open items carried forward: Linux/Windows runs, the whole UI under the sandboxed worker, deciding whether to reduce the always-on capture cost, an evaluation capability for WebDriver (still unauthenticated), object handles (D3/D5).

## D3: DOM tree, selection, computed styles, CSS editing, layout highlighting

**Scope.** `DOM` and `CSS` domains and the Elements panel. (a) DOM: bounded `getDocument`/`requestChildNodes`, node description, attribute/text edit and node removal through `Mutator`, `querySelector(All)`, coalesced change events from the revision counter; `inspectAt`/picker built on `HitTest::element_at`; `getBoxModel` (content/padding/border/margin) and UI-drawn overlay. (b) CSS: bulk `getComputedStyle`, **new `wega_dom::inspect` contract** (stylesheet and rule listing, matched rules with origin, specificity and order, source ranges) implemented in `wega-adapter-blitz` over Stylo, inline-style editing through `StyleControl`, and rule-text editing as a separate `StyleEdit` capability (all-or-nothing, invalid text ignored like `setProperty`). (c) Selected-node follow: UI keeps the selection across `documentUpdated` where the node survives (by key and revision) and clears it otherwise.

**Reuse.** `DocumentView`/`NodeView`/`DocumentMutation`, `StyleAccess`/`StyleControl`, `GeometryAccess`, `HitTest`, host ops (`children`, `attributes`, `outerHTML`, `rect`, `box`); the shadow/custom-element host ops for labelling; the Round 9–10 adapter tests as templates; the old inspector's assertions (nodes include `id=heading`, `color: rgb(12, 34, 56)`, `padding-top: 17px`, `display: flex`, non-empty rules, ≥ 10 computed entries, edit and undo) as acceptance criteria; `inspector.js` is a reference for the snapshot shape only.

**New interfaces.** `wega_dom::inspect::{StyleInspect, SheetInfo, RuleInfo, MatchedRule}` and later `StyleEdit`; `DOM.*`/`CSS.*` messages with paging cursors; box-model type with all four boxes (the current `box` op lacks margin and border).

**Depends on.** D1; D2 only for shared UI plumbing. The `inspect` contract can start in parallel with D1.

**Security risks.** DOM edits as a script-independent mutation path (budgets must apply; edits must not create script-visible capability, e.g. inserting `<script>` runs it: edits inserting active content are subject to the page's CSP and the same checks as script insertion, and are listed as such in the UI); node ids crossing a navigation (`StaleNode` tests); very large subtrees (paging, byte budgets); style/rule text injection (parsed by Stylo, never evaluated); overlay drawing must not read pixels the user cannot see; picker must not leak cross-origin iframe content (child frames are not rendered today, so none exists; the test asserts the picker never resolves into a frame target).

**Tests.** Adapter-level (engines): rule listing and matched-rule parity against fixtures covering origin (UA/author/inline), specificity ties, `!important`, media-conditional rules, `:hover` state rules, custom properties, shadow gap behaviour (pinned like the Round 9 tests); property-bulk computed equals the single-property reads for every property. Engine integration: tree paging with budgets, edit then read-back, undo, stale node after mutation and navigation, revision coalescing under a mutation storm, hit-test picker over scroll and zoom (reusing the Round 10 coordinate conventions), box-model values for the Round 10 fixtures. Root: panel snapshot tests of tree rendering and style pane ordering.

**Acceptance.** Elements panel navigates the live tree, shows matched and computed styles with cascade order, edits attributes/text/inline style and (if the contract lands) rule text with undo, highlights the selected node and picks by clicking; every old inspector assertion above passes against the Wega engine; no second DOM or stylesheet copy exists (a test asserts the backend retains only ids and counters).

**Complexity.** XL (the `inspect` contract and adapter dominate).

**Non-goals.** Pseudo-element styles, event-listener inspector (needs a registry export from `semantics.js`; tracked separately), shadow-tree styling, animation inspection, computed-style *tracing* (which rule won each property beyond order and specificity), accessibility tree viewer (data exists; separate small panel after D7).

### D3-A actual progress (implemented locally)

The D3-A slice is deliberately **read-only**: expandable paginated Elements tree,
metadata/attributes/own-text previews and selection, using the authoritative Wega
DOM, stamped generation-bearing IDs and D3.0's separate mutation epoch. Engine
protocol 24 / inspector 3. Bounded backend, UI cache/page windows and existing
browser session authorization; no injected inspection JavaScript or layout flush.
Design was committed first (`2c71136`); implementation stages `9bc0c35`, `f18d0c0`,
`678a002`, `ed8e688`. Both prerequisite investigations and the full local validation
matrix are recorded in [`devtools-d3-a-report.md`](./devtools-d3-a-report.md).

D3-B subsequently closed the in-sheet CSSOM generation gap in the existing
invalidation paths. D3-C geometry work remains constrained by the known
depth-122 first-flush hang. None of the combined D3 editing or highlighting
scope above is implied complete by D3-A/B.

### D3-B actual progress (implemented locally, supported scope)

The Elements panel now exposes author stylesheet/rule/declaration pages, inline
declarations and 23 cached scalar computed longhands through Wega-owned borrowed
contracts. Engine protocol 25 / inspector 4. Reads never request layout, selector
matching or page JavaScript; references bind the existing target/session/document
authority plus mutation epoch and stylesheet generation. Traversal, serialization,
response bytes, UI retention and pending work are bounded.

Design and executable spike were committed first (`9b08a19`). The spike disproved
reliable matched-rule recovery from retained declaration blocks, so that subfeature
stopped with explicit Unsupported responses. No guessed selectors, specificity or
cascade winners are shown. See the design and report for the runtime counterexample,
two narrow documented vendor patches, exact tests and measurements. Shared-host
console timing uncertainty remains recorded in the report.

D3-C must first resolve the layout hang or prove a bounded/cancellable geometry
contract. Matched rules need a separate provenance/matching design, and wider
computed values need bounded serialization and freshness semantics. No editing,
picker, overlay or D3-C work was added, and nothing was pushed.

## D4: Network request and response inspection, timing, filtering, safe body access

**Scope.** `Network` and `Storage` domains and the Network/Application panels. Browser-side `NetworkObserver` in `wega-network` called from `Broker::accept`'s `Fetch` arm and `fetch_cached` (request start, redirect hops, cache hit/miss, CORS result, response head, body size, finish/failure) and the WebSocket open/IO path; request ids; timings from `Instant` (queue, DNS+connect as far as reqwest exposes them, TTFB, download; unknown phases reported as unknown, not zero); per-session bounded ring (500 rows) with drop-oldest and a counter; redaction at the tap; opt-in body capture (64 KiB default, cap 8 MiB); filtering/sorting in the UI; HAR 1.2 export (with unknown timings as −1 as the old exporter did) and a validated import; cURL copy; cookies table from the browser-side inventory (HttpOnly masked); read-only storage enumeration (localStorage/sessionStorage, cache names, IndexedDB database names and sizes) through the existing services; "edit and resend" deferred (see non-goals).

**Reuse.** The broker choke point and `ResourceDestination`; `Cookies::inventory`; `downloads::Job` progress for download rows; the removed `network.rs`/`network_har.rs`/`network_controls.rs` as design and bounds reference and their assertions as criteria (POST 201 body and response header visible, redirect chains, bounded bodies, tab isolation, HAR structure, filters/sorts, security summary); the profile policy (offline/blocked hosts) is shown, not duplicated.

**New interfaces.** `wega_network::observe::{NetworkObserver, NetworkEvent, Redaction}`; `Network.*`/`Storage.*` messages; HAR module (UI side, pure functions).

**Depends on.** D1. Independent of D2/D3.

**Security risks.** The highest-sensitivity round: credentials in headers, cookies, bodies and URLs; cross-container leakage (observer instances are per broker session and the UI session is bound to one container; a test asserts a second container's traffic never reaches the first session); response bodies of opaque or cross-origin `no-cors` responses; HAR files as a leak vehicle (redacted by default, explicit export, safe file paths); observer overhead or failure affecting fetches (it must be a non-blocking tap that cannot fail a request); verifying whether `set-cookie` reaches the engine unfiltered (prerequisite).

**Tests.** Unit: redaction table (`cookie`, `authorization`, `proxy-authorization`, `set-cookie`, case variations, duplicate headers), ring eviction, timing arithmetic, HAR round-trip and malformed-import rejection (size and header limits), body cap and binary handling. Integration (engines/network + engine): redirect chain with hop records, cross-origin credential stripping visible, cache hits, CORS preflight rows, WebSocket open and close, failure rows (blocked by policy, offline), per-container isolation, observer-off byte-for-byte equivalence of responses, flood test (1e5 requests) bounded memory, a request issued during navigation attaches to the right document. Revealing sensitive values requires the grant (negative test).

**Acceptance.** Every HTTP and WebSocket request of a page appears with method, URL, status, type, initiator document, size and honest timings; sensitive values are redacted until deliberately revealed; bodies only with the grant and within caps; HAR export/import work; the observer adds no measurable cost to fetches when closed and a bounded cost when open (measured with the broker benchmarks added in D4).

**Complexity.** L.

**Non-goals.** Throttling and offline emulation (the old build could not either; the profile policy remains the only control), request blocking/modification, "edit and resend" (it must go through the page's fetch path with the page's authority and needs its own design), service-worker request attribution, HTTP/2 frame detail, TLS certificate viewer.

## D5: JavaScript sources, breakpoints, stepping, stack traces, scopes

**Scope.** `Debugger` domain and the Sources panel on **V8's inspector**: prototype first (lazily attach with `maybe_init_inspector`, `LocalInspectorSession`, the event-loop pump) then productise: `scriptParsed` listing with sources (bounded: 250 files × 1 MiB as the old build did), `setBreakpoint`/`removeBreakpoint` (line/column, conditional later), `pause`, `resume`, `stepOver/Into/Out`, call frames with scope chains and bounded `RemoteObject`s, `evaluateOnCallFrame`, stack traces for uncaught errors and `console.trace` (completing D2), pause on uncaught exception; watchdog integration so a paused page is not terminated and so a pause is cancelled on navigation or tab close; CDP-JSON tunnel through `Inspector` messages (the protocol carries the inspector's own JSON for this domain, wrapped and bounded).

**Reuse.** The vendored deno_core inspector (compiled in, unused); the watchdog (`host.rs`); `ScriptStopped`; the old debugger's capability list as the explicit "not supported yet" list (conditional/log/XHR/event/DOM breakpoints, source maps, worker targets) and the harness assertion (`paused_line == Some(5)`, real script context, frame present) as the acceptance test.

**New interfaces.** `wega-adapter-v8` `InspectorChannel` (strings in, strings out, `pump()`), `ScriptHost::{attach_inspector, detach_inspector}`; `Debugger.*` messages; pause-state in the session manager.

**Depends on.** D1, D2 (stack/exception shapes, `RemoteObject`).

**Security risks.** Pausing blocks the DOM thread: a hostile or forgotten pause is a self-inflicted hang (resume on detach, on navigation, on tab close and on UI disconnect; a pause timeout policy); the watchdog must not be defeated by a pause *or* disabled permanently by a stuck session; inspector attach changes V8 behaviour (script lifetime, performance) only while attached; exposure of cross-origin script sources (CORS-gated like the page's own view); the debugger evaluating in call frames needs the same grant as `Runtime.evaluate`; V8 inspector messages are untrusted-format JSON inside the browser: parsed with the same bounded parser and never forwarded to the page.

**Tests.** Prototype acceptance (written first): scripts parsed before attach are listed; breakpoint hit; pause while the watchdog deadline passes does not terminate; detach resumes. Then integration: breakpoints at lines and columns, stepping sequences, scope contents with bounded previews, exception pause, navigation while paused (clean resume and teardown), tab close while paused, two sessions refused, memory growth across 1e3 attach/detach cycles, performance with the debugger detached (benchmarks unchanged), broker-mode run.

**Acceptance.** User can open a script, set a breakpoint, hit it, see stack and scopes, step, resume; page and engine stay responsive and the watchdog semantics are intact in every other state; teardown on navigation/close always resumes and detaches; unsupported features are listed in the UI rather than silently missing.

**Complexity.** XL (prototype risk is high; fall-back is enabling the inspector at runtime creation behind a per-tab policy flag, which costs a reload).

**Non-goals.** Source maps, worker/service-worker debugging, blackboxing UI, live edit of running code, async stack traces beyond what V8 provides by default, WebAssembly debugging.

## D6: Performance timeline, memory, layout and paint diagnostics

**Scope.** `Performance`, `Profiler`, `Memory` domains and the Performance panel. Timeline events from the engine behind a flag: frame record (flush, paint prep, `blitz_paint`), rasterise, submission (the stage timings the Round 11 benchmark measures), layout flushes with node counts and invalidation reason where the engine knows it, script task durations (slow-script notices exist), network milestones joined from D4, long-task detection; metrics (`js_heap`, RSS, nodes, DOM allocation counters); V8 CPU profile through the inspector (`Profiler.start/stop`) and heap snapshots (`HeapProfiler`) if D5's attach proves workable; paint/layout diagnostics: paint-flash and layout-border overlays driven from `GeometryAccess` and the frame stage timings, plus the idle-repaint indicator (frames painted vs requested, which also instruments the known Round 11 idle-repaint issue). The old Performance panel (Performance-API entries plus rAF intervals) is reproduced as one *source* of events, injected only with an evaluation grant, not the primary mechanism.

**Reuse.** Frame timings and counters already benchmarked in `render_bench.rs`/`hot_path_bench.rs` (move the measurement points into flag-guarded probes); `State` metrics; `wega-platform` memory/RSS; the V8 heap statistics already used by the heap limit; `scripts/engine-bench.py` to prove closed-cost; the old `performance_ui` assertions as acceptance (resource/mark/measure/paint/longtask entries appear).

**New interfaces.** `Performance.*`/`Profiler.*`/`Memory.*` messages; probe points (`FrameProbe`, `LayoutProbe`) as no-op-when-disabled traits implemented by the inspector backend; timeline event types with a monotonic clock shared with D4.

**Depends on.** D1; D4 (network milestones), D5 (profiler/heap if via the inspector); layout/paint probes independent.

**Security risks.** Timing and memory data are side-channel material: they are shown only to the UI of the same tab, never to the page; heap snapshots contain every string in the heap (cookies, tokens, form data) — a heap snapshot requires the strongest grant, is never persisted by default and is excluded from any export; profiling overhead causing hangs; unbounded timelines (ring with a time/size window); cross-tab leakage via RSS (the process agent reports only the tab's own process).

**Tests.** Unit: ring/windowing, clock monotonicity and join of network/frame events, probe no-op when disabled (zero allocations in the hot path, measured). Integration: a page that animates produces frame events with plausible monotonic stage timings; a forced slow layout (the pinned ≥ 122-nesting hang is *not* used; a bounded slow-layout fixture is) shows a long layout slice; idle-repaint indicator reports requested frames for a settled page (documenting, not fixing, the Round 11 issue); heap snapshot refused without the grant; profile start/stop leaves V8 unchanged afterwards; closed-cost benchmark check.

**Acceptance.** A timeline of frames, layout, script tasks and network for a recorded interval with honest units, a CPU profile (flame chart data) and heap snapshot when the debugger infrastructure is attached, memory/heap graphs, paint/layout overlays; zero measurable cost when closed.

**Complexity.** L–XL.

**Non-goals.** GPU profiling (no GPU path exists), per-function memory allocation tracking, tracing of the browser UI process, Lighthouse-style audits, automatic regression detection (that is the CI benchmark gate in the Round 12 roadmap).

## D7: Browser UI integration, docking, shortcuts, context menus, persistence, security validation

**Scope.** Assemble the panels (Elements, Console, Sources, Network, Application, Performance) in one native DevTools surface on the existing dock shell (`Left`, `Right`, `Bottom`, `Window`, forced Bottom on compact windows) and detached window; per-tab sessions with clear lifecycle indicators (grants active, revealing sensitive data, paused, recording); shortcuts F12 and Cmd/Ctrl+Shift+I/E/K mapped to Elements/Network/Console (the keys are already recognised and currently only open the panel) and Escape behaviour; "Inspect element" and "Inspect with DevTools" in the page and link context menus; picker mode; persistence of dock, size, last panel, filters and preserve-log in the profile (settings only, never captured data, never tokens); accessibility of the DevTools UI itself (AccessKit nodes, focus order, keyboard-only use); error and crash states (target closed, process died, protocol mismatch); the **security validation pass** over the whole programme.

**Reuse.** `devtools_ui.rs` dock shell, `tool_window.rs`, `Profile.devtools_dock`, `Command::Dock`, the container registry (sessions never cross containers), the notification/permission UI components for grant prompts, the old harness's dock/detached-window assertions (page width ≥ 180 with a side dock, bottom dock leaves the page above, detached window frames, closing the window leaves the app alive, split-console clipping) as acceptance criteria.

**New interfaces.** `devtools::Shell` (panel registry, per-tab state), context-menu command ids, profile fields (versioned, with migration from the single `devtools_dock` field).

**Depends on.** D1–D6 (panels land incrementally; D7 integration work for each panel can ship with its round, D7 closes the programme).

**Security risks.** UI-level confusion attacks (a page spoofing the DevTools surface: the panel is native UI outside page content and visually distinct); social engineering to paste code into the console (a one-time warning on first evaluate grant, as mainstream browsers do); persistence leaking data (assert only settings persist); keyboard shortcuts firing while the page has focus (they are browser-level and already handled before page dispatch); detached-window lifetime vs process lifetime; enterprise/policy off switch (a policy flag disables DevTools entirely: required for the validation checklist).

**Tests.** UI contract tests (egui model/snapshot tests like the existing `components_tests.rs`), profile round trip and migration, shortcut routing tests, context-menu action tests, harness-style end-to-end tests on a real engine page for each panel (reusing the engine harness), accessibility tests of the panel tree, and the **security validation suite**: grant lifecycle (every grant cleared on navigation/tab close), cross-container isolation, redaction defaults across HAR/console/exports, flood and slow-consumer tests for every stream, stale-id matrix across all domains, remote debugging absent (no listener opened by any DevTools action: a test enumerates the process's listening sockets), WebDriver unchanged, `unsafe` and dependency diff review, fuzzing of the inspector parser run for a fixed budget, and the policy kill-switch.

**Acceptance.** All panels reachable from the UI and shortcuts; settings persist, data does not; the validation suite passes in broker mode on macOS and (when hosts are available) Linux and Windows, with unrun platforms recorded as not tested; a written threat-model review is checked in; the `--remote-devtools` option stays rejected and no new listener exists.

**Complexity.** L.

**Non-goals.** Remote debugging transport, a CDP bridge, extension panels, theming beyond the existing palette, mobile/device emulation, DevTools for the DevTools, cloud sync of DevTools settings.

## Dependency graph and ordering

```
cross-cutting prerequisites ─▶ D1 ─┬─▶ D2 ─▶ D5 ─▶ D6 (profiler/heap)
                                   ├─▶ D3  (wega_dom::inspect contract can start with D1)
                                   └─▶ D4 ───────▶ D6 (network milestones)
                                  D1..D6 ─▶ D7
```
D2, D3 and D4 are independent after D1 and can proceed in parallel. D5 carries the main technical risk (inspector prototype) and should start its prototype early, in parallel with D2, even though its productisation follows. Suggested order if serial: D1, D2, D4, D3, D5, D6, D7 (Console and Network deliver the most value per effort; the CSS contract in D3 is the largest single item).

## Risks that cut across the plan

| Risk | Mitigation |
|---|---|
| The engine has gaps DevTools will expose (no CSSOM, no iframes, no shadow rendering, layout hang above 121 nested blocks, idle repaint) | DevTools must label unsupported areas, not hide them; those fixes are tracked in the Round 12 roadmap and are **not** D1–D7 scope |
| V8 inspector lazy attach fails the prototype | fall back to creating the runtime with the inspector enabled under a per-tab policy flag (reload on first Sources use) |
| Protocol drift between UI and engine | `INSPECTOR_VERSION` negotiation, golden-file tests, capability gating, `Unsupported` for unknown methods |
| Observers cost performance | zero-cost-when-closed rule gated by the benchmark runner in every round |
| Scope creep toward Chrome parity | each round's non-goals list; new domains need a design note |
| Sensitive-data mistakes | redaction at the source, grant model, default-off bodies, a dedicated security validation in D7 and per-round negative tests |


## D3.0 status

D3.0 prerequisites are complete locally; see `devtools-d3-0-report.md` for results, risks and the recommended D3-A/B/C boundaries.
