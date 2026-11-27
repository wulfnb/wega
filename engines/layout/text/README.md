# layout/text

**Status: IMPLEMENTED** — crate `wega-layout-text`

## Purpose

Text shaping, measurement and line breaking.

## Responsibilities

- Parley font context and font registration
- Shaping inline content into line boxes and glyph runs

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract with plain input/output types: no dependency on `css/*`, `dom/*` or `graphics/*`; an adapter converts style and DOM data (precedent: `stylo_taffy` inside Blitz). Within this group: `layout/core`.

## Dependencies to avoid

`css/*`, `dom/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz, Taffy or Parley types in public APIs.

## Current status

Implemented as `wega-layout-text`, a member of the `engines` workspace, split out of the single `wega-layout` crate by formatting context (CSS engine modularization plan). Crates are layered with no cycles: `core` → `text` → `inline` → `block` → `flex`/`grid`/`table` → `engine` (`wega-layout`).

Actual dependencies: `wega-layout-core`, `wega-style`, `wega-css-values`, `wega-dom`, `parley`, `anyrender`, `kurbo`.

Deviations from the target: Builds on `ComputedStyle` (`wega-style`, `wega-css-values`) directly rather than a plain layout style from `adapters/style-layout` (target of the modularization plan). Shapes `LayoutBox` children directly. See [`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).
