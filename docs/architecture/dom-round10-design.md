# Round 10 design: layout and geometry ownership (audit and decision)

Scope: Wega-owned geometry, scroll, viewport and hit-test interfaces over the existing Blitz layout; no layout algorithm, no second layout tree, no second dirty state. Branch `feature/dom-rounds-9-12` (the branch named in the task text, `feature/browser-cache-accessibility`, was merged into `main` by PR #63). Local only.

## 0. Baseline

- HEAD `a538bec` (Round 9 report), working tree clean at the start, 7.2 GiB free (disk is nearly full: release benchmark builds need care), load average 2–12 over the session.
- Engines workspace: 13 members, 24 internal edges, 0 checker violations. Round 9 full suite: 462 passed, 0 failed, 8 ignored (re-run after this round's changes in the report).
- Direct Blitz-reference lines in engine `src/` (the Rounds 6–9 grep): 93 (`dom.rs` 55, `view.rs` 31, `document_host.rs` 5, `network.rs` 1, `script.rs` 1).
- Platform: macOS (Apple M4) only.

## 1. Who owns layout and geometry today

"Blitz" is the vendored, patched `engines/vendor/blitz/blitz-dom`; layout itself is Taffy (block/flex/grid) plus Parley for inline text, both driven by Blitz. Engine paths are under `engines/wega/src`.

| # | Operation | Owner | Where | State | Tests before | Gaps found (probe at `a538bec`) |
|---|---|---|---|---|---|---|
| 1 | Layout tree construction | Blitz (`layout/construct.rs`, damage-driven) | `resolve.rs` | `Node::layout_data`, inline layout in `ElementData` | reftests (`css.rs`) | — |
| 2 | Style → layout | Stylo cascade, then Blitz construction/damage, then Taffy (`resolve.rs`: device changes, `resolve_stylist`, `propagate_damage_flags`, construct, `resolve_layout`) | `BaseDocument::resolve(time)` | none outside Blitz | reftests | `resolve` returns early while render-blocking stylesheets load |
| 3 | Invalidation | Blitz: per-mutation restyle hints and damage flags (`mutator.rs`). Engine: one `layout_dirty` flag (`dom.rs:84`) set by `invalidate()` (every mutating host op, and `begin_task`, i.e. each script task) and cleared by `Dom::resolve` | the engine flag only decides *whether to call* `doc.resolve` before a geometry read; Blitz decides what is recomputed | two flags with different jobs: the engine's is a "has anything changed since I last flushed" gate, Blitz's are the real damage | indirect | `Dom::resolve` clears its flag before calling Blitz; if Blitz returns early (pending stylesheets), reads see stale layout until the next paint-time resolve |
| 4 | Scheduling | Engine: flush before geometry reads (`dom.rs`), before hit-testing and pointer input (`view.rs:1557,1715`), and twice per paint (`view.rs:2329,2345`, around the script frame callbacks) | `View::record_paint` | — | `pages.rs` | — |
| 5 | `getBoundingClientRect`, `getClientRects` | Wega JS → host ops `rect`, `clientRects` → Blitz `get_client_bounding_rect`, `node_client_rects` (CSS px, snapped to 1/64) | `semantics.js:1518`, `dom.rs:1354-1365` | none in Wega | `dom.rs` (few) | `getClientRects` returned one zero rect for `display:none`, descendants of it and detached nodes (spec: empty list); `display:none` rect kept a non-zero position; **a scrolled element's own rect was shifted by its own scroll offset** (Blitz `physical_unrounded_geometry` subtracts the node's own `scroll_offset`) |
| 6 | `offset*`, `client*`, `scroll*` sizes | Wega JS → one host op `box` returning 15 fields for **every** getter → Blitz `offset_rect`, `final_layout`, `client_width`, `scroll_width`, `offset_parent` | `semantics.js:1524-1556`, `dom.rs:1350-1353` | none | few | every `clientWidth` read also computed `offset_rect` and `offset_parent`; a stale node panicked (`unwrap`) |
| 7 | `scrollLeft/Top` read and write, `scrollTo`, `scrollIntoView`, fragment scroll | host ops `scroll`, `scrollNode`, `scrollIntoView`; Blitz scrolling (`scrolling.rs`) | `observers.js:420-455`, `dom.rs:1182-1185,1548` | element scroll offsets in `Node`, viewport scroll in `NodeTree` | none dedicated | — |
| 8 | Viewport size, DPR, scrolling | Embedder → `View::paint(width, height, scale, zoom, dark)` → Blitz `set_viewport` on every paint; script reads host op `viewport` | `view.rs:2313`, `dom.rs:1181` | `Viewport` in the Blitz document | none dedicated | `innerWidth` has a 1920 fallback in JS when the op is unavailable; no `resize` event on viewport change |
| 9 | Hit-testing | Blitz `hit` (document coordinates, honours `pointer-events`, `visibility`, transforms) | hover/click: `InputControl::pointer_moved` (Round 8) → `set_hover_to`, engine adds the scroll offset (`page_point`); wheel: `View::scroll_at` calls `hit` directly with **client** coordinates | `hover_node`, `last_client_pointer_position` in Blitz | `input.rs`, `events.rs` | `scroll_at` hit-tests at the wrong document point when the viewport is scrolled (client vs document coordinates); no `document.elementFromPoint` |
| 10 | Text geometry, selection rectangles | Blitz/Parley (selection, caret, inline layout) | `view.rs` input paths | Blitz | `input.rs` | no script API exposes text rectangles (no `Range.getBoundingClientRect`) — nothing to migrate |
| 11 | Transforms, coordinate spaces | Blitz: hit-testing handles transforms, painting applies them; **geometry reads ignore transforms** (`getBoundingClientRect` of a `translate/scale` element is its untransformed box) | `document.rs:2290-2330` | — | none | transformed rects: recorded gap, not fixed |
| 12 | DPR, document replacement, resize | DPR = `hidpi_scale * zoom` (Blitz viewport); replacement builds a new `BlitzDocument` (`view.rs:set_document`), the new document starts with Blitz's default viewport until the next paint | `view.rs` | — | `documents.rs` | geometry before the first paint on a replaced document uses the default viewport |

So: Blitz owns the layout tree, the layout calculation and all geometry; the engine owns the JS surface, the flush schedule and the `layout_dirty` gate, and reaches into Blitz (`get_node`, `final_layout`, `client_width`, `offset_rect`, `node_client_rects`, `get_client_bounding_rect`, `viewport`, `viewport_scroll`, `scroll_*`, `hit`, `set_viewport`) from 22 call sites.

## 2. Decision

### Contract: `wega_dom::layout`, implemented by `BlitzDocument` (no new crate)

Four small traits, shaped by the consumers above and nothing else:

- `GeometryAccess: DocumentView` (reads, `&self`, never relayout): `has_boxes`, `bounding_rect`, `client_rects(key, &mut Vec<Rect>)`, `client_box`, `offset_box`, `scroll_size`, `scroll_offset`, `content_box`, `viewport`.
- `HitTest: GeometryAccess`: `element_at(viewport_point)`.
- `LayoutControl: GeometryAccess`: `flush_layout(time)`, `set_viewport(ViewportConfig)`, `set_viewport_scroll`, `scroll_node_to`, `scroll_node_into_view`, `scroll_to_fragment`.
- Types: `Point`, `Size`, `Rect` (all `f64` CSS px), `ViewportGeometry`, `ViewportConfig`, `ClientBox`, `OffsetBox`, `ContentBox`, `LayoutError::Stale`.

Rules: `NodeKey` only; stale keys rejected with `LayoutError::Stale` instead of `unwrap()`; no Blitz/Taffy type in any signature; fractional values preserved; no cache, no copy of the layout tree. Separate small queries replace the single 15-field `box` op so `clientWidth` no longer computes `offset_rect`.

**Coordinate spaces** (documented in the module): CSS pixels throughout; client (viewport-relative) for rects and hit-testing; document-relative = client + `ViewportGeometry::scroll`; offset-parent-relative for `OffsetBox`; device pixels are never exposed (DPR is reported for the renderer). The hover contract from Round 8 changes from page to **client** coordinates so no caller adds the scroll offset; the adapter converts.

**Flush semantics:** reads never recalculate. The single flush entry point is `LayoutControl::flush_layout`; the engine's existing `layout_dirty` gate stays the only "needs flush" authority (Blitz's damage flags remain the authority on what to recompute). No new dirty cache.

### Crate placement

- **No `wega-layout` crate.** The consumers are the engine and the Blitz adapter, exactly `wega-dom`'s two dependents; the contract needs `NodeKey` and `DocumentView` from `wega-dom`. A new crate would add a manifest and two edges and isolate nothing. `engines/layout/*` stay documentation-only placeholders.
- No new external dependency: the adapter already has `blitz-dom` and `blitz-traits`.

### Out of scope / not added (no consumer)

`LayoutInvalidation` (Blitz invalidates inside each mutation; the engine's gate is not a contract), text/selection rectangles (no script API), transform-aware geometry (a Blitz feature), `elementFromPoint` (new web API, not a boundary extraction), `scroll_node_by` (it takes Blitz's event callback; stays a Blitz call in `View`, listed as remaining), `Viewport` for painting (rendering, Round 11).

### Pre-existing bugs fixed on the way (each tested)

1. `getClientRects()` returned a zero rectangle for boxless nodes (spec: empty list); boxless includes descendants of `display:none` and detached nodes. `getBoundingClientRect()` of a boxless element is all zero (was `0,y,0,0`).
2. A scrolled element's own bounding rect moved by its own scroll offset (vendored Blitz patch `own-scroll-bounding-rect.patch`).
3. `View::scroll_at` hit-tested at client coordinates in a document-coordinate API (wrong target with a scrolled viewport).
4. A stale node in a geometry host op panicked; now a script error.

Recorded, not fixed: transformed geometry, `resize` events, no `elementFromPoint`, flush skipped while stylesheets load, the 1920×1080 JS fallback.

## 3. Risks

- `client_rects` for boxless nodes changes observable output (fix 1); no existing test pins the old form (verified by the full suite).
- `pointer_moved` changes coordinate space; the single engine caller and the adapter test are updated together.
