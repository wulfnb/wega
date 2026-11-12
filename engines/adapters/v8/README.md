# adapters/v8

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Adapter around V8 (via vendored Deno `deno_core`/`deno_v8`).

## Intended responsibilities

- Implement the `javascript/runtime` contract using V8
- Isolate V8 handles and GC details

## Expected dependencies

The wrapped third-party engine plus the Wega-owned trait it implements. Specifically: `engines/vendor/deno`, `engines/v8-spike` learnings.

## Dependencies to avoid

Leaking third-party types into non-adapter crates; reverse dependencies from the wrapped library into Wega internals.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
