# graphics/display-list

**Status: IMPLEMENTED (Phase 1 split)** — crate `wega-display-list`

## Purpose

Backend-neutral display list representation.

## Intended responsibilities

- Paint operations, clips, transforms, effects
- Serialization and diffing of display lists

## Expected dependencies

`std` only for now (`foundation/*` deferred). The display-list contract is independent of `layout/*`, `dom/*` and `css/*` (it may mirror `anyrender::PaintScene`).

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `javascript/*`, `services/*`; Skia/GPU types outside `adapters/skia` and the dedicated GPU leaf. Also: Any rasterizer or GPU backend.

## Current status

Implemented as `wega-display-list`, a member of the `engines` workspace. The code moved here from `wega-native` without behavior changes (CSS engine modularization plan, Phase 1); `wega-native` re-exports it under the old paths.

Modules: `list` (items, the list, hit testing), `boxes` (decorations and contents), `stacking` (stacking contexts and paint order).

Actual dependencies: `wega-layout`, `wega-css-values`, `wega-image`, `wega-dom`, `anyrender`, `anyrender_svg`, `kurbo`, `peniko`.

Deviations from the target: Reads `wega_layout::LayoutBox` directly (ARCH-003: `wega-display-list -> wega-layout`, `-> wega-dom`). See [`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).
