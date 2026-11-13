# adapters/skia

**Status: IMPLEMENTED (raster only) — package `wega-adapter-skia`**

## Purpose

Adapter around Skia bindings.

## Intended responsibilities

- Provide surfaces and canvases to `graphics/raster-skia`
- Isolate `skia-bindings` build concerns

## Expected dependencies

The wrapped third-party engine plus the Wega-owned trait it implements. Specifically: `engines/vendor/blitz/skia-bindings`.

## Dependencies to avoid

Leaking third-party types into non-adapter crates; reverse dependencies from the wrapped library into Wega internals.

## Phase 2 decision

Planned package `wega-adapter-skia`: the sole owner of Skia (`skia-safe`, `anyrender_skia`) backend code. Canvas API semantics stay in `webapi/canvas`; Skia types must not appear in its public API.

## Current status

IMPLEMENTED for the raster path: `Raster` (dedicated `wega-raster` thread, `anyrender::Scene` in, RGBA `Pixels` out) moved from `engines/wega/src/raster.rs`. Canvas 2D (`canvas/*`) and WebGL's PNG encoding still use `skia-safe` directly inside `wega-engine` as documented temporary exceptions; this crate therefore owns the rasterisation backend, not yet every Skia call. See `docs/architecture/module-map.md`.
