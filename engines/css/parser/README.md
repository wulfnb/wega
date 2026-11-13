# css/parser

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

CSS tokenizer and stylesheet/declaration parser.

## Intended responsibilities

- Tokenization and grammar parsing
- Stylesheet, rule and declaration structures

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract: no dependency on `dom/*` or `layout/*`; element access goes through an abstract view that an adapter implements. Within this group: `css/values`.

## Dependencies to avoid

`dom/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (Stylo is reached only through a future `adapters/stylo`); Blitz, V8.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
