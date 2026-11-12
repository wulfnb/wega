# html/treebuilder

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

HTML tree construction (insertion modes, fragment parsing).

## Intended responsibilities

- Tree construction algorithm over parser tokens
- Fragment and `innerHTML` parsing entry points

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract: no dependency on `dom/*`, `css/*` or `layout/*`; tree construction targets a sink trait that an adapter implements. Within this group: `html/parser`.

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz types in public APIs. Also: Network or resource loading.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
