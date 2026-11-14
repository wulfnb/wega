# Wega-owned Developer Tools: design

Status: **design only** (round D0). Nothing in this document is implemented, no runtime behaviour changed. It builds on the evidence in [`devtools-pr58-deletion-audit.md`](./devtools-pr58-deletion-audit.md) and [`devtools-current-capabilities.md`](./devtools-current-capabilities.md); the implementation plan is [`devtools-implementation-roadmap.md`](./devtools-implementation-roadmap.md).

## 1. Goals, constraints, non-goals

**Goal.** Give the Wega browser developer tools whose data comes from Wega's own contracts, not from an embedded engine's private protocol, so they keep working when engines change underneath (Blitz, V8, Skia).

**Constraints (from the brief and the repository).**
- Wega owns the protocol and the UI. No dependency on Servo's DevTools (`devtools_traits`, `servo_base` channels, Firefox protocol).
- No Blitz type in any public inspector message. Blitz may implement DOM inspection *behind the existing contracts*.
- V8/Deno debugging mechanisms may be used *behind a Wega adapter*.
- No second authoritative DOM tree. No duplicate network stack. No unsafe page-context evaluation without explicit authorisation.
- Inspection respects browsing-context and origin isolation and goes through the security broker, never around it.
- Remote debugging is off by default.
- Every event stream has bounded queues and backpressure.
- Sensitive headers, cookies and bodies need deliberate access controls.
- Identifiers survive navigation: stale documents and stale nodes fail safely.
- No new `unsafe` in Wega code; no dependency cycles; no placeholder crates.

**Non-goals (whole programme).** Reviving Servo or its protocol; Chrome DevTools Protocol compatibility (a CDP bridge is possible later but is not designed here); remote debugging by default; Firefox/Chrome front-ends; extension DevTools panels; source maps and mobile emulation; DevTools for workers and frames beyond what the engine itself supports (child frames are not separate realms today).

## 2. Architecture

```
 browser process (root crate `wega`)                          content process (sandboxed `wega-engine`)
┌──────────────────────────────────────────────┐            ┌─────────────────────────────────────────────┐
│ Wega DevTools UI  (egui; src/devtools/…)     │            │ Wega Inspector Backend (engine, content side)│
│   panels, docking, picker, persistence       │            │   agents:  Runtime/Console · DOM · CSS ·     │
│        │  typed Rust API (InspectorClient)   │            │            Page/Overlay · Debugger/Profiler  │
│ Wega Inspector Client / session manager      │  Command:: │        │ adapters (no Blitz/V8 types leave)  │
│   authority, grants, credits, caches         │──Inspector─┤        ├─ DOM/CSS  → wega-dom contracts      │
│        │                                     │  Event::   │        │             (+ new `inspect` module) │
│ Browser-side backend                         │◀─Inspector─┤        ├─ Runtime  → wega-adapter-v8         │
│   Network agent  ← observer in wega-network  │ (existing  │        │             inspector channel       │
│   Storage/Cookies agent ← wega-cookies,      │  pipe,     │        └─ Render   → PaintSource /           │
│                           wega-storage       │  versioned,│                      GeometryAccess / HitTest│
│   Process/Memory agent ← wega-platform       │  bounded)  └─────────────────────────────────────────────┘
└──────────────────────────────────────────────┘
          shared by both sides:  `wega_protocol::inspector`  (Wega Inspector Protocol: messages, ids, limits)
```

**Wega DevTools UI** is egui code in the browser process and never sees engine types: only `wega_protocol::inspector` messages. **Wega Inspector Protocol** is a Wega-owned, versioned message set (§4). **Wega Inspector Backend** is split by *where the data truly lives*, not by convenience:

- *Content side* (DOM, style, layout, JavaScript, console, rendering): runs on the page's DOM thread inside `wega-engine`, calling the existing `wega-dom` contracts and the V8 adapter directly. It never copies the DOM: it answers bounded queries and sends bounded snapshots, so there is no second tree.
- *Browser side* (network, cookies, storage, process memory): runs in the browser process where the broker, cookie jar, storage services and process supervisor already are. The Network agent is an **observer** on the one HTTP choke point (`Broker::accept` → `fetch_cached`), not a second network stack; it never goes through the content process, so a compromised renderer cannot forge or suppress network records.

### Why not a standalone server process or a CDP port

- The content process cannot open sockets (sandbox), so a devtools server inside it is impossible without weakening the sandbox; everything content-side must travel over the existing protocol pipe.
- A listening port is the attack surface WebDriver already exposes unauthenticated. The in-process UI needs none. Remote debugging, if ever added, is a separate transport (§6.7) on the same backend, off by default.

## 3. Crate and module decisions

No new crate is justified for D1–D7. The test applied: a crate must give an independent consumer set, dependency isolation, or an enforced boundary that a module cannot.

| Candidate | Decision | Reasoning (evidence) |
|---|---|---|
| `wega-inspector-protocol` | **No crate; module `wega_protocol::inspector`** | Consumers are the root crate, the engine and `wega-network`, and all three already depend on `wega-protocol` (root `Cargo.toml:34`; the `wega` package depends on exactly `wega-network`, `wega-platform`, `wega-protocol` per `cargo metadata`). The protocol already provides framing, the 1 MiB cap, versioning/capabilities, `SessionIdentity`, `DocumentContext`/`RequestContext`, request correlation and stale rejection that the inspector reuses. A separate crate would duplicate that machinery or depend on `wega-protocol` anyway, and it would add a manifest and an edge without isolating anything. **Split trigger:** a second transport (remote debugging, a CDP bridge) or an external consumer that must not link the engine protocol. A boundary test (as for `wega-dom`) will assert the module names no `wega_dom`, `blitz`, `v8` or `deno` type. |
| `wega-inspector` (backend) | **No crate; module `inspector/` in `wega-engine` (content side) and `observe` in `wega-network` (browser side)** | The content-side agents need engine-private state (`Dom::call` host ops, the `ScriptHost` trait, the V8 host, the per-document revision) and run on the DOM thread; a crate would need those exposed first (a bigger change than the feature). Network observation must live where the broker is (`wega-network`). The contracts the backends consume are already in `wega-dom`. **Split trigger:** a second engine implementation (a backend that is engine-independent can then move out), or a boundary the checker cannot otherwise enforce. Until then a test forbids `blitz_` references under `engines/wega/src/inspector/` so the backend adds no new Blitz coupling. |
| `wega-devtools-ui` | **No crate; module `devtools/` in the root crate** | The root crate is one egui application; `devtools_ui.rs`, `tool_window.rs` and `toolbox.rs` already live there and share its theme, docking, profile and window code. Splitting the UI out would cut it from those internals for no isolation benefit; the UI/engine boundary is already the protocol (the root crate does not depend on `wega-dom` or `wega-engine`). **Split trigger:** the root crate itself being decomposed. |
| New contract module `wega_dom::inspect` | **Yes (a module, not a crate)** | The one real contract gap: matched CSS rules/stylesheets (§5.3). It belongs beside `style.rs` and is implemented in `wega-adapter-blitz`. |
| `wega-adapter-v8` inspector channel | **Yes (code in the existing adapter)** | A safe wrapper over deno_core's `JsRuntimeInspector`/`LocalInspectorSession` that exposes only JSON strings and a pump function; no V8 type crosses it. |

Dependency impact: **zero new crates and zero new edges in D1–D7** (the checker's edge count stays at 24 for the engine workspace; the root crate's three engine-workspace dependencies are unchanged). Cycle risk: the only new references are modules inside crates that already depend in the needed direction (root→protocol, network→protocol, engine→protocol, engine→dom, adapters→dom).

## 4. Wega Inspector Protocol

### 4.1 Envelope and transport

- JSON values inside the existing protocol: `Command::Inspector(InspectorRequest)` and `Event::Inspector(InspectorEvent)` (content side), gated by a new `Capability::Inspector` and a `VERSION` bump with a downgrade arm in `content_event()`; the browser-side backend uses the same message types in-process (a channel), so the UI has one client API regardless of where an agent runs.
- Request: `{ id, method: "Domain.method", params, context }` where `context` is the existing `RequestContext{document, revision, request}` for content-side calls, so correlation, deadlines, cancellation, the 32-pending cap and stale-document rejection are inherited, not reimplemented.
- Response: `{ id, result }` or `{ id, error: {code, message} }`; events: `{ event: "Domain.event", params, context, seq }`.
- Error codes (extending `ProtocolError`): `InspectorDisabled`, `NotAuthorised`, `StaleDocument`, `StaleNode`, `StaleObject`, `ResultTooLarge` (with a continuation cursor), `Busy`, `Unsupported`.
- Versioning: the inspector message set has its own `INSPECTOR_VERSION` negotiated inside the capability, so UI and backend can evolve independently of the engine protocol version. Unknown methods return `Unsupported` (never panic); unknown events are ignored by the client.

### 4.2 Domains (by implementation round)

| Domain | Where it runs | Methods (initial) | Events | Round |
|---|---|---|---|---|
| `Target` | browser | `list`, `attach`, `detach`, `setAuthorisation` | `attached`, `detached`, `documentChanged{old,new}` | D1 |
| `Runtime` | content (V8 adapter) | `evaluate{expression, awaitPromise, throwOnSideEffect, returnByValue}`, `getProperties`, `releaseObject`, `callFunctionOn` | `executionContextCreated/Destroyed`, `exceptionThrown` | D2 |
| `Console` | content | `enable`, `clear`, `getHistory{cursor}` | `messageAdded{level, args[], timestamp, stack, source}` | D2 |
| `DOM` | content (`wega-dom`) | `getDocument{depth}`, `requestChildNodes`, `querySelector(All)`, `describeNode`, `setAttribute`, `removeAttribute`, `setNodeValue`, `removeNode`, `highlight`, `inspectAt{x,y}`, `getBoxModel` | `documentUpdated`, `childNodeCountUpdated`, `attributeModified` (coalesced) | D3 |
| `CSS` | content (`wega_dom::inspect`) | `getComputedStyle` (bulk), `getMatchedStyles`, `getInlineStyles`, `setInlineProperty`, `setRuleText`, `listStyleSheets` | `styleSheetChanged` | D3 |
| `Network` | browser (`wega-network::observe`) | `enable{captureBodies, revealSensitive}`, `getResponseBody`, `clear` | `requestWillBeSent`, `responseReceived`, `dataReceived`, `loadingFinished/Failed`, `webSocket*` | D4 |
| `Storage` | browser (cookies, storage services) | `getCookies`, `deleteCookie`, `getStorage{kind}` | `changed` | D4/D7 |
| `Debugger` | content (V8 adapter) | `enable`, `setBreakpoint`, `removeBreakpoint`, `pause`, `resume`, `stepOver/Into/Out`, `evaluateOnCallFrame`, `getScriptSource` | `scriptParsed`, `paused`, `resumed` | D5 |
| `Profiler`/`Performance`/`Memory` | content + browser | `start/stop`, `getMetrics`, `takeHeapSnapshot` | `metrics`, timeline events | D6 |
| `Page`/`Overlay` | content + UI | `captureScreenshot`, `setShowPaintRects`, `setShowLayoutBorders` | `frameNavigated`, `lifecycle` | D3/D6 |

The table is the target surface, not a commitment to every method.

### 4.3 Identifiers and navigation safety

| Id | Definition | Lifetime and staleness |
|---|---|---|
| `TargetId` | `(tab, frame, process)` from `SessionIdentity` (top-level `FrameId(1)` today; one target per tab until child frames become realms) | invalid after the tab closes or the process dies |
| Document stamp | the existing `DocumentContext{document, revision}` | every content-side request and event carries it; the engine rejects a mismatch with `StaleDocument` (existing behaviour) |
| `InspectorNodeId` | `{revision, NodeKey}` (a u64 key plus the document revision at issue) | valid only while `revision` is current; any request using an older revision fails with `StaleNode`, never resolves to another node (Blitz node ids carry a slot version, `wega-dom` guarantees a dropped key never resolves to another node) |
| `RemoteObjectId` | `{revision, handle}` from the V8 adapter's object group | released on navigation or by `Runtime.releaseObject`; unreleased handles are bounded per session |
| Network `RequestId` | monotonically increasing per session, assigned by the broker observer | independent of documents; carries the issuing `DocumentId` so the UI can group by navigation |
| Breakpoint ids | per script `(DocumentId, scriptId, offset)` | cleared on navigation (re-set explicitly by the UI) |

On navigation the backend emits `Target.documentChanged`, drops all content-side object and node handles, and the UI discards its caches keyed by the old stamp. The UI never sends a request without a stamp.

## 5. Adapters (how each domain observes without bypassing a boundary)

### 5.1 DOM (`wega-dom` + `wega-adapter-blitz`)

The DOM agent calls `DocumentView`/`NodeView`/`DocumentMutation` and the geometry/hit-test contracts on the DOM thread. It keeps **no copy**: `getDocument` and `requestChildNodes` serialise bounded subtrees on demand (node count and byte budgets, continuation by parent id), and the UI holds a view cache invalidated by the stamp. Change notification uses the engine's existing revision counter (bumped on every mutating host op) to emit coalesced `documentUpdated`/`childNodeCountUpdated` events; fine-grained attribute events are added only if the revision signal proves too coarse (D3 decision). Edits go through the same `Mutator`/`StyleControl` paths script uses, so the usual budgets (DOM node, text bytes) apply. Shadow trees are inspectable only as far as the engine models them (detached fragments today); the UI marks them accordingly.

### 5.2 Runtime and debugger (`wega-adapter-v8`)

- **Console/Runtime (D2) do not require the V8 inspector.** Console capture is done by intercepting `console.*` at the existing `web.js` boundary (all levels, structured arguments with bounded previews, timestamp, stack from V8 message data) and by upgrading `ScriptError` to carry exception/stack/line/column. `Runtime.evaluate` reuses `execute_script` under the existing watchdog and result caps, with the new authorisation gate (§6.2).
- **Debugger/Profiler (D5) use V8's inspector**, already compiled into the vendored Deno core and unused: lazily attach with `JsRuntime::maybe_init_inspector()` when the first session enables `Debugger`, create a `LocalInspectorSession`, pump it from the engine's event loop (`poll_sessions_from_event_loop`), and tunnel its JSON through `Inspector` messages. The wrapper in `wega-adapter-v8` exposes only strings. **Prototype risks to resolve first (unverified):** whether scripts parsed before attachment are reported on `Debugger.enable`; how a paused debugger interacts with the watchdog thread (`terminate_execution` at the deadline must be suspended while paused and the `ScriptStopped` path must not fire); whether the inspector's overhead warrants attaching only on demand; and that the realm teardown on a fatal result also tears down the session. No Wega `unsafe` is needed: the `unsafe` is inside the vendored crate.

### 5.3 CSS (`wega_dom::inspect`, new contract)

Computed styles are readable today one property at a time; matched rules, stylesheet/rule listing, cascade origin, specificity and source ranges do not exist because the engine exposes no CSSOM (Stylo is used inside the Blitz adapter). The contract:

```
trait StyleInspect: StyleAccess {          // read side; names no Stylo or Blitz type
    fn computed_all(&self, key, out: &mut Vec<(Cow<str>, Cow<str>)>) -> Result<…>;
    fn style_sheets(&self, out: &mut Vec<SheetInfo>);                    // id, origin, owner node, disabled, media text
    fn rules(&self, sheet, cursor, out: &mut Vec<RuleInfo>) -> …;        // selector text, declarations, source range
    fn matched_rules(&self, key, out: &mut Vec<MatchedRule>) -> …;       // rule ref, specificity, origin, order
}
```

implemented in `wega-adapter-blitz` over the Stylist's `Stylesheet`s and selector-matching results. Rule *editing* is a separate, later contract (`StyleEdit`) and follows the same all-or-nothing, no-cache rules as `StyleControl`. This is the largest single piece of new engine work in the programme and is deliberately isolated in D3.

### 5.4 Network (`wega-network::observe`)

A per-session `NetworkObserver` trait called from `Broker::accept`'s `Fetch` arm and `fetch_cached` (request start with the sanitised request and `ResourceDestination`, each redirect hop, cache result, CORS result, response head, body chunks sizes, finish/failure with `Instant`-based timings) and from the WebSocket open/IO path. It records into a **bounded ring per session** (row cap, byte cap, drop-oldest with a `dropped` counter) and emits events on the in-process channel to the Network agent. Redaction happens **at the tap**, before anything is stored or sent (§6.3). It is a pure observer: it cannot alter, delay or reject a request; observer failure never fails a fetch. The old design's 500-row and 64 KiB-body bounds and HAR 1.2 conversion are the reference.

### 5.5 Rendering

Layout and paint diagnostics reuse `GeometryAccess` (boxes, hit test), `PaintSource` and the existing frame timings (`record scene`, `rasterise`, frame submission: today only measured by benchmarks, to be sampled behind a flag). Overlays (highlight, picker) are drawn by the UI over the page surface from box-model data returned by `DOM.getBoxModel`, not by injecting DOM into the page, so inspection does not alter page layout or script-visible state. No change to the painter is planned.

## 6. Security design

### 6.1 Threat model and invariants

- A page can never reach the inspector: only the browser sends `Command`s; the content process can only emit events, and the backend validates every inspector request against the current `RequestContext`. The page cannot observe that DevTools is open except through unavoidable side effects (timing; a paused debugger), which are inherent.
- Inspector data flows only to the UI of the same tab and container. Sessions are bound to `(tab, container/profile, process)`; a session never receives data from another container's broker session (the broker is keyed per session already).
- Every capability below is **off until explicitly enabled by a browser-side user action** and is revoked on navigation unless the user setting says otherwise.

### 6.2 Authorisation and evaluation

- A browser-side `DevtoolsAuthority` (per tab, owned by the root crate, never by the content process) holds grants: `inspect_dom`, `evaluate`, `debug`, `network`, `reveal_sensitive`, `read_bodies`. Opening the panel grants `inspect_dom` and console *reading*; **`evaluate` and `debug` are granted only by an explicit user action in the Console/Debugger UI**, are logged in the session history, and are bound to the current `DocumentContext` (cleared by navigation). They are unavailable on `wega://` pages and on opaque-origin documents unless the user opts in.
- `Runtime.evaluate` runs in the page's main world (the only world the engine has) under the existing watchdog and result caps; `throwOnSideEffect` (via V8) is used for hover/preview evaluation. Because evaluation bypasses page CSP `unsafe-eval` today, the gate above is the control; the backend refuses evaluation without a grant (`NotAuthorised`).
- The unscoped legacy `Evaluate` stays rejected in session mode (existing); the inspector does not add a second evaluation path outside `RequestContext`.
- WebDriver remains a separate, unauthenticated surface and is **not** part of this design; the design recommends giving it the same token and Origin discipline before it is ever documented as a debugging channel (non-goal for D1–D7, tracked in the roadmap risks).

### 6.3 Sensitive data

| Data | Default | Deliberate access |
|---|---|---|
| `Cookie`, `Authorization`, `Proxy-Authorization`, `Set-Cookie` headers | recorded as `<redacted>` (name and length only), redaction at the broker tap | `reveal_sensitive` grant, user-initiated, per session, shown with a persistent indicator, never persisted |
| HttpOnly cookie values | name and attributes only | same grant; value never leaves the browser process unless granted |
| Query-string secrets | shown as sent (visible in the address bar anyway) but excluded from exported HAR unless revealed | HAR export asks explicitly |
| Request/response bodies | not captured | `read_bodies` grant per session; bounded (64 KiB default, hard cap 8 MiB = broker `MAX_BODY`); binary shown as size and type; cross-origin opaque (`no-cors`) responses never exposed |
| WebSocket frames | metadata only | `read_bodies` |
| Console arguments | bounded previews (depth and length caps), accessors not invoked | evaluation grant for expansion |
| Local/session storage, cache, IndexedDB values | listing and sizes | value read is per-key on request, same-origin session only |

Exports (HAR, saved logs) are explicit user actions, redacted by default, and written with the downloads module's existing path-safety rules.

### 6.4 Bounded queues and backpressure

- Per-session **credit window**: the backend may have at most N unacknowledged events (default 256 events or 1 MiB, whichever first) in flight; the UI acknowledges as it renders. When credit is exhausted, log-like events (`Console.messageAdded`, `Network.dataReceived`, `Debugger.scriptParsed` bursts) are **dropped oldest-first with a counted `dropped` marker**, while state-changing events (`documentUpdated`, `paused`, `loadingFinished`) are coalesced (latest wins), never silently lost. Nothing blocks the DOM thread or the broker: producers never wait on the UI.
- Request side: the existing 32-pending cap, deadlines and cancellation (`DocumentRequests`) apply; each method has a result cap (default well below the 1 MiB frame and 128 KiB result caps) and returns a continuation cursor when truncated.
- Retained history: console 1,000 entries (the removed model's cap), network 500 rows, both per session and cleared on tab close; bodies and snapshots are never retained beyond the cap. Memory of the observer ring counts against the tab's existing memory budget accounting.
- A session limit per tab (one UI client) prevents fan-out amplification.

### 6.5 Isolation and brokers

Network, cookie and storage data are read by browser-side agents from the services that already enforce partitioning (per-container jar, `StorageKey`, authoritative storage). The content process is never asked for them and cannot supply them. Content-side agents act only through the document's own contracts; they gain no new filesystem, network or process authority, and the sandbox is unchanged. Frames: only the main frame is a target until child frames are real realms (the audit shows they are not); the protocol carries `FrameId` from the start so adding frame targets is additive.

### 6.6 Navigation and teardown

On navigation the backend invalidates handles (§4.3), cancels pending inspector requests (the existing `navigate()`/`commit()` cancellation), clears the evaluation and debug grants, and resets paused state (a paused debugger is resumed before teardown). On process death the browser-side session ends and the UI shows "target closed"; no content-side state outlives the process.

### 6.7 Remote debugging

**Disabled by default and not implemented in D1–D7.** If added later it is a separate transport over the same protocol with: an explicit opt-in flag per run (not a persisted default), loopback only, a random per-run token required on every connection, an `Origin`/`Host` allow-list, one client per session, the same grants as the UI (a remote client starts with none), and the broker unchanged. It must not reuse the WebDriver listener. The removed `--remote-devtools PORT` option is not restored.

## 7. UI design (D7 scope, outline)

A single native panel with tabs (Elements, Console, Sources, Network, Application, Performance) built on the surviving dock shell (`Dock::{Left, Right, Bottom, Window}`, persisted in the profile) and the existing detached window; state per tab; keyboard: F12 toggles, Cmd/Ctrl+Shift+I opens Elements, Cmd/Ctrl+Shift+K Console, Cmd/Ctrl+Shift+E Network (the shortcuts are already recognised and currently only open the panel); "Inspect element" in the page context menu; persisted panel choice, dock and filter settings in the profile (never data). Panels render from `InspectorClient` caches only; they hold no engine handles.

## 8. Alternatives considered and rejected

| Alternative | Why rejected |
|---|---|
| Port the old Servo-based panels to a different backend unchanged | their data layer was `devtools_traits`/Servo channels (audit §3); only the egui layout, bounds and test assertions are reusable |
| Run V8's CDP inspector server and attach an external front-end | needs a socket in the sandboxed process, would put page-world debugging on a network port, and makes the front-end contract V8's, not Wega's |
| One monolithic inspector crate holding protocol, backends and UI | mixes processes with different authority (browser vs sandboxed content) and would force the root crate to link engine-private code |
| New `wega-inspector-protocol` crate now | no independent consumer and it would duplicate the framing, versioning and identity machinery of `wega-protocol` |
| Inspect through injected page scripts (`inspector.js`) | runs in the page's world (the page can observe or tamper), needs a CSSOM the engine lacks, and cannot see the network or cookies |
| Network capture in the content process | a compromised renderer could forge or suppress it, and it would duplicate the fetch path |

## 9. Uncertainties

- V8 inspector behaviour when attached lazily and when paused (§5.2) is untested; the D5 prototype decides whether the design holds or the inspector must be enabled at runtime creation.
- Matched-rule extraction from Stylo through the Blitz adapter has not been prototyped; the size of the new contract (§5.3) is an estimate.
- Whether the engine's revision counter is fine enough for DOM change events is unknown.
- The completeness of response-header filtering between the broker and the engine (whether `set-cookie` reaches the engine) was not verified; D4 must check and, if needed, fix it before relying on redaction.
- Child frames, workers and shadow trees are not inspectable because the engine does not model them as realms or rendered trees; the design keeps ids frame-aware for later.
- Old DevTools quality (did the old harness assertions pass?) is unverified, so the old tests are used as acceptance *criteria*, not as proof of past behaviour.
