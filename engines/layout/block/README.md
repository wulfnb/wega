# layout/block

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Block formatting context layout.

## Intended responsibilities

- Block flow, margins, collapsing, floats
- Normal-flow positioning

## Expected dependencies

`foundation/*`, `css/values`, `css/style-engine` (computed style input), `dom/core`. Specifically: `layout/core`.

## Dependencies to avoid

`graphics/*`, `javascript/*`, `services/*`, `adapters/*`, Blitz, Taffy-specific types in public APIs, V8.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
