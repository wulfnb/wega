# graphics/compositor

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Layer tree and frame compositing.

## Intended responsibilities

- Compositing layers, scrolling and animation layers
- Frame scheduling interface to the runtime

## Expected dependencies

`foundation/*`, `foundation/geometry`; `layout/core` fragments only for display-list building. Specifically: `graphics/display-list`, `foundation/geometry`.

## Dependencies to avoid

`dom/*`, `css/*`, `javascript/*`, `services/*`, direct Skia/GPU types outside the dedicated leaves. Also: Direct windowing code.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
