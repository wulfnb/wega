# css/cache

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Style sharing and style-related caches.

## Intended responsibilities

- Matched-rules and computed-style caches
- Cache eviction and memory accounting

## Expected dependencies

`foundation/*`, `dom/core` where selectors and style need the tree. Specifically: `css/style-engine`, `foundation/metrics`.

## Dependencies to avoid

`layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (including Stylo directly), Blitz, V8.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
