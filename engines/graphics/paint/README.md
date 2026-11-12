# graphics/paint

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Paint-tree traversal producing display lists.

## Intended responsibilities

- Stacking contexts and paint order
- Translating layout fragments into display items

## Expected dependencies

`std` only for now (`foundation/*` deferred). The display-list contract is independent of `layout/*`, `dom/*` and `css/*` (it may mirror `anyrender::PaintScene`). Within this group: `graphics/display-list`. Display lists are produced from layout output only through an adapter (e.g. `adapters/blitz`).

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `javascript/*`, `services/*`; Skia/GPU types outside `adapters/skia` and the dedicated GPU leaf. Also: Rasterization.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
