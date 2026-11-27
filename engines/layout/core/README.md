# layout/core

**Status: IMPLEMENTED** — crate `wega-layout-core`

## Purpose

Layout tree, constraints and fragment output shared by all formatting contexts.

## Responsibilities

- Box tree construction from `wega-dom` and `ComputedStyle` (`box_tree`)
- Box model, line boxes and glyph runs (`box_model`)
- Layout context and the formatting-context dispatch hook (`context`): formatting contexts recurse through `layout_box`, which calls the dispatcher `wega-layout` installs
- Subtree translation, overflow and anonymous-box style (`geometry`)
- Form control decoration (`controls`) and resource hooks (`resources`)

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract with plain input/output types: no dependency on `css/*`, `dom/*` or `graphics/*`; an adapter converts style and DOM data (precedent: `stylo_taffy` inside Blitz). Within this group: none.

## Dependencies to avoid

`css/*`, `dom/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz, Taffy or Parley types in public APIs.

## Current status

Implemented as `wega-layout-core`, a member of the `engines` workspace, split out of the single `wega-layout` crate by formatting context (CSS engine modularization plan). Crates are layered with no cycles: `core` → `text` → `inline` → `block` → `flex`/`grid`/`table` → `engine` (`wega-layout`).

Actual dependencies: `wega-style`, `wega-css-values`, `wega-css-parser`, `wega-image`, `wega-dom`, `parley`, `anyrender`, `kurbo`, `peniko`.

Deviations from the target: Builds on `ComputedStyle` (`wega-style`, `wega-css-values`) directly rather than a plain layout style from `adapters/style-layout` (target of the modularization plan). Builds boxes from `wega-dom` (ARCH-003: `wega-layout-core -> wega-dom`). `GlyphRun` holds Parley font data. See [`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).
