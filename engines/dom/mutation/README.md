# dom/mutation

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

DOM mutation tracking and observation.

## Intended responsibilities

- Mutation records and observers
- Change notifications consumed by style and layout invalidation

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract: no dependency on `html/*`, `css/*`, `layout/*` or `graphics/*`. Within this group: `dom/core`.

## Dependencies to avoid

`html/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; third-party engines (Blitz, V8) belong only in future `adapters/*`. Also: Direct calls into `css/*` or `layout/*` (use notifications).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
