# dom/events

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

DOM event model (dispatch, propagation, listeners).

## Intended responsibilities

- Event objects and phases (capture, target, bubble)
- Listener registration and dispatch algorithm

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract: no dependency on `html/*`, `css/*`, `layout/*` or `graphics/*`. Within this group: `dom/core`.

## Dependencies to avoid

`html/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; third-party engines (Blitz, V8) belong only in future `adapters/*`. Also: Script-engine types (listeners are abstract callbacks).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
