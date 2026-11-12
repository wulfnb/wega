# tests/fixtures

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Shared test inputs (HTML, CSS, JS, expected outputs).

## Intended responsibilities

- Curated fixtures with provenance
- Golden files and reference renders

## Expected dependencies

Public APIs of the crates under test.

## Dependencies to avoid

Reaching into private internals; production crates depending on test crates. Also: Any code dependencies.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
