# javascript/dom-bindings

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

JavaScript bindings for DOM interfaces.

## Intended responsibilities

- Wrapper objects for DOM nodes and collections
- Lifetime and GC integration with the DOM

## Expected dependencies

`foundation/*`, `dom/core` (through bindings leaves only). Specifically: `javascript/runtime`, `javascript/bindings`, `dom/*`.

## Dependencies to avoid

Direct use of V8 types outside `adapters/v8`, `graphics/*`, `services/*`. Also: `css/*`, `layout/*` internals.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
