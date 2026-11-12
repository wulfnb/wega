# services/permissions

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Permission model and prompts.

## Intended responsibilities

- Permission state storage and queries
- Prompt requests surfaced to the UI layer

## Expected dependencies

`foundation/*`; platform and network crates in `engines/platform` and `engines/network` through narrow traits. Specifically: `services/security`.

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`. Also: Implementation of the permission UI.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
