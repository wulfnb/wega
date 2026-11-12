# layout/invalidation

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Layout invalidation and incremental relayout.

## Intended responsibilities

- Dirty-bit propagation and relayout roots
- Reflow scheduling inputs

## Expected dependencies

`foundation/*`, `css/values`, `css/style-engine` (computed style input), `dom/core`. Specifically: `layout/core`, `dom/mutation`.

## Dependencies to avoid

`graphics/*`, `javascript/*`, `services/*`, `adapters/*`, Blitz, Taffy-specific types in public APIs, V8. Also: `css/invalidation` internals.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
