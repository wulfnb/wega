# dom/events

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

DOM event model (dispatch, propagation, listeners).

## Intended responsibilities

- Event objects and phases (capture, target, bubble)
- Listener registration and dispatch algorithm

## Expected dependencies

`foundation/*`. Specifically: `dom/core`, `foundation/events`.

## Dependencies to avoid

`css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`, Blitz, V8. Also: Script-engine types (listeners are abstract callbacks).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
