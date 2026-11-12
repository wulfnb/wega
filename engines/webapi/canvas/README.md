# webapi/canvas

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Canvas 2D API semantics (state, paths, styles, compositing rules).

## Intended responsibilities

- Canvas 2D state machine, path and style semantics
- Drawing through an abstract surface interface, not Skia types

## Expected dependencies

`webapi/core`, `foundation/geometry` (if it exists), a Wega-owned drawing-surface interface implemented by `adapters/skia`.

## Dependencies to avoid

`skia-safe` and any other Skia types (owned solely by `adapters/skia`), `dom/*` internals.

## Existing code (not migrated)

`engines/wega/src/canvas/*` (~1.3k lines). It currently calls `skia_safe` directly (about 19 references); a surface interface must be validated against it before any extraction.

## Current status

PLANNED — NOT IMPLEMENTED. Placeholder only: no code, no `Cargo.toml`, not in any Cargo workspace. Existing implementation stays in `engines/wega` and is not migrated in Phase 2. See `docs/architecture/crate-initialization-plan.md`.
