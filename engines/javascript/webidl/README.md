# javascript/webidl

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

WebIDL parsing and code generation.

## Intended responsibilities

- WebIDL parsing and model
- Binding code generation templates

## Expected dependencies

`foundation/*`, `dom/core` (through bindings leaves only). Specifically: `foundation/errors`.

## Dependencies to avoid

Direct use of V8 types outside `adapters/v8`, `graphics/*`, `services/*`. Also: Runtime dependence on a JS engine.

## Phase 2 decision

The existing location (`tools/webidl`, plus root bin `wega-webidl`) is kept until the duplicate entry-point relationship is verified. No relocation planned yet.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
