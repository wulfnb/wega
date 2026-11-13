# adapters/v8

**Status: IMPLEMENTED — package `wega-adapter-v8`**

## Purpose

Adapter around V8 (via vendored Deno `deno_core`/`deno_v8`).

## Intended responsibilities

- Implement the `javascript/runtime` contract using V8
- Isolate V8 handles and GC details

## Expected dependencies

The wrapped third-party engine plus the Wega-owned trait it implements. Specifically: `engines/vendor/deno`, `engines/v8-spike` learnings.

## Dependencies to avoid

Leaking third-party types into non-adapter crates; reverse dependencies from the wrapped library into Wega internals.

## Phase 2 decision

Planned package `wega-adapter-v8` (`wega-adapter-*` naming).

## Current status

IMPLEMENTED. Owns the Deno/V8 integration extracted from `wega-engine`: `V8Host` (generic over `ScriptEnvironment`), the op table and extension setup, module loader, WebSocket broker connector, `create_runtime` (shared by documents and workers), startup-snapshot generation and the `RawIsolateGuard` unsafe exception. It does not depend on `wega-engine`; the engine supplies the snapshot sources, bootstrap script and environment. See `docs/architecture/module-map.md`.
