# graphics/display-list

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Backend-neutral display list representation.

## Intended responsibilities

- Paint operations, clips, transforms, effects
- Serialization and diffing of display lists

## Expected dependencies

`std` only for now (`foundation/*` deferred). The display-list contract is independent of `layout/*`, `dom/*` and `css/*` (it may mirror `anyrender::PaintScene`).

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `javascript/*`, `services/*`; Skia/GPU types outside `adapters/skia` and the dedicated GPU leaf. Also: Any rasterizer or GPU backend.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
