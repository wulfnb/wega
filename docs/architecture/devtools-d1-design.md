# DevTools D1: inspector foundation, IPC and lifecycle (design)

Status: design, written and committed **before** any D1 runtime change. Parent documents: [`devtools-design.md`](./devtools-design.md) (D0 design), [`devtools-implementation-roadmap.md`](./devtools-implementation-roadmap.md) (D1 section), [`devtools-current-capabilities.md`](./devtools-current-capabilities.md), [`devtools-pr58-deletion-audit.md`](./devtools-pr58-deletion-audit.md).

Scope: a typed, authenticated, bounded channel between the browser UI and one selected Wega content process, with the existing status panel fed through it. **Out of scope:** console, evaluation, DOM/CSS inspection, network, debugger, remote transport, WebDriver changes (D2+).

## 1. Baseline (Phase 1 evidence)

| Item | Finding |
|---|---|
| Branch / HEAD | `feature/dom-rounds-9-12` at `2a763f3` (D0 documents), worktree clean at start. |
| Protocol | `wega-protocol` `VERSION = 21`, accepted range 4..=21. Length-prefixed JSON, `MAX_MESSAGE` 1 MiB. Every post-`Init` packet is wrapped in `SessionIdentity{tab, frame, process(16 random bytes), service}`; mismatches and nested envelopes are rejected on both sides. |
| Browser → content path | UI thread → `Page::send` (`try_send` into a `sync_channel(32)`) → writer thread (admission check against `Snapshot.requests`, wraps in the identity, writes the pipe). Replies: reader thread validates identity, then updates `Snapshot` under a mutex and wakes the UI (`Wake::Ui`). The UI never waits on the pipe. |
| Request correlation | `DocumentRequests` (host: ≤ 32 pending, deadlines, cancelled on navigation/commit, late replies ignored) and `CommandQueue` (engine: admit-once replay protection, queue deadlines, `CancelRequest`). Both key on `scoped_context(&Command)`. |
| Tab and process identity | `Tab.key` is a monotonic never-reused `u64` (`next_tab_key`), used as `TabId`. A `Page` (one OS process) owns a fresh random `ProcessSessionId`; a restart, discard or crash recovery creates a new `Page` and so a new process identity. |
| Authority today | `FrameAuthority` (browser-owned, in `Snapshot`) binds a document principal; only the browser writes `Command`s; the content process can only emit `Event`s which the reader validates. Page script has no handle to the pipe. |
| DevTools shell | `devtools_ui.rs` renders the panel from `self.tab().wega` (always the **currently selected** tab) reading `Snapshot` fields directly; `tool_window.rs` hosts the detached window and calls the same `devtools_contents`; F12 toggles `chrome.show_console`; Cmd/Ctrl+Shift+I/E/K only open the panel. |
| Architecture checker | `wega-architecture-check`: 13 members, 24 internal edges, **0 violations**, 11 informational. Dependency validation (`check-engine-boundary.py`) passes. |
| Test baseline | `wega-protocol`: 39 passed. Engines workspace per the Round 12 report: 509 passed, 0 failed, 12 ignored. Re-run results are in the D1 report. |
| Disk | Volume at 99% (6.5 GiB free) with `target/` 42 GiB and `engines/target` 14 GiB; builds are run one target at a time. |

**Conflicts with the Round 12 roadmap:** none. D1 touches the protocol crate, the engine's `main.rs`/`view.rs` and the root crate's page/UI code; Round 12 left all of them stable. Items deferred by Round 12 (fuzzing infrastructure, Blitz retirement) stay deferred.

## 2. Decisions and deviations from D0

| # | D0 text | D1 decision | Why |
|---|---|---|---|
| 1 | `Domain.method` string envelope with `params` | **Typed Rust enums** (`InspectorCommand`, `InspectorResponse`, `InspectorEvent`), serde-tagged, `deny_unknown_fields` on requests | The brief asks for typed representations and no general-purpose command execution; a closed enum makes "arbitrary command" unrepresentable. A method-string layer can be added in front for D7/remote debugging without changing the backend. |
| 2 | Module `devtools/` in the root crate | Flat modules `src/devtools_session.rs` (+ `_tests.rs`), following `devtools_ui.rs`/`tool_window.rs` naming | Repository convention is flat `src/*.rs` with `*_tests.rs` via `#[path]`. |
| 3 | `content_event()` downgrade arm | Not needed. Inspector events are **only ever emitted to a session the browser explicitly attached over a v22 pipe**; they are top-level `Event`s, not `ContentEvent`s | There is no older worker that could be sent one. `Capability::Inspector` (v22) gates it. |
| 4 | `DevtoolsAuthority` per tab in the root crate | Authority logic lives in `wega_protocol::inspector::session` (`SessionIssuer`, `SessionRegistry`) so it is unit-testable in the engines workspace; **ownership** stays browser-side: one registry inside each `Page`'s `Snapshot` | The registry must die with the process it protects (rule 6) and `Snapshot` is already that scope. No second tab registry or process manager. |
| 5 | `Target.list` domain method | Not in D1: there is exactly one target kind (a tab's top-level frame) and only the browser UI can pick it. `GetTargetInfo` covers a single attached target. | Nothing to list yet; avoids an enumeration surface. |
| 6 | Credit-window flow control | Simplified: producer is the engine's DOM loop which **only emits on change** (≤ 2 events per loop iteration, none without a subscription); the browser reader drains the pipe into a **bounded per-session ring (64 events, drop-oldest, counted)**. Full credit windows wait for a noisy domain (D2 console). | A credit protocol with no noisy producer is untestable ceremony. The reader thread never blocks on the UI, so the pipe cannot back up behind a slow panel. |
| 7 | Fuzz target | Deferred; replaced by a table-driven malformed-payload test (truncated, oversize, unknown fields/variants, wrong types) | Fuzz infrastructure does not exist (Round 12 deferred item). |

## 3. Protocol: `wega_protocol::inspector`

New files `engines/protocol/src/inspector.rs`, `inspector/session.rs`, `inspector_tests.rs`. No `wega_dom`, Blitz, V8, Deno or Servo name appears in them (a boundary test reads the sources and asserts it).

```
VERSION                      21 → 22          Capability::Inspector (>= 22)
inspector::INSPECTOR_VERSION 1                 accepted range 1..=1

TargetId            { tab: TabId, frame: FrameId, process: ProcessSessionId }    from SessionIdentity
InspectorSessionId  (u64)   browser-issued, strictly increasing, never reused, never 0
DocumentContext     (existing) { document: DocumentId, revision: DocumentRevision }   = "document/execution-context generation"
RequestContext      (existing) carries correlation id + document stamp
```

### Wire messages

```
Command::Inspector { context: RequestContext, request: InspectorRequest }     scoped (correlation, deadline, cancel, replay guard)
Command::InspectorDetach { session }                                          unscoped, idempotent, no reply

InspectorRequest { version: u32, session, target: TargetId, command: InspectorCommand }
InspectorCommand = Attach | Detach | GetTargetInfo | SubscribeStatus | UnsubscribeStatus | GetStatusSnapshot

Event::InspectorResponse { context: RequestContext, session, result: Result<InspectorResponse, InspectorError> }
Event::InspectorEvent    { context: DocumentContext, session, seq: u64, event: InspectorEvent }

InspectorResponse = Attached | Detached | TargetInfo(TargetInfo) | Subscribed | Unsubscribed | Status(Box<StatusSnapshot>)
InspectorEvent    = DocumentChanged | LoadingChanged{loading} | EventsDropped{count}   (the last is synthesised browser-side)
InspectorError { code: InspectorErrorCode, message (≤ 512 chars) }
InspectorErrorCode = Unauthorized | UnknownTarget | StaleSession | StaleDocument | Unavailable |
                     UnsupportedVersion | Timeout | Cancelled | InvalidPayload | Busy | SessionLimit
```

`Detach` exists both as a command (symmetry, in-band reply) and as the unscoped `InspectorDetach` the browser actually uses for revocation, because a scoped detach can be discarded by the writer's stale-document admission after a navigation, which would leave the engine believing a revoked session is live. The engine treats both identically and idempotently.

`StatusSnapshot` (bounded; every string truncated, total ≪ 16 KiB): `target`, `document: DocumentContext`, `url` (≤ 2048), `title` (≤ 512), `loading`, `viewport: Option<{width, height, device_pixel_ratio}>` (CSS px, from the existing `ViewportGeometry`, no layout forced), `nodes`, `js_heap_bytes`, `can_go_back/forward`, `protocol_version`, `inspector_version`, `engine: "wega-engine <crate version>"`. Only fields already maintained by the view are included; none requires layout, style resolution or script execution.

### Limits (constants in `inspector.rs`)

`MAX_INSPECTOR_PENDING = 4` (browser-side, a sub-cap inside the shared 32 so a panel cannot starve evaluation/harness requests), `EVENT_RING = 64`, `RESULT_RING = 8`, `INSPECTOR_TIMEOUT_MS = 2000`, `MAX_FIELD = 2048`, `MAX_SESSIONS_PER_TARGET = 1`.

## 4. Authority and session ownership

**Principle:** the browser process issues, validates and revokes; the content process holds a *mirror* it uses only to refuse, never to grant.

```
SessionIssuer       process-wide atomic counter → InspectorSessionId; checked_add, error at exhaustion; ids are never recycled.
SessionRegistry     one per Page (inside Snapshot), bound to that Page's TargetId.
  open()            supersedes any existing session (returns the revoked id so the caller sends InspectorDetach); returns a new id
  authorize(s, t)   Ok only if s is the live session AND t == registry target, else Unauthorized / StaleSession / UnknownTarget
  revoke(s) / revoke_all()
  mark_subscribed() browser-side mirror of the subscription flag
```

Rules and how each is met:

1. **Only trusted UI initiates.** The only callers of `Page::inspector_open` are `DevtoolsView` methods invoked from `Browser` shortcut/button handlers (egui, native). There is no host op, WebDriver command, URL scheme, `wega://` page action or protocol event that creates a session. A content-originated `Event` cannot contain a `Command`. The engine mirrors this: an `Inspector` command is processed only if it arrived as a `Command` on stdin inside the validated session envelope.
2. **Page JS cannot construct or submit commands.** The page has no pipe handle (sandboxed process, pipe is the child's stdio) and the engine JS host exposes no operation touching the inspector. Test: scan the page's global object and host-op surface for any inspector/devtools reachable name, and assert a script-forged `Event`-like message cannot produce a session.
3. **Scoped to selected tab and target.** `SessionRegistry::authorize` compares the request's `TargetId` (tab, frame, process) with the page's; the engine independently compares it with its own `SessionIdentity`.
4. **Detach revokes.** `inspector_close` revokes in the registry first (so every later reply or event for that id is dropped browser-side), then sends `InspectorDetach`.
5. **Tab closure revokes all.** The registry lives in the `Page`'s `Snapshot`; dropping the page ends the process and the registry. `DevtoolsView::on_tab_closed` additionally clears the view in the same UI turn.
6. **Crash/restart invalidates process-bound handles.** `failed()` calls `registry.revoke_all()`; a replacement `Page` has a new `ProcessSessionId`, so `TargetId` no longer matches and old ids resolve to nothing.
7. **Navigation invalidates document-bound state.** A session is bound to tab+process, **not** to a document (the panel must survive navigation). Document-bound state is: pending requests (cancelled by the existing `commit`/`navigate`, surfacing `StaleDocument`), the cached snapshot (cleared on `DocumentChanged`) and in-flight replies (rejected by `requests.take(context)`). Every request carries the stamp it was issued under.
8. **Tab switching never redirects a session.** A `StatusSnapshot` is rendered only when its `target.tab` equals the view's current binding; the session id lives in exactly one page's registry. Following the selection is an explicit transition (detach A, attach B), never a re-target.
9. **Detached windows keep their target.** See §6 (pinning).
10. **No reuse.** Monotone issuer; the engine mirror tracks `highest_seen` and answers `StaleSession` for any id ≤ it that is not live and `Unauthorized` for greater unknown ids.

## 5. Transport behaviour

| Requirement | Mechanism |
|---|---|
| Typed correlation | `RequestContext` (document, revision, request id); `RequestKind::Inspector(session)` so a reply must match both the pending context **and** its issuing session. |
| Bounded pending | host: `MAX_INSPECTOR_PENDING = 4` (plus the shared 32); engine: existing `CommandQueue` (32, admit-once). Exceeding → `Busy`, never an unbounded queue. |
| Bounded events | engine emits only on change and only if subscribed; browser ring 64, drop-oldest, `EventsDropped{count}` marker delivered in order. Result ring 8. |
| Timeout / cancellation | 2 s deadline in `DocumentRequests`; `Page::inspector_poll` (UI tick) calls `expire_requests` and sends `CancelRequest`, so a hung engine produces `Timeout` without a later send being required. Detach/close cancels the session's pending requests. |
| No UI-thread waiting | All `Page::inspector_*` calls take the state mutex briefly and `try_send`; results are read on a later frame. |
| Disconnect | writer/reader failure → `failed()` → registry revoked, pending cancelled → `Unavailable`. |
| Hung target | UI shows "not responding" after the first timeout; frames still render; `try_send` full → `Busy`. |
| Malformed / stale messages | Engine: unknown variant/field → `InvalidPayload` reply (the pipe's JSON is parsed by serde; a hard parse failure of the whole packet keeps the existing behaviour: session is torn down). Browser reader: mismatched kind/session/context is **dropped**, never panics and never closes the pipe (a late reply is an expected race, not a violation). |
| No cross-tab delivery | Each `Page` has its own pipe, identity check and registry; the view additionally checks `snapshot.target.tab == binding.tab`. |

Limitation recorded: the shared pipe is a single FIFO, so a stalled engine DOM thread also delays other commands; D1 does not add a priority lane. The inspector adds at most 4 pending requests to that FIFO.

## 6. UI policy

* **F12 / Cmd+Shift+I/E/K:** unchanged shortcuts; opening attaches to the selected tab (`DevtoolsView::open`).
* **Close:** detaches (session revoked, `InspectorDetach` sent). No "suspend" state; reopening creates a new session. The page is untouched.
* **Docked panel follows the selected tab:** on selection change the view detaches from A and attaches to B, clearing displayed data first ("Attaching…"). Data from A is never shown under B.
* **Detached window is pinned** to the tab that was selected when it opened (`pinned = dock == Window`). Switching tabs in the main window does not change it; its header names the target tab. If the target tab closes it shows "Target closed"; if the target is unloaded/crashed "Unavailable", and when that tab gets a new process the view **re-attaches** (same tab key, new process → new session id).
* **Navigation:** `DocumentChanged`/`LoadingChanged` events clear the snapshot immediately and trigger a refresh; the panel shows "Updating…" rather than the old document.
* **Refresh:** event-driven plus one request per second while the panel is visible (the only polling; none when closed).
* **Process facts** (pid, resident memory, lifecycle phase) are browser-side supervisor data, shown beside the content snapshot and labelled; they are not routed through the engine.

## 7. Engine backend

`engines/wega/src/inspector/{mod.rs,status.rs}`: `Backend { active: Option<Active>, highest_seen }` with `handle(request, identity, context, status_source) -> Result<InspectorResponse, InspectorError>` and `observe(context, loading) -> Vec<InspectorEvent>` (emits only when subscribed). `View::inspector_status()` (in `view.rs`, which owns the private document handle) returns a plain `ViewStatus`; the module under `inspector/` names no Blitz type. `main.rs` dispatches `Command::Inspector`/`InspectorDetach` and calls `observe` once per loop iteration after `report_commit`; with no session the cost is one `Option::is_some` check.

## 8. Test plan (mapped to the brief)

Engines protocol crate (unit): round-trips, `deny_unknown_fields`, malformed/oversize table, version support, registry authority (open/supersede/revoke, forged, cross-target, stale, exhaustion), ring overflow and marker, boundary-text test. Engine crate (unit): backend session rules and snapshot bounds. **Engine subprocess** (`engines/wega/tests/inspector.rs`, real `wega-engine` over the real pipe, also under broker mode): attach/detach, forged and cross-tab ids, wrong target, stale document after navigation, delayed reply after detach (host-side registry), timeout/cancel, unsupported version, malformed payload, rapid attach/detach, snapshot correctness against the loaded page, event emission, page-JS inaccessibility. Root crate (unit, over the same types): `DevtoolsView` state machine (open/close, tab switch, pinned window, closed tab, crash/restart re-attach, rapid toggles), panel closed ⇒ no session ⇒ no requests.

## 9. Performance plan

Measured with `scripts/engine-bench.py` (baseline `baseline/engine-bench-round12.json`) plus a new test-only measurement for attach/detach/snapshot round trips and a stalled-engine probe (`SIGSTOP` the worker). Only metrics with a comparable baseline are described as "no regression"; everything else is reported as a measurement without a verdict. The Round 12 baseline was taken on a different date and load, so differences within its recorded spread are the only ones called neutral.

## 10. Risks

* A `VERSION` bump touches every older-worker compatibility test; run the full engine suite, not just the new tests.
* `Command` enum growth changes the serde shape only additively; the Init handshake for versions 4..=21 is unchanged.
* Pinned-window re-attach can race a restart; covered by treating `process != session.process` as the single trigger.
