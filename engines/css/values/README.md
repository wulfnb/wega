# css/values

**Status: IMPLEMENTED (Phase 1 split)** — crate `wega-css-values`

## Purpose

CSS value types, units and computed-value representations.

## Intended responsibilities

- Lengths, colors, numbers, keywords, calc()
- Specified-to-computed value conversion

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract: no dependency on `dom/*` or `layout/*`; element access goes through an abstract view that an adapter implements.

## Dependencies to avoid

`dom/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (Stylo is reached only through a future `adapters/stylo`); Blitz, V8.

## Current status

Implemented as `wega-css-values`, a member of the `engines` workspace. The code moved here from `wega-native` without behavior changes (CSS engine modularization plan, Phase 1); `wega-native` re-exports it under the old paths.

Modules: `color` (with `color_space` for `color()`), `length` (and the style-pass viewport), `calc`, `keywords`, `box_values`, `gradient`, `grid`, `background`, and `specified`: specified values as the CSSOM serializes them, ported from Stylo `values/specified/*` per family (`length`, `number`, `color`, `font`, `background`, `align`, `box_values`). `wega-css-parser` maps each longhand to these parsers.

Actual dependencies: `cssparser`, `kurbo`.

Deviations from the target: None from the target list. See [`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).
