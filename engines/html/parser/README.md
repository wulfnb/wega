# html/parser

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

HTML tokenizer and input-stream handling.

## Intended responsibilities

- Spec-compliant tokenization
- Encoding sniffing and input preprocessing

## Expected dependencies

`foundation/*`, `dom/core`. Specifically: `foundation/errors`.

## Dependencies to avoid

`css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`. Also: Tree construction logic (belongs in `treebuilder`).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
