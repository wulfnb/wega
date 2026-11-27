# layout/table

**Status: IMPLEMENTED** — crate `wega-layout-table`

## Purpose

Table layout.

## Responsibilities

- Row, column and cell sizing, with row and column spans

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract with plain input/output types: no dependency on `css/*`, `dom/*` or `graphics/*`; an adapter converts style and DOM data (precedent: `stylo_taffy` inside Blitz). Within this group: `layout/core`, `layout/inline`, `layout/block`.

## Dependencies to avoid

`css/*`, `dom/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz, Taffy or Parley types in public APIs.

## Current status

Implemented as `wega-layout-table`, a member of the `engines` workspace, split out of the single `wega-layout` crate by formatting context (CSS engine modularization plan). Crates are layered with no cycles: `core` → `text` → `inline` → `block` → `flex`/`grid`/`table` → `engine` (`wega-layout`).

Actual dependencies: `wega-layout-core`, `wega-layout-inline`, `wega-layout-block`, `wega-css-values`.

Deviations from the target: Builds on `ComputedStyle` (`wega-style`, `wega-css-values`) directly rather than a plain layout style from `adapters/style-layout` (target of the modularization plan). Table layout is still basic (Phase 4). See [`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).
