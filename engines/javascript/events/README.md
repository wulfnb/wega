# javascript/events

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

JavaScript-facing event and callback plumbing.

## Intended responsibilities

- Event listener bridging into script callbacks
- Microtask and task-source integration

## Expected dependencies

`foundation/*`, `dom/core` (through bindings leaves only). Specifically: `javascript/runtime`, `dom/events`.

## Dependencies to avoid

Direct use of V8 types outside `adapters/v8`, `graphics/*`, `services/*`.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
