# D3 final: geometry, picker, overlay, Box Model, authorized editing — design

Baseline: `589dc12` (D3-C.0 complete). Predecessors:
[`devtools-d3-c-0-design.md`](./devtools-d3-c-0-design.md) and
[`devtools-d3-c-0-report.md`](./devtools-d3-c-0-report.md). This document fixes the decisions for
the final D3 round; results are in `devtools-d3-final-report.md` and
`devtools-d3-acceptance.md`.

## 1. Layout safety validation (precondition)

The guard from D3-C.0 (`MAX_LAYOUT_DEPTH = 256`, tree depth from the document root) is validated
rather than assumed:

* The engine thread's stack size becomes one constant, `wega_engine::ENGINE_THREAD_STACK`, used by
  `main.rs` and by every probe. The guard is valid only for that size.
* Probes run in killable children on a thread with exactly that stack, in debug **and release**,
  at depths 120, 122, 128, 200, 256, 257, 520, 600 and 1,000, through script flush, paint, paint
  then flush and a "misc" route (computed style, `scrollIntoView`, geometry reads, resize, hidpi
  and dark repaint, pointer), then recover with an ordinary page on the same view.
* The **unguarded** cliff of each build is measured with a `#[doc(hidden)]` probe hook that lifts
  the limit in a child process only; the safety margin is `cliff / 256`. If the release margin is
  below 2x the limit is lowered, never raised silently.
* Only the adapter's `flush_layout` reaches Blitz `resolve` in production code (verified by
  search); the other callers are vendor unit tests.
* Linux and Windows cannot be measured on this host; that stays an explicit unverified item.
* The O(depth²) HTML parser cost stays tracked as an engine defect.

## 2. Protocol (inspector message set 5)

`INSPECTOR_VERSION` becomes 5. All new commands are closed enum variants with typed arguments
and bounded responses, like the D3-A/B commands, and all pass the existing envelope checks
(session, target, process, document stamp).

| Command | Args | Response | Authority |
|---|---|---|---|
| `GetGeometry` | `DomNode { node }` | `Geometry(GeometryReport)` | session |
| `PickNode` | `Pick { x, y }` (viewport CSS px) | `Picked(PickReport)` | session |
| `GrantMutation` | `MutationGrantArgs { token, opaque_override, origin }` | `MutationGranted` | browser UI only |
| `RevokeMutation` | none | `MutationRevoked` | session |
| `Mutate` | `Mutate { grant, node, epoch, op }` | `Mutated(MutationOutcome)` | mutation grant |

`GeometryReport { document, epoch, node, state, boxes, viewport, paint }`:

* `state`: `Current`, `Stale`, `Pending`, `NoBoxes`, `Refused`, `Invalid` (the `Geometry` enum of
  `wega-dom`, mapped 1:1). `boxes` is present only for `Current` and `Stale`.
* `viewport { width, height, dpr, scroll_x, scroll_y }` in CSS px, from the same cached state.
* `paint` says whether the boxes equal the painted geometry: `Exact { clip }` (an optional clip
  rectangle in viewport CSS px, the intersection of ancestor overflow clips) or `Unsupported
  { reason }` with reason `Transform`, `ClipPositioned` or `Unknown`.
* A node reference is `InspectorNodeId { document, key }`; a key is the engine's generation
  bearing node key. A reply is bound to the document stamp and mutation epoch the backend saw and
  the browser drops a reply whose scope differs from its current one.

`PickNode` runs `HitTest::element_at` against cached layout, with no script and no flush, and
returns the topmost element's `GeometryReport` plus its ancestor chain's node ids bounded to 128
so the browser can expand the tree without N round trips. A point over nothing returns the root
element, as `elementFromPoint` does; a document with `Pending`/`Refused` layout returns the state,
not a guess.

### Mutation operations (`MutationOp`)

`SetAttribute { name, value }` (add or change), `RemoveAttribute { name }`,
`SetText { text }` (text and comment nodes only), `RemoveNode`, `SetInlineProperty { property,
value, important }` (empty value removes). No HTML replacement, no subtree insertion.

Limits: name 128 bytes and the HTML attribute-name grammar; value 4 KiB; text 4 KiB; CSS property
128 bytes, value 2 KiB; total request under 8 KiB. Event-handler attributes (`on*`), `srcdoc`
and `<script>` text are refused unless the evaluation grant for the same session and document is
also live, because setting them is code execution.

## 3. Authorization of mutation

* A separate `GrantToken`-shaped secret, a separate browser record, and a separate content slot;
  no code path accepts one in place of the other (tests both ways).
* The browser mints it only from a trusted UI action ("Enable editing") and only from the
  committed principal (`policy::authorise`, same opaque-origin rule as evaluation).
* Bound to session, target (tab, frame, process), document stamp and principal; compared in
  constant time; re-verified in the content process immediately before applying, together with the
  principal consistency check and the **mutation epoch** the edit names. An epoch mismatch is
  `StaleMutation`; nothing is applied.
* Revoked on navigation or document replacement (the scope check fails and the browser clears its
  record), process replacement or crash (new `ProcessSessionId`), detach, session revocation,
  explicit revoke, tab close, DevTools disconnect.
* Page script cannot reach any of this: the pipe is browser-only (existing guarantee) and no new
  host op exposes inspector commands.
* Mutation applies through `Dom::call` host ops (`setAttr`, `removeAttr`, `setText`, `remove`,
  `styleSet`), the **same** functions script uses, so budgets (text bytes), invalidation
  (`layout_dirty`, revision), custom-element reaction queue and the mutation epoch are identical.
  Known difference: page `MutationObserver` records are produced by the script-side wrappers and are
  not produced for native edits; documented as a limitation.
* Undo is **not implemented** and is not advertised.

## 4. Picker

* UI state per DevTools target: `picker: Off | Active`, owned by the browser. The control lives in
  the Elements panel toolbar (and therefore works in the detached window, which holds a pinned
  target).
* Interception point: the egui pointer loop in `src/wega_ui.rs` consults the active tab's picker
  state *before* forwarding `Command::Pointer`. While active, moves, presses, releases and leave
  are consumed (never forwarded), so the page sees no event and cannot react; wheel stays
  forwarded so the user can scroll. A click selects and exits; Escape or the toolbar button exits.
* Hover requests are coalesced: at most one `PickNode` outstanding; the newest pending position
  replaces older ones.
* Cancellation: navigation, document change, tab switch, detach, process replacement, session
  revocation and DevTools close set the picker to `Off`, drop the outstanding request and clear
  the hover highlight. A reply whose target/session/document/epoch no longer matches is discarded.

## 5. Overlay and highlight

* Browser-owned egui painter above the page rect; never touches the page DOM, layout, page hit
  testing or page screenshots (the page texture is captured before the overlay is drawn).
* Mapping: `screen = page_rect.min + css * page_zoom`. Boxes are viewport-relative CSS px that
  already include viewport and nested scroll (measured in D3-C.0). DPR needs no extra factor.
* Accuracy contract (`PaintMapping`): the adapter reads, from cached Stylo computed values, whether
  the node or any ancestor has a non-identity `transform`/`translate`/`rotate`/`scale`/
  `perspective`; if so `Unsupported(Transform)` and **no highlight is drawn**, only a notice. For
  clipping, every ancestor whose `overflow-x/y` is not `visible` contributes its padding box; the
  intersection is returned as `clip` and the overlay clips to it. If the chain contains a
  `position: fixed|absolute|sticky` element and any clipping ancestor, containing-block rules decide
  which clips apply; the adapter does not model that and returns `Unsupported(ClipPositioned)`.
  Hit-testing results are not trusted to be paint-accurate under transforms either.
* Selected element: persistent highlight; hover: lighter highlight while the picker is active.
  Both draw margin, border, padding and content bands with a label (tag, `#id`, classes, size).
* Overlapping/stacking: the overlay draws above the page; it highlights the chosen element even if
  another element paints over it, as other DevTools do. Hit testing decides which element is chosen.
* Stale geometry (`Stale`) is drawn dashed and dropped as soon as the epoch advances again.

## 6. Box Model panel

Read-only diagram of the same `GeometryReport`: concentric margin/border/padding/content with the
eight edge values, position and size, freshness text and an explicit message for `Pending`,
`NoBoxes`, `Refused`, `Invalid` and unsupported paint mapping. It requests a new report when the
selection or the document epoch changes (one outstanding request, coalesced), and shows only
the report whose scope matches the current one. No editing through the diagram.

## 7. Elements editing UI

An "Editing" indicator and Enable/Revoke control in the Elements toolbar (distinct from the
Console's evaluation grant). When enabled: attribute rows become editable (edit value, remove,
add), text nodes editable, a Remove node button, and the inline style editor (add, change, remove
declaration, `!important` toggle). After a successful edit the browser refreshes the affected
parent page, the selected node, geometry and the style page, using the returned epoch; it never
rebuilds the whole tree. One mutation outstanding at a time; further edits queue up to a small bound
and are dropped (with a visible notice) beyond it. Errors (`StaleMutation`, `BudgetExceeded`,
`Unauthorized`, `InvalidPayload`) are shown inline.

## 8. Matched CSS rules

D3-B proved retained declaration-block identity cannot identify the matching selector. A fix
needs provenance retained during matching inside vendored Stylo/Blitz, with measured matching
overhead. It is optional for D3 completion and is **not attempted**: the Unsupported state stays,
with no guessed selectors, specificity or winners.

## 9. Evidence plan

Unit tests (protocol shapes, bounds, grants); engine integration tests per command (stale,
cross-document, over-budget, transforms and clips fixtures); browser session tests (host grant
lifecycle, picker state machine, coalescing); real-process inspector tests with the engine harness;
the native end-to-end scenario with the signed worker where the environment allows it; optimized
benchmarks with alternating isolated builds for the closed-DevTools and console baselines.
