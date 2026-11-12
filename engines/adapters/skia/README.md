# adapters/skia

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Adapter around Skia bindings.

## Intended responsibilities

- Provide surfaces and canvases to `graphics/raster-skia`
- Isolate `skia-bindings` build concerns

## Expected dependencies

The wrapped third-party engine plus the Wega-owned trait it implements. Specifically: `engines/vendor/blitz/skia-bindings`.

## Dependencies to avoid

Leaking third-party types into non-adapter crates; reverse dependencies from the wrapped library into Wega internals.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
