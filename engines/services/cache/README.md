# services/cache

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

HTTP and resource cache service.

## Intended responsibilities

- Persistent and in-memory cache policy
- Cache Storage backing

## Expected dependencies

`foundation/*`; platform and network crates in `engines/platform` and `engines/network` through narrow traits. Specifically: `services/http`.

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
