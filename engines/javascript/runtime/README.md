# javascript/runtime

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Engine-neutral JavaScript runtime abstraction.

## Intended responsibilities

- Realm, context, job-queue and module-loading interfaces
- Script execution and error reporting contracts

## Expected dependencies

`foundation/*`, `dom/core` (through bindings leaves only). Specifically: `foundation/*`, `adapters/v8` (implementation only).

## Dependencies to avoid

Direct use of V8 types outside `adapters/v8`, `graphics/*`, `services/*`. Also: V8 types in the public API.

## Phase 2 decision

Planned package `wega-js-runtime` (`wega-js-*` naming). Must be validated against `v8.rs`/`script.rs` before creation.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
