# tools/dependency-graph

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Generate and visualize the crate dependency graph.

## Intended responsibilities

- Parse `cargo metadata` and emit graphs
- Diff graphs between revisions

## Expected dependencies

Standard library, `cargo metadata` output, and file-system access.

## Dependencies to avoid

Being depended on by production engine crates.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
