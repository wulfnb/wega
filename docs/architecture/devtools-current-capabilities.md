# What a Wega-owned DevTools can observe today

Audit round D0, current HEAD (`d8784df` plus the D0 documents). Nothing was run or changed; this is a source review with the design-critical claims re-verified (✔). Paths are repository-relative; `E` = `engines/wega`.

Question answered: *how can DevTools observe each Wega system without bypassing its boundary, what already exists, and what needs new instrumentation?*

## 1. Systems and what each exposes

### Process and transport

| Fact | Evidence |
|---|---|
| One sandboxed `wega-engine` process per page; it cannot open the network (descriptor ceiling, sandbox), so the only ways out are the protocol pipe, the inherited resource-broker channels and the shared frame store. | `src/wega_page.rs:575-700`; `engines/platform/src/sandbox*.rs` |
| Protocol pipe: 4-byte little-endian length + `serde_json`; `MAX_MESSAGE` 1 MiB, `MAX_RESULT_BYTES` 128 KiB, oversize rejected before allocation. Frame pixels never use the pipe (shared `FrameStore`). | `engines/protocol/src/lib.rs:15-17, 655-686` |
| `VERSION = 21` ✔, accepted range 4..=21; features gated by a `Capability` enum ✔; `content_event()` downgrades unknown newer events to `Notice` for older workers. The browser requires `Event::Ready{version}` first. | `lib.rs:14, 212-231, 300-331, 104-135`; `src/wega_page.rs:806-823` |
| Identity: every packet is wrapped in `SessionIdentity{tab, frame, process (16 random bytes), service}`; mismatches and nested Session/Init rejected. There is no secret: the pipe is the child's stdio and the identifiers are routing/authority keys, not bearer secrets. | `engines/protocol/src/identity.rs:1-90` |
| Only the browser sends `Command`s; the content process can only emit `Event`/`ContentEvent` variants, which the browser validates against the current document context and drops if stale. | `src/wega_page.rs:125, 1115-1121`; `engines/protocol/src/authority.rs` |
| Documents: `DocumentId` (navigation generation), `DocumentRevision` (DOM/global replacement counter), `DocumentContext{document, revision}`; scoped commands carry `RequestContext{document, revision, request}`. The engine re-checks both and answers `RequestFailed{StaleDocument}`; the host holds ≤ 32 pending requests with deadlines, cancels them on navigation, and ignores late replies. | `lib.rs:25-38, 204-209`; `requests.rs:84-88`; `E/src/main.rs:884-906` |
| Frames: `FrameId` exists, the top-level view is hard-coded `FrameId(1)`, the browser keeps a frame tree from `FrameAttached/Detached/Navigated`, but **child frames are not separate evaluation targets** (`FrameNavigate`/`FramePostMessage` only produce a notice in the engine) and iframes do not render. A `ContextKey` type does not exist (✔ the nearest concept is `DocumentContext`). | `lib.rs:622-646`; `src/wega_page.rs:1123-1190`; `E/src/main.rs:1046-1075` |
| Listeners: the only product listener is the WebDriver server (`--webdriver-port`, `127.0.0.1`, Host-header check, **no authentication**; `--help` says so ✔). There is no remote-debugging or inspector port; `--remote-devtools` is rejected as an unknown option at HEAD. | `src/automation.rs:80-97`; `src/main.rs:219`; `src/options_tests.rs:79-90` |
| Release builds refuse an unsandboxed worker; a debug build may opt in with `WEGA_UNSAFE_DEVELOPMENT_WORKER=1`. | `engines/platform/src/launch.rs:25-37` |

### Scripting (V8 via the Deno core)

| Fact | Evidence |
|---|---|
| `evaluate_json` runs `runtime.execute_script` in the page's **main world** under a watchdog thread that terminates execution at the deadline; a fatal result tears the realm down. Results are JSON (`v8::json::stringify`), `undefined` → `"null"`, over 128 KiB fails. Errors are `ScriptError{fatal, message}`: plain text, **no stack, line or column**. It is not subject to the page's CSP `unsafe-eval` (CSP governs page code only). | `engines/adapters/v8/src/host.rs:122-160, 296-313, 334, 462-505`; `script.rs:5-9`; `E/src/view.rs:670` |
| **V8's inspector is compiled in and unused** ✔: `mod inspector;` is unconditional (no feature), `JsRuntimeInspector`/`LocalInspectorSession` are exported, `RuntimeOptions.inspector: bool` defaults to false and Wega's `create_runtime` leaves it unset, and `JsRuntime::maybe_init_inspector()` exists to attach one to a **running** runtime. `create_local_session`, `dispatch`/`post_message`, `poll_sessions_from_event_loop` and `capture_network_event` are public. So an in-process CDP-style session (`Runtime.*`, `Debugger.*`, `Profiler.*`, `HeapProfiler.*`) is reachable without vendor patches. **Not tried;** whether a lazily attached inspector sees scripts parsed before it attached, and how a paused debugger interacts with the watchdog, are untested (D5 prototype items). | `engines/vendor/deno/deno_core/lib.rs:26,113-122`; `runtime/jsruntime.rs:641, 2223-2245`; `inspector.rs:1255, 1387`; `engines/adapters/v8/src/runtime.rs:63-80` |
| Console: `console.*` goes through deno_web's formatter to host op `"log"`; **only warning and error levels** become a structured diagnostic (to stderr); every message is also `eprintln!`ed truncated to 4 KiB. Log/info/debug do not reach the browser structured. No timestamps, argument previews, stacks or call sites. | `E/src/dom/web.js:746-748`; `E/src/dom.rs:1604-1618` |
| Uncaught errors: host op `"error"` → stderr diagnostic + a bounded list (16) → `view.notices` (ring of 16) → `Event::Diagnostic` (Info) and `ContentEvent::Notice`; messages containing `; missing:` also set `compatibility`. | `E/src/dom.rs:970-983`; `view.rs:784-797`; `main.rs:595-624` |
| Workers (dedicated, shared, service) are separate realms with no inspection channel. | `E/src/workers/` |

### Logging and diagnostics

| Fact | Evidence |
|---|---|
| Structured `Diagnostic{category Javascript/Css/Html/Browser/Network, severity warning/error, message ≤ 16 KiB, url ≤ 8 KiB, line, column}` printed as `WEGA_DIAGNOSTIC\t<json>` on stderr; Blitz emits the CSS/HTML parser ones. | `src/diagnostic_event.rs:3-45`; `engines/vendor/blitz/blitz-dom/src/error_diagnostics.rs:25` |
| The browser records them to a rotating file (5 MiB × 3) and a signed-report outbox; **no in-memory per-document store** exists for a UI to read (only the 16-entry `Snapshot.notices`, which no panel consumes). | `src/diagnostics.rs:8-9, 113-195`; `src/wega_page.rs:847-860, 1006-1021` |

### Network

| Fact | Evidence |
|---|---|
| The broker lives in the **browser process**; the engine reaches it through inherited channels (or a loopback socket with a 64-hex token). The engine's requests carry no secret origin: the broker rewrites and checks origin/initiator, requires a navigation receipt for top-level documents, and forces `redirect = manual` for bound sessions. | `engines/network/src/lib.rs:1289-1330, 1440-1560`; `src/wega_page.rs:638-650` |
| **Single choke point for HTTP:** `Broker::accept`'s `Fetch` arm → `fetch_cached` (HSTS upgrade, ≤ 8 redirect hops with a policy check per hop, cache, cookie jar, Set-Cookie ingestion). It sees the sanitised request, the final `Response{url,status,headers,redirected,body}`, `ResourceDestination` (Document/Script/Style/Image/Font/Fetch/Xhr/WebSocket/Module/…), and is the only place real `Cookie`/`Set-Cookie` values exist. WebSocket open/IO pass through the same broker (`ResourceRequest::WebSocketOpen/Io`). | `lib.rs:487-805, 260-360`; `services.rs:367-373` |
| **No request ids, timings, per-request log or initiator information exist today** (no per-request `Instant`; only timeouts). Bodies are not retained; limits 8 MiB body / 64 KiB headers. Response headers (including `set-cookie`) are built before cookie ingestion, so they may reach the engine unfiltered (inference; the later filtering code was not read). | `lib.rs:29-30, 648-655` |
| Engine side: `Network::fetch_http` with an `interceptor` hook that can short-circuit; it cannot see browser-initiated navigations. Downloads stream in the browser through `src/downloads.rs` with progress already tracked. | `E/src/network.rs:195-201, 456, 528`; `src/downloads.rs` |
| Cookie inventory (`CookieEntry{name,value,domain,path,secure,http_only,same_site}`) is available browser-side. | `engines/services/cookies`; `src/wega_page.rs:565-570` |

### DOM, style, layout, paint (Wega contracts, Rounds 6–11)

| Capability | Contract | Engine host op |
|---|---|---|
| DOM tree reads | `DocumentView`/`NodeView` (kind, name, attributes, parent, children, `element_by_id`, `base_url`) | `root`, `children`, `childAt`, `parent`, `attributes`, `info`, `text`, `outerHTML`, `innerHTML`, `query*`, `matches`, `closest` (`E/src/dom.rs:1199-1350`) |
| DOM edits | `DocumentMutation`/`Mutator` | `setAttr`, `setText`, `remove`, … (`dom.rs:1387-1510`) |
| Computed style (one property) | `StyleAccess::resolved_value` | `computed` (`dom.rs:1304`) |
| Inline style read/write | `StyleAccess`/`StyleControl` | `styleSet`, `inlineCSS` |
| Geometry, hit test | `GeometryAccess`, `HitTest::element_at` | `rect`, `box`, `clientBox`, `offsetBox`, `clientRects`, `scrollSize` |
| Paint | `PaintSource` (record/prepare/animating) | via `View::paint` |
| Focus/hover | `FocusState`/`InputControl` | |
| Accessibility tree | Blitz-built AccessKit `TreeUpdate` | `EnableAccessibility` → `ContentEvent::Accessibility(json)` |
| Shadow/custom-element metadata | JS-side + host ops (`shadowRoot`, `shadowHost`, `assignedNodes`, `customDefinition`) | |

These are reachable today **only through the JavaScript bindings** (`op_dom_read`); there is no protocol path. A native backend running on the DOM thread can call the traits directly on the same `Rc<RefCell<BlitzDocument>>`, so no second DOM tree is needed. Node ids are `NodeKey` u64s, stable within a document and not across documents.

### Existing UI pieces (HEAD)

| Piece | State |
|---|---|
| `src/devtools_ui.rs` (104 lines) | The dock shell and a status panel: URL, title, PID, worker RSS, JS heap, DOM nodes, lifecycle phase, compatibility. Reads `page.state` under a mutex. Ends: "Worker inspection is available through WebDriver. The embedded-backend inspector and debugger have been removed." Docks `Left/Right/Bottom/Window` persisted in `Profile.devtools_dock`; forced to Bottom when the window is compact. |
| `src/tool_window.rs` (198 lines) | Detached OS window for `Dock::Window`; F12/Cmd-W close it. |
| Shortcuts | **F12** toggles (`src/browser.rs:1197`); **Cmd/Ctrl + Shift or Alt + I / E / K** are consumed and only open the panel (`browser.rs:1130-1135` ✔: `show_console = true` and a redraw; the per-tool tab selection of the old toolbox is gone); Escape closes. (The delegated review of this document's inputs claimed only F12 exists; the source says otherwise.) |
| `src/toolbox.rs` (13 lines) | `Toolbox{panel_rect, error}`. |
| `src/inspector.js` (44 lines), `src/console_eval.js` (28 lines) | Engine-neutral page scripts left over from the removed toolbox: nothing in `src/` loads them (✔); `tests/probe_scripts_tests.mjs` runs them under Node. `inspector.js` needs `document.styleSheets`/`getComputedStyle` in the page (the engine has no CSSOM, so it cannot work as is); `console_eval.js` previews objects four levels deep and supports `await`. |
| `wega://` internal pages | rendered natively (egui), not by the engine, so a DevTools page would be native UI. |

### Authority and security facts

- The page cannot originate commands or choose its origin/partition (`FrameAuthority::bind` after `DocumentCommitted` and the broker's navigation receipt; "Renderer values never select an origin or partition").
- **`evaluate` has no capability flag**: any `EvaluateDocument` with valid identity and context runs with full page-world authority; callers today are WebDriver `ExecuteScript`/`GetPageSource` (unauthenticated), the harness and legacy `Page::evaluate`. The engine rejects the unscoped `Evaluate`/`CheckPageState` in session mode (✔ `E/src/main.rs:126-133`).
- Activation (`ActivationId`) can be minted only by the trusted input adapter, not by evaluation or synthetic events.
- Opaque origins cannot receive permissions or be storage keys.

## 2. Capability table

| Inspector capability | Exists today | Needs new instrumentation |
|---|---|---|
| Page info, URL/title, nodes, heap, RSS | `Event::State` (changes only) → the status panel | per-frame breakdown |
| Evaluate / REPL | `EvaluateDocument` (scoped, stale-checked, 5 s watchdog, 128 KiB) | rich result model (value previews, exception with stack/line/column), a devtools evaluation gate, async/await (`console_eval.js`), optional V8 `Runtime.evaluate` |
| Debugger | V8 inspector compiled in, not started | lazy `maybe_init_inspector`, pump sessions from the event loop, CDP-JSON tunnel (new command/event + capability), watchdog suspension while paused |
| Console messages | warning/error only, stderr diagnostics | structured event for every level (level, argument previews, timestamp, stack, document context) intercepted at `web.js:746`; bounded ring |
| Uncaught errors, promise rejections | notices (16, 4 KiB), no stack | line/column/stack from V8 message data; source URL |
| Parser/CSS warnings | file + outbox only | in-memory per-document store the backend can read |
| Network list, timings, headers | none | broker-side observer (request id, start/end, redirect hops, sizes, cache hit, CORS result), redaction, a browser-process event stream |
| Request/response bodies | not retained | opt-in bounded capture in the broker with privacy gating |
| WebSocket frames | broker path exists | tap in the broker's WebSocket I/O |
| Cookies | inventory exists browser-side | edit/delete API, UI table, HttpOnly masking |
| Storage / Cache / IndexedDB | authoritative services over the broker | read-only enumeration commands |
| DOM tree | host ops + `DocumentView` | protocol commands for bounded snapshots/expansion keyed by `DocumentRevision`; mutation notifications (revision-based, coalesced) |
| DOM editing | `Mutator`, host ops | command surface, undo, authority |
| Computed style | one property per call | bulk enumeration |
| **Matched CSS rules, stylesheets, cascade origin** | **none** (no CSSOM; Stylo used inside the Blitz adapter only) | a new read contract in `wega-dom` (stylesheet and rule listing, matched rules per element, specificity, source ranges) implemented in `wega-adapter-blitz` over Stylo. **The largest gap.** |
| Inline style editing | `StyleControl::set_inline_property` | rule-level editing absent |
| Box model / geometry | `rect`, `box`, `clientBox`, `offsetBox`, `clientRects` | margin and border boxes, per-fragment boxes, an overlay |
| Hit test / picker | `HitTest::element_at` | `InspectAt` command and an overlay path |
| Screenshot / paint | `CaptureDocument` returns a hash only; WebDriver screenshots the UI's last frame | pixel capture through the shared frame store; paint diagnostics do not exist |
| Accessibility | enable/action/tree events | a tree viewer |
| Event listeners | none host-side (JS-internal in `semantics.js`) | export through a host op |
| Frames tree | frame events and browser frame tree | per-frame targets (child frames are not separate realms yet) |
| Workers | separate realms | a target type and an inspection channel |
| Performance / memory | `State.js_heap`, `rss_bytes`, slow-script notices; render stage timings exist only in benchmarks | timeline events, V8 profiler domains if the inspector is enabled, heap snapshots |
| Remote transport | none (WebDriver is unauthenticated and minimal) | not part of the in-process design; if wanted, a separate token-authenticated, Origin-checked loopback endpoint |
| Trust gating | none for `evaluate`; page cannot originate commands | browser-side `DevtoolsAuthority` per tab/container and a `Capability::Inspector` carried in `RequestContext` |

## 3. Reusable without Servo

Present and reusable: the dock/window shell, the profile dock persistence, the F12 and shortcut plumbing; the request correlation, stale-document and size/queue-bounding machinery in `wega-protocol`; the structured `Diagnostic` type and the rotating log; the DOM/style/layout/hit-test/paint contracts; the broker choke point; the cookie inventory; the V8 inspector inside the vendored Deno core; engine-neutral page scripts (`console_eval.js`, `inspector.js`, with limits); the old egui panels' *design* (ten tabs, 500-row/64 KiB bounds, HAR 1.2 conversion logic) as reference in git history; the old tests' *assertions* as acceptance criteria. Not reusable: anything importing `servo`, `devtools_traits`, `servo_base` (the old `toolbox.rs`, `debug_driver.rs`, `network.rs` bridge): they would have to be rewritten against Wega's protocol.
