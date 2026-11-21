# css/selectors

**Status: IMPLEMENTED (Phase 1 split)** — crate `wega-css-selectors`

## Purpose

CSS selector parsing and matching.

## Intended responsibilities

- Selector grammar, specificity
- Matching against DOM elements via an abstract element trait

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract: no dependency on `dom/*` or `layout/*`; element access goes through an abstract view that an adapter implements. Within this group: `css/parser`.

## Dependencies to avoid

`dom/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (Stylo is reached only through a future `adapters/stylo`); Blitz, V8. Also: Concrete `dom/core` types in the public API where a trait suffices.

## Current status

Implemented as `wega-css-selectors`, a member of the `engines` workspace. The code moved here from `wega-native` without behavior changes (CSS engine modularization plan, Phase 1); `wega-native` re-exports it under the old paths.

Modules: `types`, `parser`, `matching` (specificity and `:has()`).

Actual dependencies: `wega-dom`, `cssparser`.

Deviations from the target: Matches through `wega_dom::DocumentView` directly. Target: an `ElementView` trait implemented in `adapters/dom-css`. See [`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).
