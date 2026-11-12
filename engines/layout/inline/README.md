# layout/inline

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Inline formatting context layout.

## Intended responsibilities

- Line box construction and baseline alignment
- Inline-level box fragmentation

## Expected dependencies

`foundation/*`, `css/values`, `css/style-engine` (computed style input), `dom/core`. Specifically: `layout/core`, `layout/text`.

## Dependencies to avoid

`graphics/*`, `javascript/*`, `services/*`, `adapters/*`, Blitz, Taffy-specific types in public APIs, V8.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
