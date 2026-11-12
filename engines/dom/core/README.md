# dom/core

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

DOM tree data model (nodes, documents, elements, attributes).

## Intended responsibilities

- Node and document storage and tree structure
- Attribute, text, comment and fragment node types

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract: no dependency on `html/*`, `css/*`, `layout/*` or `graphics/*`.

## Dependencies to avoid

`html/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; third-party engines (Blitz, V8) belong only in future `adapters/*`.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
