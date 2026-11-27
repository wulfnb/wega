# layout/engine

**Status: IMPLEMENTED** — crate `wega-layout`

## Purpose

Layout entry point.

## Responsibilities

- `perform_layout`: viewport units, viewport overflow and indefinite percentage heights, then layout of the root
- The dispatcher that sends each box to its formatting context
- Re-exports the public layout API (`wega_layout::*`, `wega_layout::text`)

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract with plain input/output types: no dependency on `css/*`, `dom/*` or `graphics/*`; an adapter converts style and DOM data (precedent: `stylo_taffy` inside Blitz). Within this group: every other layout crate.

## Dependencies to avoid

`css/*`, `dom/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz, Taffy or Parley types in public APIs.

## Current status

Implemented as `wega-layout`, a member of the `engines` workspace, split out of the single `wega-layout` crate by formatting context (CSS engine modularization plan). Crates are layered with no cycles: `core` → `text` → `inline` → `block` → `flex`/`grid`/`table` → `engine` (`wega-layout`).

Actual dependencies: `wega-layout-core`, `wega-layout-text`, `wega-layout-block`, `wega-layout-flex`, `wega-layout-grid`, `wega-layout-table`, `wega-css-values`.

Deviations from the target: None beyond those of the crates it wires together. See [`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).
