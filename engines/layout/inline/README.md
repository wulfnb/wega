# layout/inline

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Inline formatting context layout.

## Intended responsibilities

- Line box construction and baseline alignment
- Inline-level box fragmentation

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract with plain input/output types: no dependency on `css/*`, `dom/*` or `graphics/*`; an adapter converts style and DOM data (precedent: `stylo_taffy` inside Blitz). Within this group: `layout/core`, `layout/text`.

## Dependencies to avoid

`css/*`, `dom/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz, Taffy or Parley types in public APIs.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
