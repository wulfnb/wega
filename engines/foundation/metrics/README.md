# foundation/metrics

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Metrics, tracing and profiling hooks.

## Intended responsibilities

- Counter/gauge/timer interfaces
- Trace span and event-recording interfaces

## Expected dependencies

Rust `std` and small, widely used utility crates only. Specifically: `types`.

## Dependencies to avoid

Any other engine group, `adapters/*`, Blitz, Stylo, Skia, V8, Servo, UI toolkits, OS APIs. Also: Concrete exporters tied to a specific backend.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
