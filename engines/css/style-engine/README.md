# css/style-engine

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Cascade, inheritance and computed style resolution.

## Intended responsibilities

- Cascade ordering and origin handling
- Computed style tree production for layout

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract: no dependency on `dom/*` or `layout/*`; element access goes through an abstract view that an adapter implements. Within this group: `css/parser`, `css/selectors`, `css/values`.

## Dependencies to avoid

`dom/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (Stylo is reached only through a future `adapters/stylo`); Blitz, V8. Also: `layout/*`.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
