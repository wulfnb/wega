# foundation/events

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Engine-internal event and message primitives.

## Intended responsibilities

- Event/message envelope types
- Channel and subscription abstractions (traits only)

## Expected dependencies

Rust `std` and small, widely used utility crates only. Specifically: `types`, `ids`.

## Dependencies to avoid

Any other engine group, `adapters/*`, Blitz, Stylo, Skia, V8, Servo, UI toolkits, OS APIs. Also: DOM event semantics (belongs in `dom/events`).

## Phase 2 decision

Deferred: no crate is created until a stable shared interface has multiple real consumers.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
