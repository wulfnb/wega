# css/style-engine

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Cascade, inheritance and computed style resolution.

## Intended responsibilities

- Cascade ordering and origin handling
- Computed style tree production for layout

## Expected dependencies

`foundation/*`, `dom/core` where selectors and style need the tree. Specifically: `css/parser`, `css/selectors`, `css/values`, `dom/core`.

## Dependencies to avoid

`layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (including Stylo directly), Blitz, V8. Also: `layout/*`.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
