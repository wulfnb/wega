# dom/mutation

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

DOM mutation tracking and observation.

## Intended responsibilities

- Mutation records and observers
- Change notifications consumed by style and layout invalidation

## Expected dependencies

`foundation/*`. Specifically: `dom/core`.

## Dependencies to avoid

`css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`, Blitz, V8. Also: Direct calls into `css/*` or `layout/*` (use notifications).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
