# adapters/stylo

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Adapter around Stylo for CSS style resolution.

## Intended responsibilities

- Implement the `css/style-engine` contract using Stylo
- Translate DOM/element views for Stylo

## Expected dependencies

The wrapped third-party engine plus the Wega-owned trait it implements. Specifically: `engines/vendor/blitz/stylo`, `css/*` traits.

## Dependencies to avoid

Leaking third-party types into non-adapter crates; reverse dependencies from the wrapped library into Wega internals.

## Phase 2 decision

Planned package `wega-adapter-stylo` (`wega-adapter-*` naming).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
