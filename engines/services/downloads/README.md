# services/downloads

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Download management.

## Intended responsibilities

- Download lifecycle, progress and resume
- File-system handoff through platform traits

## Expected dependencies

`foundation/*`; platform and network crates in `engines/platform` and `engines/network` through narrow traits. Specifically: `services/http`, `engines/platform` (existing).

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
