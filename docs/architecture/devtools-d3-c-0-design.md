# D3-C.0: layout safety, geometry feasibility and editing architecture — design

Baseline: D3-B report commit `82e642b`. Scope of this round: investigate the depth-122
layout hang (fixed, see below), design safe cached-only geometry, element picking and
highlighting, and DOM/CSS editing authorization. **Nothing in this document is implemented
except the layout fix and the geometry reader prototype.** No picker, overlay or editing code
exists. Results and the go/no-go table are in
[`devtools-d3-c-0-report.md`](./devtools-d3-c-0-report.md).

## 1. Layout safety

### Finding

The "depth-122 hang" is **not a layout loop, deadlock or exponential algorithm**. It is a native
stack overflow that does not abort the process:

* The stopped thread was in `EXC_BAD_ACCESS` at a stack address inside Taffy's
  `perform_final_layout_on_in_flow_children` (lldb), i.e. a guard-page fault.
* Time is flat, not exponential: 0.5 s at every depth that completes, then never finishes.
* The cliff moves with stack size and call context. With a 2 MiB thread (Rust test default) a
  script-initiated first flush fails at 122 and a paint-first flush at between 128 and 256; with
  the 8 MiB `wega-dom` thread (`engines/wega/src/main.rs`) every depth up to 520 completes and
  600 fails; with a 64 MiB thread 256 completes in all modes.
* The blocking operation is Taffy block layout recursing through Blitz
  `LayoutPassState::compute_child_layout_internal` once per nesting level. Stylo and the HTML
  parser are not involved in the overflow (parse completes, style is not the faulting frame).
  Why overflow spins instead of aborting is not established (the platform handler re-faults);
  the report records this as unexplained and platform-dependent. Linux and Windows were not
  tested and may abort instead; either way the tab's engine is lost.

Earlier notes that the adapter alone "does not reproduce" the hang are explained by frame sizes:
the adapter path uses less stack per level than the full engine path.

### Fix (narrow, Wega-owned, no vendored change)

* `wega_dom::layout::MAX_LAYOUT_DEPTH = 256` (tree depth from the document root; `html` and
  `body` take two levels).
* `BlitzDocument::flush_layout` runs an iterative, early-exit depth check (skipped when the tree
  has no more nodes than the limit) and does nothing for deeper documents; the page keeps no
  layout (blank) but the engine stays alive.
* `GeometryAccess::layout_refused()` lets diagnostics distinguish "refused for ever" from
  "pending".
* The limit has more than 2x headroom over the measured 8 MiB cliff in a debug build (release
  frames are smaller; not measured). It is **only valid on the 8 MiB engine thread**. A
  characterisation test pins that a 2 MiB stack fails at depth 200 so the contract stays honest.

### Alternatives considered

| Option | Verdict |
|---|---|
| Bigger engine stack only | Moves the cliff, does not remove it; hostile 100k-deep input still overflows. Useful as defence in depth. |
| Cap parser/script nesting at insertion (Chrome caps the parser at 512) | Right long-term fix for the other recursive consumers and the parser's O(depth²) cost, but touches the parser, every script mutation entry and `cloneNode`: not narrow. Recommended as a separate round. |
| Patch Taffy/Blitz with a recursion guard or `stacker` | Vendored/foreign change in several algorithms (block, flex, grid, inline, paint). Not narrow. |
| Stack-pointer based guard | Needs platform `unsafe`; rejected by the unsafe inventory policy. |

### Residual risks (documented, not fixed)

* html5ever scans the open-element stack per start tag: parse cost is O(depth²) (1,000 levels
  about 0.1 s, 5,000 about 2 s, 20,000 does not finish within 15 s in a debug build).
* Script `cloneNode(true)` at 5,000 levels is stopped by the script watchdog, not a crash.
* The 2 MiB-stack contract applies to any future thread that hosts a `View`.

## 2. Inspector geometry

### Audit of what exists

| Capability | Where | Notes |
|---|---|---|
| Border box, viewport-relative | `GeometryAccess::bounding_rect` over `get_client_bounding_rect` | CSS px, includes ancestor and viewport scroll, snapped to layout units |
| Border widths, padding box size | `client_box` | |
| Content size, padding offsets | `content_box` | no margin |
| Margin, border, padding insets | Taffy `Layout` on the node (cached) | now exposed as `BoxModel` |
| Viewport size, zoom, DPR, scroll | `viewport()` | size = window px / (hidpi x zoom) |
| Hit testing | `HitTest::element_at`, viewport-relative CSS px, adds viewport scroll for Blitz | takes `&self`, no flush, no JS |
| Staleness | `layout_pending()` | unresolved, dirty descendants, damage, pending critical resources |

### Wega-owned types (implemented in `wega-dom`, independent of Blitz, Stylo, Skia)

* `Insets { top, right, bottom, left }`, `BoxModel { border_box, border, padding, margin }` with
  `padding_box()`, `content_box()`, `margin_box()` derived arithmetically.
* `Geometry`: `Current(BoxModel)`, `Stale(BoxModel)`, `Pending`, `NoBoxes`, `Refused`, `Invalid`.
* `GeometryAccess::inspect_geometry(key) -> Geometry`: reads cached layout only, never flushes.

### Rules

1. DevTools never calls `flush_layout`. Only the page's own frame loop and script (already
   guarded) flush. A `Pending` or `Refused` answer is shown as such, never retried by flushing.
2. `Stale` geometry may be drawn only with a visible "stale" treatment and is dropped when the
   mutation epoch moves again.
3. All coordinates leaving the engine are viewport CSS pixels. The browser multiplies by its own
   page zoom and egui points-to-pixels factor; the engine reports no device pixels (measured:
   CSS geometry is identical for DPR 1 and 2 and for zoom 1 and 1.5; only the viewport size in
   CSS px changes).
4. Known inaccuracy, measured: **transforms are not applied** to boxes (a `translate(30px,40px)`
   box still reports x=0) and **clips are not applied** (a 200 px child of a 50 px
   `overflow:hidden` parent reports 200). The overlay must intersect ancestor clips itself and
   must not claim accuracy under transforms (see section 3).
5. Per-read cost is microseconds (1,000 reads on a 5,000-paragraph page: 1.4 ms), independent of
   document size, so the budget is the protocol round trip, not the read.

## 3. Element picking and highlighting (design only)

### Input interception

All page pointer input is forwarded in one place: the egui loop in `src/wega_ui.rs`, which turns
`egui::Event::{PointerMoved, PointerButton, MouseWheel, PointerGone}` into `page.pointer(...)`
(`Command::Pointer`). The page never sees raw OS input. Inspect mode adds a browser-owned check
before that loop:

* If this tab's DevTools session has `picker = Active`, pointer events inside the page rect are
  **consumed** and never forwarded (no `mousemove`, `mousedown`, `click`, hover state or wheel
  reaches page script, so a page cannot observe or cancel the pick). Escape, a click, or leaving
  the mode restores forwarding.
* Wheel scrolling is the one input that is still forwarded so the user can scroll to the target;
  documented as the only page-visible side effect, and it is user-initiated.
* Keyboard focus stays with the browser UI while the mode is active.

### Hit testing without page JavaScript

A new inspector command `PickNode { x, y }` (viewport CSS px, from `local(position)` in the UI)
runs `HitTest::element_at` in the engine against cached layout. No script runs, no flush happens,
the reply is an `InspectorNodeId` bound to the existing target/session/document/epoch scope
(the D3-A mechanism) and `Geometry`. Replies are rate-limited by the existing single-outstanding-read
rule: the UI keeps at most one `PickNode` in flight and coalesces moves (hover frequency
decoupled from protocol frequency). Pending/Refused/Invalid layout replies with the state, and the
UI shows "layout not available" instead of a highlight.

### Overlay

A browser-owned egui painter layer on top of the page rect, drawn in the same pass as page
corners. It never touches the page DOM, so it cannot be observed, styled or spoofed by the page
and needs no injected script. Drawing rules:

* Convert each box with `screen = rect.min + css * page.zoom`; egui handles points to pixels, so
  high-DPI needs no extra factor (the engine reports CSS px, verified).
* Draw margin, border, padding and content as four translucent bands, plus a tag with the
  element name and size. Clip the drawing to the page rect and (best effort) to the intersection
  of ancestor `overflow` boxes obtained from bounded computed reads; if ancestor clipping cannot
  be established the overlay says "approximate".
* Stacking: egui draws it above the page texture; it does not participate in page hit testing.
* Transforms: if the node or any ancestor has a non-`none` transform (needs a bounded computed
  `transform` read, not in the current 23 longhands) the overlay is suppressed with a reason
  rather than drawn in the wrong place. This is the main condition on the highlight go decision.
* Fixed/sticky and scrolled content: viewport-relative boxes already include scroll; each epoch
  or scroll change invalidates the cached box and requests one new read.

### Lifecycle

| Event | Behaviour |
|---|---|
| Escape, picker toggle, successful pick | Mode ends, overlay cleared, pointer forwarding restored |
| Tab switch | Mode and overlay are per tab and are cleared on deactivation; forwarding restored for the page |
| Navigation / document change | Session scope changes: pending pick dropped, overlay cleared, mode ends (existing stale-id behaviour) |
| Engine process death or worker termination | Overlay cleared, mode ends, "target closed" shown; no retry |
| DevTools closed or detached window closed | Mode ends; a detached window owns the toggle but the overlay is drawn over the *tab's* page rect, so closing the window clears it |
| Layout refused or pending for long | Mode stays but reports unavailable; no flush |

## 4. DOM/CSS editing authorization (design only)

### Current facts

* The evaluation grant (`GrantToken`, `InspectorArgs::Grant`) is minted by a trusted browser
  action, bound to session, document and committed principal, compared in constant time, and
  cleared on every boundary. `Evaluate` is the only command that needs it.
* Page script and native edits would share one `Dom` (`engines/wega/src/dom.rs`) and one
  `BlitzDocument`; `View::reserve_dom_budget(nodes, text_bytes, depth)` already charges the very
  counters script ops charge (node and text budgets), with native depth capped at 121.
* `MutationClock` epoch = adapter mutation counter + Blitz stylesheet generation; D3-B closed the
  in-sheet CSSOM gap, so inline, attribute and sheet changes all advance it.

### Separate capability

Introduce `GrantKind { Evaluate, Mutate }`. Mutation is a **distinct token and distinct user
action**: granting evaluation never grants mutation and vice versa (a protocol test per
direction). Both stay bound to `(session, tab/target, process, document, principal)` and are
cleared on navigation, detach, process death, tab close and DevTools disconnect. The grant
prompt states "allow this DevTools session to change this page" and is shown by the browser UI
only; a page cannot trigger it. Policy kill switch disables Mutate independently.

### Operations

| Operation | Validation | Budget |
|---|---|---|
| `SetAttribute(node, name, value)` | node live and in the scope epoch; name matches the HTML attribute-name grammar; deny `on*` handler attributes unless the evaluation grant is also live; length cap | text bytes |
| `RemoveAttribute` | as above | none |
| `SetText(node, text)` | text node or element with only a text child; length cap | text bytes |
| `SetInlineStyle(node, property, value)` / remove | property in the supported longhand list; value parsed by the existing bounded declaration writer; length cap | text bytes |
| `RemoveNode(node)` | not the document, `html`, `head` or `body`; node live | none (frees budget) |

Every operation: check the grant, then the scope (document and epoch; edits on a stale epoch are
refused with `Stale` rather than applied to a guessed node), then `reserve_dom_budget`, then
apply through the same mutation contract script uses, then return the new epoch. Failures have
no side effects (reservation precedes mutation; tested for the budget today).
Stylesheet-rule editing and matched-rule based editing are **out of scope**: matched rules are
unsupported (D3-B), so no operation targets a rule.

### Undo

A bounded per-session undo stack (last 50 operations, 64 KiB) of inverse operations, applied only
while the epoch equals the epoch the edit produced; any page-originated mutation invalidates the
stack because the inverse could clobber page state. Undo needs the Mutate grant. The stack is
cleared on navigation and detach.

### Cleanup

Navigation, detach, process death and tab close revoke the Mutate grant, drop the undo stack and
pending edits; nothing persists across documents. Edits are never written back to the network or
disk.

### Same authoritative state

Native edits go through the same `Dom` and `BlitzDocument` as script, and charge the same
budgets, so a script that reads after an edit observes it and vice versa; the epoch advances on
both. A test must assert a page `MutationObserver`-free read sees a native edit and that a
script edit during a pending native edit makes the native edit return `Stale`.

## 5. Proposed D3-C phases

| Phase | Content | Depends on |
|---|---|---|
| C.1 | Protocol: `GetGeometry` (cached `Geometry`, bounded, scoped, no flush); Elements panel shows box model diagram and freshness | this round |
| C.2 | Picker: `PickNode`, UI interception, mode lifecycle, no overlay | C.1 |
| C.3 | Overlay: egui layer, ancestor clip intersection, transform detection (bounded computed `transform` read) | C.1, C.2 |
| C.4 | Mutate capability, attribute/text/remove/inline-style edits, undo | separate security review, C.1 |
| C.5 | Hardening: parser depth cap, release-stack measurement on Linux and Windows, console benchmark gate | independent |

## 6. Security model summary

* No flush, no page JavaScript, no DOM injection for geometry, picking or highlighting.
* Page input is consumed in the browser before reaching the page while picking; the overlay is
  browser-owned.
* Evaluate and Mutate are separate, session/target/process/document-bound, revocable grants.
* Every response is bounded and bound to the scope epoch; stale references are refused.
* Pathological documents (depth beyond the limit) degrade to "geometry unavailable", never to a
  hang.
