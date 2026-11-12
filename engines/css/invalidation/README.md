# css/invalidation

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Style invalidation from DOM and stylesheet changes.

## Intended responsibilities

- Dependency tracking for selectors and attributes
- Restyle hints for changed subtrees

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract: no dependency on `dom/*` or `layout/*`; element access goes through an abstract view that an adapter implements. Within this group: `css/selectors`, `css/style-engine`.

## Dependencies to avoid

`dom/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (Stylo is reached only through a future `adapters/stylo`); Blitz, V8. Also: `layout/invalidation` (coordinate via events, not imports).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
