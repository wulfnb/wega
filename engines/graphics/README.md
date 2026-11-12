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

Default dependencies for this group: `foundation/*`, `foundation/geometry`; `layout/core` fragments only for display-list building.

Dependencies to avoid: `dom/*`, `css/*`, `javascript/*`, `services/*`, direct Skia/GPU types outside the dedicated leaves.
