# graphics/fonts

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Font discovery, loading and glyph data.

## Intended responsibilities

- System/web font enumeration and matching
- Glyph metrics and outlines for layout and raster

## Expected dependencies

`foundation/*`, `foundation/geometry`; `layout/core` fragments only for display-list building. Specifically: `foundation/*`.

## Dependencies to avoid

`dom/*`, `css/*`, `javascript/*`, `services/*`, direct Skia/GPU types outside the dedicated leaves. Also: Layout algorithms.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
