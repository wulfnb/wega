# css/parser

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

CSS tokenizer and stylesheet/declaration parser.

## Intended responsibilities

- Tokenization and grammar parsing
- Stylesheet, rule and declaration structures

## Expected dependencies

`foundation/*`, `dom/core` where selectors and style need the tree. Specifically: `css/values`, `foundation/errors`.

## Dependencies to avoid

`layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (including Stylo directly), Blitz, V8.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
