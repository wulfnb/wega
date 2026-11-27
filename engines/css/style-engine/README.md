# css/style-engine

**Status: IMPLEMENTED (Phase 1 split)** — crate `wega-style`

## Purpose

Cascade, inheritance and computed style resolution.

## Intended responsibilities

- Cascade ordering and origin handling
- Computed style tree production for layout

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract: no dependency on `dom/*` or `layout/*`; element access goes through an abstract view that an adapter implements. Within this group: `css/parser`, `css/selectors`, `css/values`.

## Dependencies to avoid

`dom/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (Stylo is reached only through a future `adapters/stylo`); Blitz, V8. Also: `layout/*`.

## Current status

Implemented as `wega-style`, a member of the `engines` workspace. The code moved here from `wega-native` without behavior changes (CSS engine modularization plan, Phase 1); `wega-native` re-exports it under the old paths.

Modules: `computed_style`, `cascade`, `apply`, `values`, `gradient`, `hints`, `user_agent`, `computed` (serialization for `getComputedStyle`).

Actual dependencies: `wega-css-values`, `wega-css-selectors`, `wega-css-parser`, `wega-dom`.

Deviations from the target: Depends on `wega-dom` for element access and presentational hints. `apply.rs` (687 lines) is over the 600-line limit; it splits per property family in Phase 2. See [`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).
