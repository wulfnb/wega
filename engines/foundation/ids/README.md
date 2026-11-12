# foundation/ids

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Strongly typed identifiers for engine entities.

## Intended responsibilities

- Tab, frame, node, request, process and worker ID newtypes
- ID allocation and generation counters

## Expected dependencies

Rust `std` and small, widely used utility crates only. Specifically: `types`.

## Dependencies to avoid

Any other engine group, `adapters/*`, Blitz, Stylo, Skia, V8, Servo, UI toolkits, OS APIs.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
