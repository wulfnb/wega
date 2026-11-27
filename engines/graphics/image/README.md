# graphics/image

**Status: IMPLEMENTED (Phase 1 split)** — crate `wega-image`

## Purpose

Image decoding, resizing and caching.

## Intended responsibilities

- Format decoding and color management
- Decoded-image cache

## Expected dependencies

`std` only for now (`foundation/*` deferred). The display-list contract is independent of `layout/*`, `dom/*` and `css/*` (it may mirror `anyrender::PaintScene`).

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `javascript/*`, `services/*`; Skia/GPU types outside `adapters/skia` and the dedicated GPU leaf.

## Current status

Implemented as `wega-image`, a member of the `engines` workspace. The code moved here from `wega-native` without behavior changes (CSS engine modularization plan, Phase 1); `wega-native` re-exports it under the old paths.

Modules: raster decoding with shared budgets, `svg` (bounded SVG decoding), `svg_graphic` (inline SVG serialization), `fonts`. Blitz licenses are in `licenses/`.

Actual dependencies: `wega-dom`, `image`, `usvg`, `anyrender_svg`, `flate2`, `parley`, `kurbo`, `peniko`.

Deviations from the target: None from the target list. See [`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).
