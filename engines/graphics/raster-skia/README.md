# graphics/raster-skia

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Skia-based rasterization of display lists.

## Intended responsibilities

- Executing display lists on Skia surfaces
- Glyph and path rasterization via Skia

## Expected dependencies

`std` only for now (`foundation/*` deferred). The display-list contract is independent of `layout/*`, `dom/*` and `css/*` (it may mirror `anyrender::PaintScene`). Within this group: `graphics/display-list`.

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `javascript/*`, `services/*`; Skia/GPU types outside `adapters/skia` and the dedicated GPU leaf. Also: Layout, DOM or CSS types.

## Phase 2 decision

Documentation-only and merged into `adapters/skia` (`wega-adapter-skia`), which is the sole Skia backend owner. Canvas API semantics live in `webapi/canvas`, not here.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
