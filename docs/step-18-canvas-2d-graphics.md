# Step 18 Contract: Canvas 2D Graphics and Context Acceleration

## 1. Overview & Objectives

Step 18 delivers the complete HTML5 Canvas 2D rendering pipeline and graphics engine for Wega:
- **Canvas Engine & Raster Surface Context** (`engines/wega/src/canvas/`):
  - Hardware/raster surface management backed by Skia (`skia_safe::surfaces::raster_n32_premul`).
  - Drawing state stack management (`save`, `restore`, `reset`) preserving styles, line geometry, compositing operations, global alpha, shadows, and transformation matrices.
  - Coordinate transformations: affine 2D transforms (`scale`, `rotate`, `translate`, `transform`, `set_transform`, `reset_transform`).
  - Path builder and vector geometry (`Path2D`, `CanvasPath`): subpaths, line segments, quadratic and cubic Bézier curves, rectangles, and 360-degree oval arcs.
  - Geometry operations: `fill`, `stroke`, `clip`, and point-in-path hit testing (`is_point_in_path`).
  - Styled rendering: hex/rgb/named color parsers, linear gradients, radial gradients, conic gradients, and pattern shaders.
  - Typography and text measurement: `fill_text`, `stroke_text`, `measure_text` with fallback ascent, descent, and width metrics in headless environments.
  - Direct pixel manipulation: `getImageData` and `putImageData` supporting RGBA8888 unpremultiplied pixel readback and writes.
  - Image serialization: `toDataURL` and `toBlob` encoding canvas surfaces to standard PNG data URIs and binary blobs.
  - Canvas tainting & origin cleanliness: propagation of cross-origin taint on `drawImage`, strictly throwing `SecurityError` upon attempted readback of tainted surfaces.
  - Memory accounting and quota enforcement: aggregate pixel memory allocation tracking against engine limits (`limits.max_pixels * 4`), rejecting allocations with `QuotaExceededError` and reclaiming memory on destruction.
- **OffscreenCanvas & Worker Interoperability** (`engines/wega/src/workers/transfer.rs`, `engines/wega/src/dom/semantics.js`):
  - `OffscreenCanvas` constructor for headless and worker thread rendering.
  - `transferControlToOffscreen()` preventing concurrent main-thread rendering on transferred canvases.
  - `OffscreenCanvasTransfer` transferable primitive across isolate and thread boundaries.

## 2. Architecture & Implementation

### Module Layout (`engines/wega/src/canvas/`)
- `context.rs`:
  - `CanvasContext2D`: wraps a Skia `Surface` and `CanvasState`, executes vector drawing operations, manages the state save/restore stack, handles image snapshots and pixel read/write operations, and tracks origin cleanliness.
  - `CanvasEngine`: manages the registry of live canvases by `CanvasId`, enforces global pixel memory limits, and coordinates allocation/deallocation.
- `path.rs`:
  - `CanvasPath`: encapsulates `skia_safe::PathBuilder` for path manipulation, handling 360-degree circle arcs via `add_oval` to prevent degenerate arcs.
- `state.rs`:
  - `CanvasState`: holds active fill/stroke styles, transform matrices, line styles, shadows, font metrics, and compositing blend modes.
- `style.rs`:
  - `CanvasStyle`: parses CSS color strings and represents linear/radial/conic gradient and pattern shader definitions.

### DOM & JavaScript Integration
- `Dom` (`engines/wega/src/dom.rs`):
  - Added `pub canvas_engine: RefCell<CanvasEngine>` with memory limits derived from configured engine limits.
  - Dispatched host operations: `canvasCreate`, `canvasResize`, `canvasDestroy`, `canvasGetMemory`, `canvasExecute`, `canvasToDataURL`, `canvasToBlob`, `canvasGetImageData`, `canvasPutImageData`, `canvasMeasureText`, `canvasTaint`, and `canvasDrawImage`.
  - Encoded structured arguments for `canvasGetImageData` and `canvasPutImageData` via JSON to cleanly observe the host bridge's 3-argument boundary.
- `semantics.js`:
  - Implemented `HTMLCanvasElement`, `OffscreenCanvas`, `CanvasRenderingContext2D`, `OffscreenCanvasRenderingContext2D`, `ImageData`, `Path2D`, `CanvasGradient`, and `CanvasPattern`.

## 3. Verification & Evidence

- Integration test suite `engines/wega/tests/canvas2d.rs`:
  - `canvas_2d_raster_surface_draw_and_pixel_readback` — PASSED
  - `canvas_paths_curves_arcs_and_hit_testing` — PASSED
  - `canvas_text_rendering_and_font_metrics` — PASSED
  - `canvas_to_data_url_and_to_blob_serialization` — PASSED
  - `canvas_resizing_resets_bitmap_and_drawing_state` — PASSED
  - `canvas_tainting_blocks_cross_origin_readback_with_security_error` — PASSED
  - `canvas_memory_accounting_and_quota_enforcement` — PASSED
  - `offscreencanvas_context_and_worker_transfer` — PASSED
  - Total: 8/8 tests passing.
- Unit test suite:
  - All 29 unit tests in `engines/wega` passing.
- Servo binary compatibility:
  - `cargo check --bin wega` passing with 0 errors.
