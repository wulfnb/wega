# tests/performance

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Performance and regression benchmarks.

## Intended responsibilities

- Layout/style/paint microbenchmarks
- Page-load and memory regression checks

## Expected dependencies

Public APIs of the crates under test. Specifically: `tests/fixtures`, `foundation/metrics`.

## Dependencies to avoid

Reaching into private internals; production crates depending on test crates.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
