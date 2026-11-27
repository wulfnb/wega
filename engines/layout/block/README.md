# layout/block

**Status: IMPLEMENTED** — crate `wega-layout-block`

## Purpose

Block formatting context layout.

## Responsibilities

- Block flow, floats, margin collapsing and positioned children (`block`)
- Container sizing shared by flex, grid and table (`container`)
- Text centering in single-line form controls (`control_text`)

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract with plain input/output types: no dependency on `css/*`, `dom/*` or `graphics/*`; an adapter converts style and DOM data (precedent: `stylo_taffy` inside Blitz). Within this group: `layout/core`, `layout/inline`.

## Dependencies to avoid

`css/*`, `dom/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz, Taffy or Parley types in public APIs.

## Current status

Implemented as `wega-layout-block`, a member of the `engines` workspace, split out of the single `wega-layout` crate by formatting context (CSS engine modularization plan). Crates are layered with no cycles: `core` → `text` → `inline` → `block` → `flex`/`grid`/`table` → `engine` (`wega-layout`).

Actual dependencies: `wega-layout-core`, `wega-layout-inline`, `wega-style`, `wega-css-values`.

Deviations from the target: Builds on `ComputedStyle` (`wega-style`, `wega-css-values`) directly rather than a plain layout style from `adapters/style-layout` (target of the modularization plan). See [`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).
