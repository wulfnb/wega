# layout/text

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Text shaping, measurement and line breaking.

## Intended responsibilities

- Segmentation, bidi, line-break opportunities
- Glyph run measurement via an abstract font interface

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract with plain input/output types: no dependency on `css/*`, `dom/*` or `graphics/*`; an adapter converts style and DOM data (precedent: `stylo_taffy` inside Blitz). Within this group: `layout/core`.

## Dependencies to avoid

`css/*`, `dom/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz, Taffy or Parley types in public APIs. Also: Concrete font rasterization (belongs in `graphics/fonts`).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
