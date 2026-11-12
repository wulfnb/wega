# graphics/display-list

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Backend-neutral display list representation.

## Intended responsibilities

- Paint operations, clips, transforms, effects
- Serialization and diffing of display lists

## Expected dependencies

`foundation/*`, `foundation/geometry`; `layout/core` fragments only for display-list building. Specifically: `foundation/geometry`.

## Dependencies to avoid

`dom/*`, `css/*`, `javascript/*`, `services/*`, direct Skia/GPU types outside the dedicated leaves. Also: Any rasterizer or GPU backend.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
