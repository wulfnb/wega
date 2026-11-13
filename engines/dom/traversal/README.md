# dom/traversal

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

DOM traversal and querying utilities.

## Intended responsibilities

- Tree walkers, iterators and node filters
- Ranges and selection traversal helpers

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract: no dependency on `html/*`, `css/*`, `layout/*` or `graphics/*`. Within this group: `dom/core`.

## Dependencies to avoid

`html/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; third-party engines (Blitz, V8) belong only in future `adapters/*`.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
