# Round 11 report: rendering and painting ownership

Scope: Wega-owned orchestration of the paint path through narrow interfaces, keeping Blitz's painter and the Skia backend. No new painter, no display-list abstraction, no second render tree. Local only, not pushed. Audit and design were committed first: [`dom-round11-design.md`](./dom-round11-design.md) (the stage-by-stage ownership table is there and not repeated). Branch `feature/dom-rounds-9-12` (the branch named in the task text was already merged into `main`). Baseline re-established at `fb48f4a` after reading the Round 10 report.

**Outcome:** recording a frame now goes through `wega_dom::render::PaintSource`, implemented by `BlitzDocument`; the engine no longer depends on `blitz-paint` or `anyrender`. Frame submission uses typed `FrameRequest`/`FrameTarget` instead of positional tuples, dirty tracking is a named `FrameState`, and the paint API takes the Wega-owned `ViewportConfig` instead of Blitz's `Viewport`. Pixels are bit-identical to Round 10 on a 42-frame corpus. No new crate, no new unsafe, no external dependency change. One pre-existing frame-lifecycle problem (idle pages repaint continuously) was found, measured and deliberately not fixed.

## 1. Pipeline: before and after

```
DOM → style → layout          (Rounds 2–10 contracts over Blitz/Stylo/Taffy)
  → prepare_frame             viewport + paint-time style-threading hint + layout flush      [PaintSource]
  → script frame callbacks    (rAF; may change the document) → flush again                   [engine]
  → record_scene              blitz_paint writes an owned display list (anyrender::Scene)    [PaintSource → Blitz]
  → rasterise                 Skia on the wega-raster thread, one frame in flight            [wega-adapter-skia]
  → submit                    Painted{serial,size,Owned|Shared slot}; engine process loop    [main.rs, FrameTarget]
  → compose / window surface  browser UI process (outside this workspace, untouched)
```

Before, the engine called `blitz_paint::paint_scene` itself, named `anyrender::Scene` in its signatures, hand-set the Blitz viewport and threading mode through `Deref`, and passed `(serial, w, h, scene, Option<(u8, u64)>)` positionally. After, the same calls happen in the same order, behind the trait and typed requests. Order and behaviour of the lifecycle are unchanged (verified, §5).

## 2. Ownership and contract decisions

| Concern | Owner | Interface |
|---|---|---|
| When a frame is recorded, with which viewport, validation, frame callbacks, dirty state, where the recording goes | **Wega** (engine `View`, `FrameState`) | `begin_paint(ViewportConfig, serial, FrameTarget)` |
| Painting semantics: backgrounds, borders, text, images, gradients, filters, clips, transforms, opacity, scroll offsets, scrollbars | **Blitz** (`blitz_paint`, Parley) | `PaintSource::record_scene` (adapter owns the call) |
| Display-list format | `anyrender::Scene`, backend-neutral third-party crate shared by painter and rasteriser | associated type `PaintSource::Scene`; no Wega-defined display list |
| Rasterisation | **Skia** (`anyrender_skia`) | `wega-adapter-skia::Raster` + `FrameRequest`/`FrameTarget`/`Painted`/`Pixels` |
| Shared-slot transport | `wega-platform::FrameStore` | `FrameTarget::Shared { slot, sequence }` |
| Window surface, compositing | browser UI process | not in this workspace |

- **`PaintSource`** (in `wega-dom`, no new crate: consumers are the engine and the Blitz adapter, as for every Round 8–10 contract): `prepare_frame`, `record_scene`, `is_animating`; documented coordinate convention. It names no painter, display-list or graphics type.
- **Render viewport = `ViewportConfig`** (Round 10): device-pixel size, `scale` device px per CSS px, `zoom` multiplier, painter scale `scale * zoom`, `dark`. No second viewport type.
- **No `Rasterizer` trait, no display-list abstraction, no `wega-render` crate.** One backend and one consumer; a generic `View` would add churn without a migration path. The Skia adapter needed no expansion beyond typed requests; its surface (`start`, `take`, `wait`, `release`, `Painted`, `Pixels`) already is the raster contract. Replacement strategy in §10.
- **Not added (no consumer):** damage rectangles, scene reuse (needs a return channel to the raster thread), `RenderViewport`/`PaintContext`/`SurfaceTarget` as separate types.

## 3. Migrated paint path

- Frame initiation: `View::begin_paint(ViewportConfig, serial, FrameTarget)` (was Blitz `Viewport` + `Option<(u8, u64)>`); its only in-repo caller, `engines/wega/src/main.rs`, updated. **API change:** the signature is different; `wega_engine::{Viewport, ColorScheme}` stay re-exported for compatibility but the paint API no longer takes them; `wega_engine` newly exports `ViewportConfig`, `FrameRequest`, `FrameTarget`.
- Viewport/scale configuration and layout flush: `PaintSource::prepare_frame` (viewport, threading hint — applied only at paint, as before — flush).
- Paint invocation: `PaintSource::record_scene` (moved `blitz_paint::paint_scene` and the scene allocation into `wega-adapter-blitz`); `is_animating` through the trait.
- Surface submission: `Raster::start(FrameRequest, Scene)`; `FrameTarget::{Owned, Shared}`; behaviour when a shared target has no store (falls back to owned pixels) preserved.
- Invalidation signalling: `FrameState` (`invalidate`, `recorded`, `needs_paint`) replaces the bare `dirty: bool` and its 30 writers; unit-tested.
- Resize: unchanged mechanism (new size on the next paint; the raster thread recreates its renderer on a size change), now covered by tests and measured (§6).

Not migrated: Canvas 2D/WebGL (direct `skia-safe`, documented temporary exception, not part of the painter path), media surfaces, the accessibility tree (recorded in `record_paint` but not a rendering concern), the window surface.

## 4. Frame lifecycle (review)

Verified by tests: a new view needs a frame; a paint settles it; mutation, `request_redraw`, document replacement and thawing request one; a frozen view never paints (and `freeze` releases the image cache and the renderer, and painting works afterwards); `begin_paint` rejects zero/oversized sizes, non-finite or non-positive scale and zoom outside 0.3–5 without starting a frame; one frame in flight (a second `begin_paint` fails until the first is collected); results carry the request's serial and size; shared-slot frames equal owned frames pixel for pixel; CSS transitions keep painting until finished and then idle (existing test).

**Findings (pre-existing, not fixed):**
1. **Idle pages repaint continuously.** After a paint `needs_paint()` is false; after one `poll()` it is true again, for a plain page, a page with a script and a page with a 60-second timer. `poll()` marks the view dirty unconditionally in its timer task, and every raster completion wakes the engine loop, which polls, so a settled page repaints at the 33 ms frame cap indefinitely. It cannot be removed locally: canvas/WebGL draws do not invalidate on their own (their host calls return before the invalidating ops), and media frames and resource loads (`handle_messages` inside `poll`) rely on the same flag. A safe fix needs those producers to invalidate explicitly first (then the timer task can invalidate only when it ran callbacks or the revision changed). Pinned by `known_gap_every_poll_marks_a_settled_page_dirty_again`. Cost measured in §6.
2. Painting records an empty scene while render-blocking stylesheets load, and nothing re-requests a frame when they arrive except finding 1's flag.
3. Surface loss/recreation, GPU/CPU fallback: not applicable inside the engine (CPU Skia only; window surface lives in the UI process). Worker panic → renderer dropped, error result, view frozen by the embedder (existing behaviour, unchanged).
4. No damage tracking: a one-pixel change repaints and rasterises the full surface.
5. A size change recreates the Skia renderer; alternating sizes is ~20× slower than repainting at one size (§6). Existing adapter behaviour.

## 5. Screenshot / reftest evidence

- **Pixel parity with Round 10:** `sha256` (first 16 hex) of the RGBA output for 7 pages (background+border, clip, opacity+transform, text with non-ASCII, gradient+radius+shadow, overflow scroller, tall scrolled page) × viewports 100×100@1×, 200×200@2×, 100×100 zoom 2, 40×60, scrolled, dark scheme = **42 frames, all identical** between `fb48f4a` and the new code (30 distinct hashes, so the corpus exercises varied content). Script run at both commits, not committed.
- **Existing reftests** (`css.rs` 21, `css_wpt.rs`, `canvas2d.rs`, `pages.rs`, `compat.rs`, `frames.rs`, `webgl.rs`): pass unchanged.
- **New `render.rs` (17 tests):** backgrounds and borders (exact edge pixels), `overflow` clipping, opacity (50% black → `0,0,0,128`) and transform (painted translated, layout not), text (glyphs painted in the colour, deterministic, hidden text paints nothing), images (an SVG data URL paints once decoded), viewport and container scrolling, high-DPI at 2× and page zoom 2, resize (reflow, size change, returning to a size reproduces its pixels), dynamic style/layout updates, document replacement, Shadow DOM (light children render; shadow content does not: known gap from Round 9), and the lifecycle tests of §4. Solid fills are asserted exactly; no platform tolerance. No GPU reference images exist; none were invented.

## 6. Performance and memory

Release test binaries for `fb48f4a` and `f082a14`, built from the same benchmark file (`hot_path_bench.rs`, which for the paint paths uses only `View::paint`), run interleaved r10,r11 ×3, median of 3 process runs (each a median of 7). Machine: Apple M4, 10 cores, 16 GiB; **load average 5.7–8.5** (not quiet). Release profile, CPU rasterisation, scenarios at 800×600@1× unless stated.

| Path | Round 10 | Round 11 | Δ |
|---|---|---|---|
| paint 800×600, unchanged page | 0.64 ms | 0.63 | −2% |
| paint 1600×1200 at scale 2 | 1.31 | 1.31 | 0% |
| paint after a one-element move | 0.73 | 0.71 | −3% |
| paint after a container scroll | 0.63 | 0.63 | 0% |
| paint alternating two sizes (resize) | 13.99 | 13.99 | 0% |
| paint 200×150 | 0.15 | 0.15 | 0% |
| document load + first paint | 63.4 | 65.7 | +4% (range 63.1–66.3, within variance) |
| viewport resize + layout + paint (Round 10 bench) | 12.29 | 11.82 | −4% |

The Round 10 layout paths and the older DOM/style paths, re-measured in the same runs, are within ±5% (`dom ops` +5% and `deep clone` +5% are inside their run-to-run range). **No regression and no improvement is claimed**: the migration moved calls behind a trait with static dispatch; there is no per-frame cost added or removed.

**Architectural overhead vs Blitz/Skia cost** (`render_bench`, current code only, 40 frames, median): recording the scene (layout flush when nothing changed + paint preparation + `blitz_paint`) **0.09 ms** at both 800×600@1× and 1600×1200@2×; rasterising and returning pixels **0.24 ms** at 800×600 and **0.60 ms** at 1600×1200@2×. The contract adds a statically dispatched call; the cost is Blitz's painter and Skia. The page used here is light (40 rows plus a clipped box); heavier pages cost more in both stages.

- **Resize is the real cost:** alternating two sizes costs ~14 ms per frame against ~0.6 ms for steady size, because the raster thread rebuilds its `SkiaImageRenderer` on every size change. Existing adapter behaviour; a candidate for a pooled or resizable renderer (future work).
- **Idle cost of finding 1:** a settled 800×600 page costs ≈ 0.6 ms of paint per frame at up to 30 frames per second (≈ 2% of one core on this machine) instead of zero; heavier pages scale linearly.
- **Memory:** resident size 55 MiB at start, 76 MiB after the page and first frames, **78 MiB after 300 further 800×600 frames** (no growth beyond +2 MiB: no per-frame leak). Peak usage and allocation counts were not measured; per frame the code allocates one `Scene` (moved to the raster thread, not copied) and, for the owned target, one pixel buffer; the shared target writes in place with no pixel copy.

## 7. Remaining Blitz painting dependencies

- `wega-adapter-blitz` now owns `blitz-paint` and `anyrender` (boundary test lists them). The engine has no `blitz_paint` or `anyrender` reference (grep over engine `src/`: 0); engine-level Blitz references (Rounds 6–10 grep): **83 → 81**. The checker's informational ARCH-007 findings for `wega-engine` dropped from 14 to 12 (no more direct `blitz-paint` and `anyrender`).
- Still Blitz-owned: all painting semantics and the display-list producer; `AccessKit` tree building (`build_accessibility_tree`); the image cache and its release; `scroll_node_by`; the re-export `pub use blitz_traits::shell::{ColorScheme, Viewport}` (kept for compatibility only).
- Still direct Skia in the engine: Canvas 2D and WebGL PNG encoding (documented exception).

## 8. Tests and platform coverage

Full engines workspace (run one test target at a time because the disk was nearly full): **506 passed, 0 failed, 11 ignored** (Round 10: 485/0/9): +21 tests (`render.rs` 17, `frame.rs` unit tests 4) and two ignored benchmarks (`render_paths`, `render_bench`). Broker mode (`WEGA_TEST_BROKER=1`): security 9, forms 4, scripts 8, documents 5, history 3, events 20, input 8, style 16, layout 15, render 17, dom 30, dom_identity 4, pages 16, lib 31: all pass. Required-worker test (content helper from `wega-content-bundle`): passes. `cargo fmt --check`; clippy `-D warnings` (engines and tools workspaces); root `cargo check --locked`; `cargo metadata --locked` for root, engines and tools; `scripts/check-engine-boundary.py`; architecture checker (13 members, 24 edges, 0 violations, 12 informational) and its tests; `wega-dom` and `wega-adapter-blitz` boundary tests.

Dependency graph unchanged (13 members, 24 internal edges, no cycles). `engines/Cargo.lock`: 2 lines added and 2 removed, all of them the `anyrender`/`blitz-paint` entries moving from `wega-engine`'s dependency list to `wega-adapter-blitz`'s; no external package added, removed or changed; root and tools lockfiles unchanged. No new `unsafe` (grep over non-vendor changes: 0). No vendored-source change this round. Not executed: Linux and Windows builds, native-sandbox tests, any GPU or windowed rendering (no GPU path exists in the engine; the UI process was not run), and the browser UI test suites outside `engines/`.

## 9. Commits

`9cd2ce5` audit and design, `f082a14` implementation, tests and benchmarks, plus the report commit.

## 10. Future replacement strategy

- **Painter:** implement `PaintSource` for the new document engine (its `Scene` type must be what the rasteriser accepts; today `anyrender::Scene`, which Blitz's painter and Skia both already speak). Then `blitz-paint` and `anyrender` leave `wega-adapter-blitz` with it. Nothing in the engine changes.
- **Rasteriser:** a replacement for Skia implements `wega-adapter-skia`'s four calls (`start`, `take`, `wait`, `release`) over the same `FrameRequest`/`Painted`/`Pixels` types; `View` holds the concrete type, so swapping is a one-line type change rather than a generic parameter until a second backend exists.
- **Before either:** fix the idle repaint (invalidation from canvas/WebGL/media/resource producers, then drop the unconditional timer-task flag), give the raster thread a resizable or pooled renderer (resize ~20× cost), then consider damage rectangles and scene reuse with the numbers above as the baseline.
