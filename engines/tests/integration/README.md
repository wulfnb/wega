# tests/integration

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Cross-subsystem integration tests.

## Intended responsibilities

- Pipeline tests across parser, style, layout and paint
- Boundary tests for adapters

## Expected dependencies

Public APIs of the crates under test. Specifically: Public crate APIs, `tests/fixtures`.

## Dependencies to avoid

Reaching into private internals; production crates depending on test crates.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
