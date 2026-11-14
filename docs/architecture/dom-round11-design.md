# Round 11 design: rendering and painting ownership (audit and decision)

Scope: Wega-owned orchestration of the paint path through narrow interfaces, keeping Blitz's painter (`blitz_paint`) and the Skia backend. No new painter, no display-list abstraction, no second render tree. Branch `feature/dom-rounds-9-12` (the branch named in the task text was merged into `main` by PR #63). Local only.

## 0. Baseline

- HEAD `fb48f4a` (Round 10 report), working tree clean, 5.3 GiB free (release builds are disk-constrained), load average 1.7–5.
- Engines workspace: 13 members, 24 internal edges, 0 checker violations. Round 10 suite: 485 passed, 0 failed, 9 ignored (re-run after this round).
- Direct Blitz-reference lines in engine `src/` (Rounds 6–10 grep): 83.
- Engine crate dependencies relevant here: `blitz-paint` (features `scrollbars`) and `anyrender` are used by exactly one file, `view.rs`; `skia-safe` by canvas/WebGL; `wega-adapter-skia` for rasterisation.
- Platform: macOS (Apple M4) only; CPU rasterisation (`anyrender_skia::SkiaImageRenderer`); no GPU path exists in the engine.

## 1. The pipeline and who owns each stage

The engine is a separate process (`engines/wega/src/main.rs`); the browser UI (root `src/`, other workspace) owns the window and its surface and receives pixels over a pipe or a two-slot shared frame store. "Blitz" is the vendored `blitz-dom`/`blitz-paint`; the display list is `anyrender::Scene` (a backend-neutral recording the painter writes and the rasteriser replays).

| Stage | Owner | Where | State |
|---|---|---|---|
| DOM → style → layout | Wega contracts over Blitz (Rounds 2–10) | `View::record_paint` calls `flush_layout` twice | Blitz |
| Viewport/scale configuration | Wega (`ViewportConfig`, Round 10) → Blitz `set_viewport`; paint scale is `hidpi_scale * zoom` | `view.rs:2313` | Blitz `Viewport`; the public `begin_paint` still takes Blitz's `Viewport`/`ColorScheme` (re-exported from `wega-engine`) and an untyped `Option<(u8, u64)>` target |
| Script frame callbacks (`requestAnimationFrame`) | Wega (V8 adapter) | between the two flushes in `record_paint` | JS realm |
| Paint preparation → display list | **Blitz** `blitz_paint::paint_scene` writes backgrounds, borders, text (Parley glyph runs), images, gradients, shadows, filters, clips, transforms, opacity layers and scroll offsets into an `anyrender::Scene`; it records nothing (empty scene) while render-blocking resources are pending | `view.rs:2372` (the engine calls it directly; this is the only `blitz_paint` and `anyrender` use in the engine) | scene is created per frame and moved to the raster thread (no reuse, no copy) |
| Rasterisation | **Skia** via `anyrender_skia::SkiaImageRenderer`, owned by `wega-adapter-skia::Raster` on a dedicated thread; one frame in flight; renderer reused while the size is unchanged | `adapters/skia/src/lib.rs` | CPU RGBA surface; full-surface repaint every frame (`reset()` then replay; no damage rectangles anywhere) |
| Frame submission | Wega: `Painted { serial, width, height, pixels }`, `Pixels::Owned(Vec<u8>)` (pipe) or `Pixels::Shared { slot, sequence }` (the raster thread writes straight into the mapped slot; no copy) | adapter + `main.rs:518-560` | two slots with credit tracking in `main.rs` |
| Compositing / window surface | the browser UI process (egui/winit, root `src/`) | not in the engines workspace | untouched |
| Canvas 2D / WebGL | Wega engine, directly on `skia-safe` (documented temporary exception); bitmaps reach the page as images through host ops, not through the painter | `canvas/`, `webgl/` | |
| Video / media surfaces | Wega `media/`; frames are delivered like images | | |
| Images | Blitz image cache (decode/budget patches), painted by `blitz_paint`; `release_image_cache` on freeze | | |
| Clip, transforms, opacity, text rendering | `blitz_paint` (and Parley for glyphs), replayed by Skia | | |
| Scrolling | Blitz scroll state; `paint_scene` offsets by `viewport_scroll` | | |
| Dirty tracking | Wega: `View::dirty` (30 write sites), `needs_paint() = dirty && !frozen`; each recorded frame sets `dirty = another_frame (rAF pending) || doc.is_animating()` | `view.rs` | one bool |
| Frame scheduling | `main.rs`: at most one paint in flight, 33 ms minimum interval, slot credits, `view.next_wakeup()` | engine process | |
| Resize | embedder sends a new viewport; the next paint passes the new size; the raster thread recreates its renderer when the size changes; shared slots are capped at 4096×2160×4 | | |
| Surface lifecycle | `freeze(true)` releases the image cache and sends `Release` (drops the renderer, releases shared pages, purges Skia caches, trims the allocator); worker panic → renderer dropped, error result | | |

## 2. Findings that shape the design

1. **The engine names the painter and the display-list crate.** `record_paint` calls `blitz_paint::paint_scene` and returns `anyrender::Scene` (so `blitz-paint` and `anyrender` are direct engine dependencies for one call site), after hand-building paint-side state (`set_style_threading`, viewport) on the Blitz document through `Deref`.
2. **Typed frames are missing at the boundary.** `begin_paint(viewport, serial, Option<(u8, u64)>)` takes Blitz's `Viewport` and an untyped tuple; `Raster::start(serial, width, height, scene, Option<(u8, u64)>)` takes five positional values.
3. **Dirty tracking is a bare `bool` with 30 writers.**
4. **Idle pages repaint continuously (pre-existing, measured).** After a paint a page reports `needs_paint() == false`; one `poll()` later it is `true` again, for plain pages, pages with a script and pages with a 60-second timer. Cause: `poll()` marks the view dirty unconditionally in the timer task (`view.rs`, `Task::Timers`). Because every raster completion wakes the engine loop, which polls, an idle page repaints at the 33 ms frame cap indefinitely. Canvas and WebGL draws do not invalidate on their own (their host calls return before the invalidating ops), media frames and resource loads (`handle_messages`) rely on the same unconditional flag, so removing it is not a safe local change. **Not fixed this round**; pinned by a test and recorded with the cost.
5. Painting is skipped (empty scene) while render-blocking stylesheets load; nothing re-marks the view dirty when they arrive other than the unconditional flag above.
6. No damage tracking exists: a one-pixel change repaints and rasterises the whole surface.

## 3. Decision

- **`wega_dom::render::PaintSource`** (in `wega-dom`, no new crate; consumers are the engine and the Blitz adapter, as for every Round 8–10 contract): `trait PaintSource: LayoutControl { type Scene: Send + 'static; fn prepare_frame(&mut self, viewport, time); fn record_scene(&mut self, viewport) -> Self::Scene; fn is_animating(&self) -> bool }`. The associated `Scene` type means the contract names no painter, display-list or graphics type. `BlitzDocument` implements it with `Scene = anyrender::Scene`, moving the `blitz_paint::paint_scene` call, the scene allocation and the paint-time style-threading hint into the adapter. Result: the engine no longer depends on `blitz-paint` or `anyrender` (the adapter does, and the boundary test lists them); `engines/Cargo.lock` changes by those entries only.
- **Reuse `ViewportConfig` (Round 10) as the render viewport.** Convention (documented on the type): width/height are device pixels, `scale` is device pixels per CSS pixel before zoom, `zoom` multiplies it, the painter scale is `scale * zoom`, `dark` selects `prefers-color-scheme`. No second viewport type.
- **Typed frame submission in `wega-adapter-skia`:** `FrameTarget { Owned, Shared { slot, sequence } }` and `FrameRequest { serial, width, height, target }` replace the tuples; `Raster::start(request, scene)`. `Pixels`/`Painted` stay (already typed, no Skia types). `View::begin_paint(ViewportConfig, serial, FrameTarget)` replaces the Blitz-typed signature; `main.rs` is the only caller. The re-exports of Blitz's `Viewport`/`ColorScheme` from `wega-engine` stay for compatibility but are no longer used by the paint API.
- **`FrameState` (engine):** one small struct owning the dirty flag with named transitions (`invalidate`, `recorded(another_frame)`, `needs_paint(frozen)`), replacing the 30 raw writes; behaviour identical.
- **No `Rasterizer` trait and no display-list abstraction.** One backend, one consumer; making `View` generic would add churn without a migration path. The Skia adapter is not expanded beyond the typed request; its public surface is already a complete raster contract (`Raster`, `Painted`, `Pixels`). A replacement backend implements the same four calls (`start`, `take`, `wait`, `release`) behind the same types, and a replacement painter implements `PaintSource`.
- **No damage tracking, no scene reuse:** neither has a consumer today; reuse would need a return channel to the raster thread. Recorded as future work with the measured per-frame costs.
- **Not touched:** canvas/WebGL/media (not part of the painter path), the UI process and window surface (outside this workspace), GPU (none in the engine).

## 4. Risks

- `begin_paint`'s signature changes (one in-repo caller). Documented in the report.
- Moving the threading hint into `prepare_frame` must not change when it is applied: it stays paint-only (geometry reads still use whatever mode the last paint set).
- The idle-repaint behaviour is deliberately preserved.
