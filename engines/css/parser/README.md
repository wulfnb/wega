# css/parser

**Status: IMPLEMENTED (Phase 1 split)** — crate `wega-css-parser`

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

Implemented as `wega-css-parser`, a member of the `engines` workspace. The code moved here from `wega-native` without behavior changes (CSS engine modularization plan, Phase 1); `wega-native` re-exports it under the old paths.

Modules: `stylesheet`, `rules` (at-rules), `declarations`, `media`, `shorthand`, `variables`, `properties`.

Actual dependencies: `wega-css-values`, `wega-css-selectors`, `cssparser`.

Deviations from the target: None from the target list. Declarations are still strings (typed declarations are Phase 2). See [`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).
