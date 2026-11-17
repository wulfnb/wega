# Iframe design (security-first)

Status: design, written **before** production changes (commit 1 of the iframe milestone).
Scope: `<iframe>` browsing contexts in the Wega content engine (`engines/wega`), their
integration with the existing protocol `FrameTree`, rendering, input and D3 DevTools.
Companion documents (written at the end of the milestone): `iframe-implementation-report.md`,
`iframe-security-review.md`, `iframe-wpt-compatibility.md`, `baseline/iframe-evidence.json`.

Priority order: **security and document isolation > feature completeness**. A sub-feature that
cannot be made safe stays unsupported and fails closed; it is listed in §9.

## 1. Audit of the current architecture (source-verified)

| Area | Finding | Class |
|---|---|---|
| HTML parsing | The parser (`engines/html/parser`) builds `<iframe>` elements as ordinary DOM nodes; `srcdoc` is kept as an attribute. | implemented |
| DOM representation | `HTMLIFrameElement` (`dom/semantics.js`) has `src/srcdoc/name/width/height/sandbox` reflecting attributes; `contentWindow`/`contentDocument` are hard-wired `null`. | partial |
| Browsing contexts (engine) | One `View` = one Blitz document + one V8 `JsRuntime` (own isolate) + one `Dom`. No child contexts. | missing |
| Browsing contexts (protocol) | `engines/protocol/src/browsing.rs` `FrameTree`: bounded contexts (128), depth (16), opaque-origin derivation for `about:srcdoc`/sandbox, sandbox inheritance, message acceptance, detach revokes descendants. `Command::FrameNavigate/FramePostMessage`, `Event::FrameAttached/Detached/Navigated/FramePostMessage`, `Capability::SiteIsolationFrames`. Host (`src/wega_page.rs`) validates against it. | implemented (host side), unused by the engine |
| Shared frames | `SharedFrames`/`InheritedFrames` are *frame buffers* (raster slot sharing), not browsing contexts. Not to be confused with iframes. | implemented (different concept) |
| Rendering | Blitz lays out and paints one document; an `<iframe>` is an empty box. `View` reports `compatibility = "Visible iframe rendering is not implemented by Wega"`. Output is an `anyrender::Scene` rasterised by the Skia adapter (`painter.append_scene(scene, Affine)` is available). | missing |
| Layout safety | Depth guard (`MAX_EDIT_DEPTH` 121; nested block ≥122 hangs, roadmap B1). Per-document. | implemented (limits documented) |
| Network | `Network` (`engines/wega/src/network.rs`) with policy, cookies (per-profile jar, `cookies.context(url, Some(initiator), top_level)`), referrer, mixed content, CSP `connect-src`; `FetchContext.destination = Document`. | implemented |
| CSP | `Csp` supports fetch directives; `frame-src`, `frame-ancestors`, `sandbox`, `form-action`, `base-uri` absent (roadmap B2). | missing |
| X-Frame-Options / COOP / COEP / CORP | Absent (roadmap B3). | missing |
| Origins | Derived from the document URL (`url.origin()`); no per-document origin override. `about:srcdoc` yields an opaque origin. | partial |
| Window relationships | `window.parent/top/frames` are `globalThis`, `frameElement` null, `length` 0. `postMessage` is a self-post (structured clone, `targetOrigin` check, task). | partial |
| Input | `View::pointer/click/key/scroll_at` operate on the single Blitz document. | partial (no frame routing) |
| Inspector | D3 `inspector` protocol is per `View` with `frame`/document identity in the session; node handles are `NodeKey`s of one document. | partial (single document) |
| Process model | One content-worker process per tab (signed, sandboxed). Isolation between sites inside the process is therefore *logical* (isolate + principal), not OS-level. | implemented (design constraint) |

## 2. Core decision: a child frame is a nested `View`

Reuse, do not duplicate. The parser, script scheduler, network, CSP, load/commit pipeline,
document generation/revision, console, permissions and budgets are all `View` behaviour.
A **child browsing context is a `View` owned by its parent `View`** (`frames::FrameHost`), keyed
by the `<iframe>` element:

```
View (top)                        FrameHost (per View)
 ├─ Blitz document (parent)        ├─ child[frame_id] = ChildFrame {
 ├─ V8 realm (parent)              │     view: View            // own doc, realm, Dom, revision
 └─ frames: FrameHost              │     element: NodeId        // the owning <iframe> in the parent
                                   │     principal: Principal   // committed security principal
                                   │     sandbox: SandboxFlags  // effective (inherited ∪ own)
                                   │     name, depth, nav_epoch // navigation identity
                                   │  }
```

* **Frame identity ≠ document identity.** `FrameKey` (u64, monotonically increasing, never
  reused within a top-level `View`) identifies the browsing context; the child `View`'s
  `document_generation`/`document_revision` identify its active document. Navigating a child
  replaces only that child's document (a new document ⇒ new realm, new `Dom`); the frame key
  is stable, siblings and the parent are untouched. A removed frame's key is never reissued, so
  stale references cannot alias a new frame.
* **Separate realms.** Each child `View` creates its own `JsRuntime`/isolate. There is no
  shared heap between frames; this is a deliberate security property (§4) and it is what makes
  cross-origin isolation inside one process defensible.
* **Bounds** (constants in `frames.rs`, surfaced in the evidence file): nesting depth 8 (below
  the protocol's 16 and far below layout's 121), live frames per tab 32, pending frame
  navigations 8, srcdoc bytes = `Limits::html_bytes`, frame viewport pixels = `Limits::max_pixels`.
  Over-limit creation yields a frame with no document (blank, non-scriptable) and a console
  diagnostic; it never panics and never recurses unboundedly.
* **No recursive synchronous layout.** Each child lays out *its own* document when painted
  (`record_paint` on the child), bounded by depth ≤ 8; the parent never lays out the child's
  tree. The parent only supplies a content box (from its own layout) as the child viewport.

## 3. Document, origin and sandbox model

Origin is a property of the committed document, not of a URL string compared later.

`Principal` = `{ origin: Origin | Opaque(nonce), sandbox: SandboxFlags }`, computed once at
commit from: the response URL (after redirects) for `src`; the *creator's* principal for
`about:blank`/`srcdoc` (inherits unless sandboxed); and the sandbox flags.

| Case | Origin |
|---|---|
| `src=https://a/…`, no sandbox | `a`'s origin, taken from the **final** URL after redirects |
| `src` + `sandbox` without `allow-same-origin` | opaque (unique per document) |
| `srcdoc`, no sandbox | parent's origin (inherited) |
| `srcdoc` + sandbox without `allow-same-origin` | opaque |
| `about:blank` / empty `src` (initial document) | parent's origin (inherited), opaque if sandboxed |
| `data:`, `blob:`, `javascript:` frames | unsupported, fail closed (blank) |

Implementation of inherited/opaque origins: the engine's origin derivations read the document URL
(`Dom::document_origin`, cookies, fetch). To avoid scattering an override, the child `View` is given
an explicit **document origin override** consumed by one function (`Dom::document_origin` and the
helpers that currently call `url().origin()`); any call site not routed through it is treated as a
defect and covered by a test (an opaque-origin frame must not send cookies, must not read the
parent's storage, and must get `Origin: null`). Where a sink cannot honour an opaque origin the
sink is **disabled for opaque frames** (storage, cookies, IndexedDB) rather than falling back to the
URL origin.

Sandbox flags honoured (enforced, each with a test): `allow-scripts` (absent ⇒ no realm is created
at all: no script, no event handlers), `allow-same-origin`, `allow-forms` (form submission blocked
when absent), `allow-popups` (window.open blocked when absent), `allow-top-navigation` family
(top navigation from a child is always refused in this milestone — fail closed),
`allow-modals`. Flags not enforced (`allow-pointer-lock`, `allow-presentation`,
`allow-downloads`, `allow-orientation-lock`, `allow-popups-to-escape-sandbox`,
`allow-storage-access-by-user-activation`) are **treated as absent** (the capability stays denied)
and are reported unsupported; the effective policy is never more permissive than requested.
Nested sandboxes inherit: the child's flags are the union of restrictions (the protocol
`FrameTree` already implements this and is reused as the single source of truth for inheritance).

Navigation authorization is by committed principal, never by URL equality:
* a script may navigate a frame it can legitimately script (same committed origin, or the frame is
  its own descendant); sandboxed frames cannot navigate their ancestors or top;
* a navigation request is bound to `(FrameKey, nav_epoch)`; a response arriving for an older
  epoch, a removed frame, or a replaced element is dropped (stale-commit protection);
* redirects are re-evaluated: the principal is computed from the **final** URL, and the policy
  checks (CSP/XFO when available, mixed content, scheme allow-list) run on every hop.

## 4. JavaScript and Window relationships

* `iframe.contentWindow` returns a **WindowProxy** object: a host-backed, per-realm facade
  (not the child's `globalThis`, no engine object crosses isolates). Its public surface is the
  cross-origin-safe set only: `postMessage`, `closed`, `frames`, `length`, `parent`, `top`,
  `opener`(null), `window`, `self`, `location` (write-only `replace`/`href` set, subject to
  navigation authorization). Any other property read on a cross-origin proxy throws a
  `SecurityError` DOMException.
* The child realm's `window.parent` / `window.top` are the same kind of facade pointing at the
  owning frames; `window.frameElement` is `null` unless the parent is same-origin *and* the
  facade can safely hand back the element (see blocker B-1: `null` in this milestone).
* `postMessage` crosses frames through the **host** (`FrameHost::deliver`), never by object
  reference: the sender structured-clones to a JSON-compatible form (non-representable values,
  transfers and ports ⇒ `DataCloneError`, fail closed), the host validates the target frame is
  live and its `nav_epoch` matches the proxy's, applies `targetOrigin` against the target's
  **committed principal** (opaque targets only match `"*"`), and queues a task on the target
  realm. The `MessageEvent` carries the **host-computed** `origin` of the source principal (a
  page cannot forge it) and `source` as a facade for the source frame.
* Same-origin synchronous DOM access (`contentDocument`, `contentWindow.document`) requires a
  cross-isolate bridge over Wega's JS DOM layer (`semantics.js`, a global-realm install with
  per-realm wrapper caches). That is a separate project; **B-1: `contentDocument` returns `null`
  and same-origin `contentWindow` exposes only the safe surface.** Same-origin frames can still
  cooperate through `postMessage`. This is documented as unsupported, never faked.
* Task/microtask ordering: cross-frame message delivery uses the target `View`'s existing
  scheduler timer queue (the same mechanism as the in-realm `postMessage`), so microtask
  checkpoints and ordering semantics are unchanged.

## 5. Lifecycle

States: `Created → Navigating(epoch) → Committed(doc) → (Navigating …)* → Detached`.

* **Create**: the parent's `poll`/layout pass discovers `<iframe>` elements (parser-created or
  script-created, plus attribute changes) via a bounded scan keyed by element `NodeId`; a new key
  is allocated, the initial `about:blank` document is created synchronously, then `src`/`srcdoc`
  navigation starts. `srcdoc` takes precedence over `src`.
* **Navigate**: `src`/`srcdoc` change ⇒ new `epoch`, abort the child's pending fetch
  (`stop_pending`), start a new load; commit only if `(key, epoch)` still current.
* **Commit**: principal computed from the final URL; child realm created (unless sandboxed
  without `allow-scripts`); `load` event on the `<iframe>` element in the parent after the child
  `load`. Failure ⇒ child shows the standard error document, `error` is **not** fired on iframes
  (per HTML, only `load`), and the frame stays alive.
* **Detach** (element removed, replaced, parent navigates, tab/view dropped): the child `View` is
  dropped, which tears down its realm, timers, listeners, pending fetches (`AbortController`) and
  permission requests (`cancel_document`); the key is retired; messages queued for the key are
  discarded; D3 inspector handles for the key are invalidated.
* **Parent navigation**: dropping the parent `View` document drops `FrameHost` first.
* **Worker crash/restart**: the content process owns all frames; a crash loses all of them and
  the host rebuilds from the protocol `FrameTree` (already torn down by the host on process
  loss). No child state outlives the process.

Back/forward: session history stays per top-level `View` (existing `traverse`); frame
navigations do not create separate joint-session-history entries in this milestone (partial,
documented). `location.replace` semantics are the child's own.

## 6. Rendering and compositing

* The iframe element's **content box** comes from the parent's own layout
  (`BlitzDocument::content_box`), converted to device pixels with the parent's `scale * zoom`.
* The child `View` paints into an `anyrender::Scene` at the content-box size
  (`record_paint`), and the parent **composites that scene** into its own scene with a
  translation and a clip to the content box (`append_scene` with `Affine`); no DOM flattening,
  no second layout engine, no pixel readback.
* Parent scrolling: the box is mapped through the parent's scroll offset so the frame moves with
  the content; clipping is the intersection with the parent viewport.
* Child scrolling uses the child document's own viewport scroll; wheel events are routed to the
  child first and chain to the parent only if the child cannot scroll (existing scroll-chaining
  rules).
* **Known limitation L-1 (paint order):** frames are composited **after** the parent scene, so a
  positioned parent element with a higher z-index does not overlap the frame. Correct stacking
  needs paint-time hooks inside Blitz's painter (a `replaced content` callback), which is a
  vendor-integration task and the right seam for the Blitz→Wega layout replacement: the compositor
  interface (`FrameCompositor::paint_into(scene, box, clip)`) is engine-neutral so a future
  layout engine can call it from its own paint pass.
* Invisible/zero-size/`display:none` frames do not paint and are not laid out; `visibility:hidden`
  does not paint; `loading=lazy` frames below the viewport by more than one viewport height are
  not navigated until they near it (feature-gated, tested).

## 7. Input and focus

Hit testing is performed against the parent document; if the hit node is an `<iframe>` with a live
child, the event is **consumed by the frame** (it never reaches parent elements behind it) and the
point is converted: `child = (parent_point − content_box_origin − border/padding) / (scale·zoom)`
applied recursively for nested frames. Keyboard and IME go to the **focused frame chain** (the
single leaf frame that has focus); focus moves between frames only by pointer activation inside the
frame, by `focus()` on same-origin... (via the proxy: `contentWindow.focus()`), and on removal
focus returns to the parent. Frame removal or navigation cancels in-flight pointer sequences for
that frame. The browser-owned DevTools picker keeps interception priority above frame routing
(it runs before `View::pointer`, unchanged).

## 8. DevTools (D3) integration

Reuse the D3 session/protocol/mutation-epoch/cache/authorization contracts. The inspector
`frame` dimension becomes meaningful: every inspector request names a `FrameKey`; node handles
are `(FrameKey, document revision, NodeKey)`, so a handle from frame A is rejected by frame B and
from a navigated frame. Cross-origin inspection goes through browser-owned DevTools authority
(`Command` handled by the engine, not exposed to page JS). Mutation grants are per frame and are
revoked on frame navigation/replacement/removal. The plan and what lands in this milestone are in
the implementation report; items that depend on cross-frame hit-testing geometry are only enabled
where the compositor can prove the transform (§6).

## 9. Network and policy

Frame loads use `View::load_request` → `View::fetch` → `Network::request` with
`FetchContext { destination: Document }` and the **initiator** being the parent document; the
existing cookies, referrer, mixed-content and redirect handling apply. No new network authority is
given to workers. `frame-src`/`child-src` (CSP of the **parent**), `frame-ancestors` and
`X-Frame-Options` (response of the **child**) are implemented for this milestone because they are
the embedding checks the roadmap puts in B2/B3: a frame response that fails them is replaced by a
blocked-frame document and is never scriptable. CORS is **not** used for navigation authorization.
Permissions Policy, COOP/COEP/CORP, and third-party cookie partitioning remain **security gaps**
(documented, not silently permissive: frames are allowed the same permissions as the top document
only when same-origin; cross-origin frames get none, fail closed).

## 10. Implementation Status of Initial Blockers

* **B-1 Same-origin synchronous DOM bridge** (`contentDocument`, `frameElement`): **RESOLVED in Stage 2**. Implemented using generation-safe host-backed facades over Wega-owned DOM and V8 host interfaces without exposing raw pointers or duplicating the authoritative DOM. Origin checks enforced at access time; cross-origin and opaque sandboxed frames fail-closed with `SecurityError` and `null`.
* **B-2 OS-process isolation per site**: Maintained as documented architectural constraint (one signed, sandboxed content worker per tab). Isolation is strictly enforced at V8 isolate boundaries and committed security principals.
* **B-3 Z-order interleaving of frames with positioned parent content**: **RESOLVED in Stage 3**. Integrated into display-list / scene pipeline (`PaintScene::Iframe`), supporting parent stacking contexts, clipping, and z-ordered hit-testing.
* **B-4 Joint session history for frame navigations**: Intentionally roadmapped for browser-level navigation history overhaul.
* **B-5 `data:`/`blob:`/`javascript:` frame URLs, `<object>/<embed>` as frames, `<frameset>`, `allow` (Permissions Policy), `allowfullscreen`, `credentialless`, `csp` attribute**: Stay fail-closed and denied.
* **B-6 Process-level worker per cross-origin frame**: Architectural constraint (one worker per tab).

## 11. Test strategy

Deterministic loopback HTTP fixtures (existing `pages.rs` pattern) for every case in the task's
§12 matrix, hostile cases (forged key, stale epoch commit, redirect origin change, sandbox escape,
oversized srcdoc, deep nesting, frame-creation loops, removal during pending request) run under
an externally-killable subprocess where a hang is conceivable; protocol-level tests for the
`FrameTree` contracts the engine consumes; native-window and signed-worker tests re-use the D3
harness. Benchmarks use release builds and are recorded in `baseline/iframe-evidence.json`.
