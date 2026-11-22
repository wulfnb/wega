# css/cache

**Status: PARTIAL** — crate `wega-css-cache`

## Purpose

Style sharing and style-related caches.

## Intended responsibilities

- Matched-rules and computed-style caches
- Cache eviction and memory accounting

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract: no dependency on `dom/*` or `layout/*`; element access goes through an abstract view that an adapter implements. Within this group: `css/style-engine`.

## Dependencies to avoid

`dom/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (Stylo is reached only through a future `adapters/stylo`); Blitz, V8.

## Current status

Implemented as `wega-css-cache`: `SelectorMap` buckets each rule under the id, class or tag
of its selectors' subject compound (or a universal bucket). The cascade (`wega-style`) tests
only the candidate rules, in source order, so matching results are unchanged. Every
`StyleSheet` builds its map at parse time; a map whose rule count differs from the sheet's
is treated as stale and the cascade scans every rule.

Not yet: bloom filters for ancestor combinators, style sharing, invalidation. See
[`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).

Dependencies: `wega-css-selectors`.
