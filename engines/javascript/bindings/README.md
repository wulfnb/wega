# javascript/bindings

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Generated and hand-written binding infrastructure.

## Intended responsibilities

- Shared binding helpers and conversions
- Consuming code generated from WebIDL

## Expected dependencies

`foundation/*`, `dom/core` (through bindings leaves only). Specifically: `javascript/runtime`, `javascript/webidl`.

## Dependencies to avoid

Direct use of V8 types outside `adapters/v8`, `graphics/*`, `services/*`.

## Phase 2 decision

Planned package `wega-js-bindings` (`wega-js-*` naming).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
