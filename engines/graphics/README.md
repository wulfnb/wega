# engines/graphics — Graphics

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`display-list`](./display-list/README.md) — Backend-neutral display list representation.
- [`paint`](./paint/README.md) — Paint-tree traversal producing display lists.
- [`raster-skia`](./raster-skia/README.md) — Skia-based rasterization of display lists.
- [`compositor`](./compositor/README.md) — Layer tree and frame compositing.
- [`gpu`](./gpu/README.md) — GPU abstraction and resource management.
- [`image`](./image/README.md) — Image decoding, resizing and caching.
- [`fonts`](./fonts/README.md) — Font discovery, loading and glyph data.

Default dependencies for this group: `std` only for now (`foundation/*` deferred). The display-list contract is independent of `layout/*`, `dom/*` and `css/*` (it may mirror `anyrender::PaintScene`).

Dependencies to avoid: `dom/*`, `css/*`, `layout/*`, `javascript/*`, `services/*`; Skia/GPU types outside `adapters/skia` and the dedicated GPU leaf.
