# css/values

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

CSS value types, units and computed-value representations.

## Intended responsibilities

- Lengths, colors, numbers, keywords, calc()
- Specified-to-computed value conversion

## Expected dependencies

`foundation/*`, `dom/core` where selectors and style need the tree. Specifically: `foundation/geometry`.

## Dependencies to avoid

`layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (including Stylo directly), Blitz, V8.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
