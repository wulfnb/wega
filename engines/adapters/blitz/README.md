# adapters/blitz

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Adapter around the vendored Blitz components (`engines/vendor/blitz`).

## Intended responsibilities

- Implement Wega DOM/layout/paint traits using Blitz
- Isolate Blitz types behind the adapter boundary

## Expected dependencies

The wrapped third-party engine plus the Wega-owned trait it implements. Specifically: `engines/vendor/blitz`, target Wega traits.

## Dependencies to avoid

Leaking third-party types into non-adapter crates; reverse dependencies from the wrapped library into Wega internals. Also: Being imported by core subsystems.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
