# services/http

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

HTTP client service.

## Intended responsibilities

- Request execution, redirects, caching headers
- Interface over the existing `engines/network` crate

## Expected dependencies

`foundation/*`; platform and network crates in `engines/platform` and `engines/network` through narrow traits. Specifically: `engines/network` (existing).

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`. Also: Duplicating `engines/network` code.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
