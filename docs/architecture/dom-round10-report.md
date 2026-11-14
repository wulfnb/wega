# Round 10 report: layout and geometry ownership

Scope: Wega-owned geometry, scroll, viewport and hit-test interfaces over the existing Blitz layout (Taffy and Parley underneath). No layout algorithm, no second layout tree, no second dirty state, no rendering extraction. Local only, not pushed. Audit and design were committed first: [`dom-round10-design.md`](./dom-round10-design.md) (the 12-operation ownership table is there and not repeated). Branch `feature/dom-rounds-9-12` (the branch named in the task text was already merged into `main`). The Round 9 report was read and the baseline re-established at `a538bec`.

**Outcome:** every geometry, scroll-offset, viewport and hit-test call the engine made into Blitz now goes through `wega_dom::layout`, implemented once by `BlitzDocument`. Four pre-existing bugs fixed with tests (one in vendored Blitz). Five more differences from the old behaviour that tests exposed are listed in §6. No new crate, no new unsafe, no dependency or lockfile change, no JavaScript API change.

## 1. Layout ownership: before and after

Before: Blitz owned the layout tree, calculation and every geometry value; the engine owned the JS surface, the flush schedule and an engine-side `layout_dirty` gate, and reached into Blitz from 22 sites (`get_node`, `final_layout`, `client_width`, `offset_rect`, `node_client_rects`, `get_client_bounding_rect`, `viewport`, `viewport_scroll`, `scroll_*`, `hit`, `set_viewport`, `resolve`). One host op (`box`) computed 15 fields for every `offsetWidth`/`clientWidth`/`scrollHeight` read.

After: same owners and same state. The engine calls `GeometryAccess`, `HitTest` and `LayoutControl`; the JS getters call small host ops (`clientBox`, `offsetBox`, `scrollSize`) instead of the 15-field one (kept only for ResizeObserver, now also behind the contract).

## 2. Contracts and coordinate definitions

In `wega-dom` (`engines/dom/core/src/layout.rs`), no new crate: the consumers are the engine and the Blitz adapter, `wega-dom`'s only two dependents, and the contract needs `NodeKey`/`DocumentView` (a `wega-layout` crate would add a manifest and two edges and isolate nothing). `engines/layout/*` stay documentation placeholders.

- `GeometryAccess` (reads, `&self`): `has_boxes`, `bounding_rect`, `client_rects(key, &mut Vec<Rect>)`, `client_box`, `offset_box`, `scroll_size`, `scroll_offset`, `content_box`, `viewport`.
- `HitTest`: `element_at(Point)`.
- `LayoutControl`: `flush_layout(time)`, `set_viewport(ViewportConfig)`, `set_viewport_scroll`, `scroll_node_to`, `scroll_node_into_view`, `scroll_to_fragment`.
- Types: `Point`, `Size`, `Rect` (`f64`), `ViewportGeometry`, `ViewportConfig`, `ClientBox`, `OffsetBox`, `ContentBox`, `LayoutError::Stale`.

**Coordinate spaces.** Everything is CSS pixels, never device pixels (DPR, with zoom, is reported only so a renderer can convert). *Viewport-relative (client)*: rects from `bounding_rect`/`client_rects`, and the `HitTest`/`pointer_moved` inputs. *Document-relative*: client + `ViewportGeometry::scroll`; the adapter converts, callers never add the scroll. *Offset-parent-relative*: `OffsetBox::rect`. *Element-relative*: `ClientBox`, `ContentBox`. Fractions are preserved (Blitz snaps rects to 1/64 px); the integer CSSOM getters round in JS, as before.

**Flush semantics.** Reads never relayout; they report the last flush. `LayoutControl::flush_layout` is the single recalculation entry point. No cache, no copy of layout. `NodeKey` only, stale keys give `LayoutError::Stale` (was `unwrap()` → panic). No Blitz or Taffy type in any signature.

## 3. Migrated operations

`getBoundingClientRect`, `getClientRects`, `client{Left,Top,Width,Height}`, `offset{Left,Top,Width,Height,Parent}`, `scroll{Width,Height}`, element `scrollLeft/Top` read and write, `scrollTo`/`scrollIntoView`, window `scrollX/Y`, `scrollTo`, `innerWidth/innerHeight/devicePixelRatio`, fragment navigation scroll, ResizeObserver boxes, layout flush (engine and `View`), viewport configuration on paint, the iframe-visibility size check, wheel hit-testing, hover coordinate conversion.

Deliberately not migrated: `scroll_node_by` (two sites in `view.rs`; it takes Blitz's event callback and is part of Blitz's wheel/scroll-chaining default action, like Round 8's `handle_dom_event`), `primary_styles().clone_visibility()` in the iframe check (a style read), the paint-side `Viewport` type (rendering, Round 11), text/selection rectangles (no script API exists), `LayoutInvalidation` (no consumer, see §5).

## 4. Hit-testing integration

- `InputControl::pointer_moved` (Round 8) now takes **viewport** coordinates; the adapter adds the scroll offset before Blitz's `set_hover_to`. The engine's `page_point` is kept only for event `pageX/pageY`. Hover, click synthesis, default actions and focus code are untouched; target selection is still Blitz's hover result, so there is no second hit path and no second event dispatcher.
- `View::scroll_at` (wheel under the pointer) used Blitz's raw `hit` with client coordinates although that API takes document coordinates, so it picked the wrong element with a scrolled viewport. It now uses `HitTest::element_at` (**fix 3**). Tests: clicks, hover and wheel targeting with `scrollY = 250`.
- Shadow DOM retargeting is unaffected (events are dispatched by deno_web from the target the document reports); a shadow tree has no layout, so nothing inside it is ever a hit target (Round 9 gap, pinned again below).
- `document.elementFromPoint` does not exist in Wega; adding it is a web API, not a boundary extraction. Not added. The contract method it would use is `element_at`.

## 5. Layout invalidation (audit result)

Authority is unchanged. Blitz's per-mutation restyle hints and damage flags decide *what* is recomputed (DOM insertion/removal, attributes, class/style, stylesheet and text changes, form state, viewport change via queued device changes). The engine keeps one gate, `layout_dirty`: set by every mutating host op and at the start of each script task, cleared when the engine flushes before a read. Scrolling does not dirty layout (Blitz bumps a geometry generation for paint). `View` flushes before hit-testing/pointer input and twice per paint. No second dirty cache was added; none was needed (tests verify reads after style, class, stylesheet, insertion and removal changes, and repeated reads, see §7).

Existing gap, not fixed: `Dom::resolve` clears its gate before Blitz's `resolve`, which returns early while render-blocking stylesheets load; reads in that window report the old layout until the next paint-time flush. No `LayoutInvalidation` trait was introduced.

## 6. Bugs fixed and gaps (pre-existing, separated from migration)

Fixed, each tested:

| # | Bug at `a538bec` | Fix |
|---|---|---|
| 1 | `getClientRects()` returned one zero rect for `display:none`, descendants of `display:none` and detached nodes (spec: empty list); `getBoundingClientRect()` of `display:none` kept its y position | boxless nodes: empty list, all-zero rect |
| 2 | A scrolled element's own `getBoundingClientRect()` moved by its own `scrollLeft/Top` (Blitz subtracted the node's own scroll offset) | vendored patch `own-scroll-bounding-rect.patch` (listed in `vendor/README.md`, `sources.json`) |
| 3 | `scroll_at` hit-tested at client coordinates with a document-coordinate API | `HitTest::element_at` |
| 4 | A stale node in a geometry host op panicked | `LayoutError::Stale`, a script error |
| 5 | `offsetWidth/clientWidth/scrollHeight/offsetParent` of a removed element still returned its last layout; `offsetParent` of a `display:none` element returned `body` | all boxless nodes report zero and no offset parent (the same `has_boxes` rule as fix 1) |

Recorded, not fixed (Blitz features): geometry ignores transforms (`translate/scale` rects are the untransformed box, pinned); no geometry in shadow trees (Round 9); no `resize` event on viewport change; no `elementFromPoint`; `Range` rects do not exist; the JS `innerWidth` 1920×1080 fallback; flush skipped while stylesheets load (§5).

## 7. Tests and validation

Full engines workspace: **485 passed, 0 failed, 9 ignored** (Round 9: 462/0/8): +23 tests (`wega-adapter-blitz/tests/layout.rs` 8, `wega-engine/tests/layout.rs` 15) and one ignored benchmark. Coverage: nested boxes with border/padding/margin, inline line boxes, fractions, overflow and element scrolling (clamping), viewport scrolling, viewport resize / device scale / zoom, hidden/detached/boxless nodes, transforms (gap), dynamic DOM and CSS mutation with no stale reads, repeated reads, Shadow DOM, document replacement, ResizeObserver, clicks/hover/wheel with a scrolled viewport, stale keys, and the contract's no-relayout rule. The values in the engine tests were first recorded from the pre-migration code (parity baseline); the intentional differences are exactly fixes 1, 3 and 5.

Broker mode (`WEGA_TEST_BROKER=1`, `--test-threads=1`): security 9, forms 4, scripts 8, documents 5, history 3, events 20, input 8, style 16, layout 15, lib 27, dom 30, dom_identity 4, pages 16: all pass. Required-worker test (content helper from `wega-content-bundle`): passes. Also: `cargo fmt --check`; clippy with `-D warnings` (engines and tools workspaces); root `cargo check --locked`; `cargo metadata --locked` for root, engines and tools; `scripts/check-engine-boundary.py`; architecture checker (0 violations, 14 informational) and its tests; `wega-dom` and `wega-adapter-blitz` boundary tests.

## 8. Performance (Round 9 `a538bec` vs Round 10 `9f62df7`)

Release test binaries for both commits built from the same benchmark file (the Round 10 `hot_path_bench.rs`, which uses only script and `View` APIs), run interleaved r9,r10,r9,r10,r9,r10 ×3, median of 3 process runs (each a median of 7). Machine load average 10 → 3 over the runs (10 cores, other processes present); ranges are shown in the raw output and were ≤ 5% for every path below. Allocations not measured.

| Path | Round 9 | Round 10 | Δ |
|---|---|---|---|
| `getBoundingClientRect`, no mutation | 3.23 µs | 3.27 | +1% |
| `getClientRects`, no mutation | 2.76 | 2.80 | +1% |
| `offsetWidth+offsetTop`, no mutation (2 reads) | 3.94 | 1.58 | −60% |
| `clientWidth+clientHeight` (2 reads) | 4.61 | 2.04 | −56% |
| `scrollHeight+scrollTop` (2 reads) | 2.69 | 1.16 | −57% |
| rect read after a style write (includes the relayout) | 314.9 | 312.0 | −1% |
| 300 style writes, then 300 `offsetHeight` reads | 11.30 | 8.99 | −20% |
| class toggle + `offsetWidth` | 496.3 | 492.2 | −1% |
| element `scrollTop` write+read | 2.26 | 2.23 | −1% |
| viewport `scrollTo`+`scrollY` | 8.39 | 8.53 | +2% |
| viewport resize + layout + paint | 11.88 ms | 11.96 | +1% |
| wheel `scroll_at` (hit-test + scroll) | 65.8 | 65.9 | 0% |
| document layout (300 cells) + paint | 76.97 ms | 77.35 | 0% |

The 56–60% gain comes only from splitting the 15-field `box` op so `clientWidth`/`offsetWidth`/`scrollHeight` no longer compute the offset rect and offset parent; it is a measured release-build difference at stable load, not an effect of the contract itself. Everything else is within noise (the Round 9 style paths and the older DOM paths were re-measured in the same runs: all within ±5%; `deep clone` +5% is within its run-to-run range). The batched read test shows no layout thrashing: reads after N writes cost one relayout, and repeated reads without mutation cost no relayout.

## 9. Dependency graph, references, lockfile

- Engines workspace: 13 members, 24 internal edges, no cycles, 0 violations (unchanged). No dependency added to any crate.
- All three lockfiles unchanged. No new `unsafe` in the diff (grep over non-vendor changes: 0). `wega-dom` still names no Blitz type (boundary test).
- Direct Blitz references, same grep as Rounds 6–9: **93 → 83** (`dom.rs` 55 → 46, `view.rs` 31 → 30, `document_host.rs` 5, `network.rs` 1, `script.rs` 1). Remaining direct layout calls: the two `scroll_node_by` sites.
- Generated bindings: none; `dom/semantics.js` is hand-written, its public API unchanged (only host-op names changed internally).

## 10. Not executed, blockers, commits

- Only macOS (Apple M4) was run. Linux and Windows builds and the native-sandbox tests were not executed; nothing in this change touches them.
- The vendored Blitz patch must be re-applied on a Blitz update.
- Disk was at 99% (5.7 GB free) after release builds; only this project's `engines/target` grew.
- Commits: `fe90755` audit and design, `9f62df7` implementation, tests and benchmark, plus the report commit.

Next (Round 11, not started): rendering extraction (paint scene, raster, the paint-side `Viewport`), then form state.
