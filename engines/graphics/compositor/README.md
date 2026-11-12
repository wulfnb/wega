# graphics/compositor

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Layer tree and frame compositing.

## Intended responsibilities

- Compositing layers, scrolling and animation layers
- Frame scheduling interface to the runtime

## Expected dependencies

`std` only for now (`foundation/*` deferred). The display-list contract is independent of `layout/*`, `dom/*` and `css/*` (it may mirror `anyrender::PaintScene`). Within this group: `graphics/display-list`.

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `javascript/*`, `services/*`; Skia/GPU types outside `adapters/skia` and the dedicated GPU leaf. Also: Direct windowing code.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
