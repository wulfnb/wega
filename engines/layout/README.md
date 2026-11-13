# engines/layout — Layout

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`core`](./core/README.md) — Layout tree, constraints and fragment output shared by all formatting contexts.
- [`block`](./block/README.md) — Block formatting context layout.
- [`flex`](./flex/README.md) — Flexbox layout.
- [`grid`](./grid/README.md) — CSS Grid layout.
- [`inline`](./inline/README.md) — Inline formatting context layout.
- [`text`](./text/README.md) — Text shaping, measurement and line breaking.
- [`invalidation`](./invalidation/README.md) — Layout invalidation and incremental relayout.

Default dependencies for this group: `std` only for now (`foundation/*` deferred). Independent contract with plain input/output types: no dependency on `css/*`, `dom/*` or `graphics/*`; an adapter converts style and DOM data (precedent: `stylo_taffy` inside Blitz).

Dependencies to avoid: `css/*`, `dom/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz, Taffy or Parley types in public APIs.
