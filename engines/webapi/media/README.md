# webapi/media

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Media element and media playback API semantics.

## Intended responsibilities

- Media session, clock and buffering semantics
- Container parsing helpers (WAV, WebM) currently used by the engine

## Expected dependencies

`webapi/core`, `wega-protocol` media types.

## Dependencies to avoid

`dom/*` internals, `graphics/*` internals, `adapters/*` directly.

## Existing code (not migrated)

`engines/wega/src/media/*` (~0.9k lines); `wega-protocol` has `media.rs` wire types.

## Current status

PLANNED — NOT IMPLEMENTED. Placeholder only: no code, no `Cargo.toml`, not in any Cargo workspace. Existing implementation stays in `engines/wega` and is not migrated in Phase 2. See `docs/architecture/crate-initialization-plan.md`.
