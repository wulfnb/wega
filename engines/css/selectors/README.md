# css/selectors

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

CSS selector parsing and matching.

## Intended responsibilities

- Selector grammar, specificity
- Matching against DOM elements via an abstract element trait

## Expected dependencies

`foundation/*`, `dom/core` where selectors and style need the tree. Specifically: `css/parser`.

## Dependencies to avoid

`layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (including Stylo directly), Blitz, V8. Also: Concrete `dom/core` types in the public API where a trait suffices.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
