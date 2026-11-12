# graphics/raster-skia

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Skia-based rasterization of display lists.

## Intended responsibilities

- Executing display lists on Skia surfaces
- Glyph and path rasterization via Skia

## Expected dependencies

`foundation/*`, `foundation/geometry`; `layout/core` fragments only for display-list building. Specifically: `graphics/display-list`, `adapters/skia`.

## Dependencies to avoid

`dom/*`, `css/*`, `javascript/*`, `services/*`, direct Skia/GPU types outside the dedicated leaves. Also: Layout, DOM or CSS types.

## Phase 2 decision

Documentation-only and merged into `adapters/skia` (`wega-adapter-skia`), which is the sole Skia backend owner. Canvas API semantics live in `webapi/canvas`, not here.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
