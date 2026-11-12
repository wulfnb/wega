# webapi/storage

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Web Storage, IndexedDB and Cache API surfaces.

## Intended responsibilities

- localStorage/sessionStorage semantics
- Storage quota and origin partitioning hooks

## Expected dependencies

`foundation/*`, `javascript/bindings`, `services/*` through traits. Specifically: `webapi/core`, `services/storage`.

## Dependencies to avoid

`graphics/*` internals, `layout/*` internals, `adapters/*` directly.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
