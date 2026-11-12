# graphics/gpu

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

GPU abstraction and resource management.

## Intended responsibilities

- Surface, texture and context management
- Backend selection and fallback

## Expected dependencies

`foundation/*`, `foundation/geometry`; `layout/core` fragments only for display-list building. Specifically: `foundation/*`.

## Dependencies to avoid

`dom/*`, `css/*`, `javascript/*`, `services/*`, direct Skia/GPU types outside the dedicated leaves. Also: DOM, CSS or layout types.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
