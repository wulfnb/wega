# tools/architecture-check

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Enforce architectural dependency rules.

## Intended responsibilities

- Check allowed/forbidden dependencies from the module map
- Fail CI on layering violations (future)

## Expected dependencies

Standard library, `cargo metadata` output, and file-system access. Specifically: `cargo metadata` output.

## Dependencies to avoid

Being depended on by production engine crates.

## Phase 2 decision

Development-only tool. `cargo_metadata` is permitted as a dependency here and in `tools/dependency-graph`; it must never be a dependency of production crates.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
