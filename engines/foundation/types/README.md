# foundation/types

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Shared primitive and newtype definitions used across the engine.

## Intended responsibilities

- Small value types and enums with no behavior tied to a subsystem
- Stable serialization-friendly representations

## Expected dependencies

Rust `std` and small, widely used utility crates only.

## Dependencies to avoid

Any other engine group, `adapters/*`, Blitz, Stylo, Skia, V8, Servo, UI toolkits, OS APIs.

## Phase 2 decision

Deferred: no crate is created until a stable shared interface has multiple real consumers.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
