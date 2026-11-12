# css/invalidation

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Style invalidation from DOM and stylesheet changes.

## Intended responsibilities

- Dependency tracking for selectors and attributes
- Restyle hints for changed subtrees

## Expected dependencies

`foundation/*`, `dom/core` where selectors and style need the tree. Specifically: `css/selectors`, `css/style-engine`, `dom/mutation`.

## Dependencies to avoid

`layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (including Stylo directly), Blitz, V8. Also: `layout/invalidation` (coordinate via events, not imports).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
