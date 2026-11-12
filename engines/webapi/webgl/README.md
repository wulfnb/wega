# webapi/webgl

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

WebGL API semantics and resource management.

## Intended responsibilities

- WebGL context, resource and state semantics
- Constants and validation rules

## Expected dependencies

`webapi/core`, `graphics/gpu` (when it exists).

## Dependencies to avoid

`skia-safe` types, `dom/*` internals, `adapters/*` directly.

## Existing code (not migrated)

`engines/wega/src/webgl/*` (~1.6k lines); `webgl/context.rs` references Skia types.

## Current status

PLANNED — NOT IMPLEMENTED. Placeholder only: no code, no `Cargo.toml`, not in any Cargo workspace. Existing implementation stays in `engines/wega` and is not migrated in Phase 2. See `docs/architecture/crate-initialization-plan.md`.
