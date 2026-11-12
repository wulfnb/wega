# services/security

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Security policy enforcement.

## Intended responsibilities

- CSP, mixed content, certificate policy hooks
- Origin and site-isolation primitives

## Expected dependencies

`foundation/*`; platform and network crates in `engines/platform` and `engines/network` through narrow traits. Specifically: `foundation/*`.

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
