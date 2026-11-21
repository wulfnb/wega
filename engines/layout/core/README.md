# layout/core

**Status: IMPLEMENTED (Phase 1 split)** — crate `wega-layout`

## Purpose

Layout tree, constraints and fragment output shared by all formatting contexts.

## Intended responsibilities

- Box/fragment tree and layout constraints
- Formatting-context dispatch

## Expected dependencies

`std` only for now (`foundation/*` deferred). Independent contract with plain input/output types: no dependency on `css/*`, `dom/*` or `graphics/*`; an adapter converts style and DOM data (precedent: `stylo_taffy` inside Blitz).

## Dependencies to avoid

`css/*`, `dom/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz, Taffy or Parley types in public APIs.

## Current status

Implemented as `wega-layout`, a member of the `engines` workspace. The code moved here from `wega-native` without behavior changes (CSS engine modularization plan, Phase 1); `wega-native` re-exports it under the old paths.

Modules: `box_model`, `box_tree`, `engine`, `block`, `inline`, `intrinsic`, `flex`, `grid`, `table`, `common`, `text` (Parley shaping), `controls` (form control decoration).

Actual dependencies: `wega-style`, `wega-css-values`, `wega-css-parser`, `wega-image`, `wega-dom`, `parley`, `anyrender`, `kurbo`, `peniko`.

Deviations from the target: Builds boxes from `wega-dom` and `ComputedStyle` directly (architecture check ARCH-003: `wega-layout -> wega-dom`). Target: a layout style produced by `adapters/style-layout`. Formatting contexts recurse through `engine::layout_box`, so block, inline, flex, grid and table stay modules of this crate until a dispatch trait lets them split. `block.rs` (653 lines) is over the 600-line limit. See [`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).
