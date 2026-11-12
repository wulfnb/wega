# layout/text

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Text shaping, measurement and line breaking.

## Intended responsibilities

- Segmentation, bidi, line-break opportunities
- Glyph run measurement via an abstract font interface

## Expected dependencies

`foundation/*`, `css/values`, `css/style-engine` (computed style input), `dom/core`. Specifically: `layout/core`, `foundation/geometry`.

## Dependencies to avoid

`graphics/*`, `javascript/*`, `services/*`, `adapters/*`, Blitz, Taffy-specific types in public APIs, V8. Also: Concrete font rasterization (belongs in `graphics/fonts`).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
