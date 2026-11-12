# html/parser

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

HTML tokenizer and input-stream handling.

## Intended responsibilities

- Spec-compliant tokenization
- Encoding sniffing and input preprocessing

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract: no dependency on `dom/*`, `css/*` or `layout/*`; tree construction targets a sink trait that an adapter implements.

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz types in public APIs. Also: Tree construction logic (belongs in `treebuilder`).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
