# graphics/paint

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Paint-tree traversal producing display lists.

## Intended responsibilities

- Stacking contexts and paint order
- Translating layout fragments into display items

## Expected dependencies

`foundation/*`, `foundation/geometry`; `layout/core` fragments only for display-list building. Specifically: `graphics/display-list`, `layout/core`.

## Dependencies to avoid

`dom/*`, `css/*`, `javascript/*`, `services/*`, direct Skia/GPU types outside the dedicated leaves. Also: Rasterization.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
