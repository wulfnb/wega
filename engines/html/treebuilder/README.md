# html/treebuilder

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

HTML tree construction (insertion modes, fragment parsing).

## Intended responsibilities

- Tree construction algorithm over parser tokens
- Fragment and `innerHTML` parsing entry points

## Expected dependencies

`foundation/*`, `dom/core`. Specifically: `html/parser`, `dom/core`, `dom/mutation`.

## Dependencies to avoid

`css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`. Also: Network or resource loading.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
