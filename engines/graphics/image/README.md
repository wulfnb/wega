# graphics/image

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Image decoding, resizing and caching.

## Intended responsibilities

- Format decoding and color management
- Decoded-image cache

## Expected dependencies

`std` only for now (`foundation/*` deferred). The display-list contract is independent of `layout/*`, `dom/*` and `css/*` (it may mirror `anyrender::PaintScene`).

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `javascript/*`, `services/*`; Skia/GPU types outside `adapters/skia` and the dedicated GPU leaf.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
